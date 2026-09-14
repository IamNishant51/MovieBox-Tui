//! Background download jobs for the web UI — TUI parity.
//!
//! The TUI downloads direct files with a resumable segmented engine
//! (`crate::download`) and DASH manifests with `yt-dlp` (progress parsed from
//! `--newline` output). This module mirrors that behavior for the browser and
//! adds HLS (`.m3u8` segment fetching + `ffmpeg` remux), because MovieBox web
//! streams are very often HLS and the old `GET /api/download` answered those
//! with `422`.
//!
//! Flow: `POST /api/downloads` -> job id -> worker task streams to
//! `<data_dir>/downloads/` -> frontend polls `GET /api/downloads` (percent,
//! bytes, speed, ETA — same fields the TUI status line shows) -> user saves
//! via `GET /api/downloads/:id/file` (`Content-Disposition: attachment`).

use super::{check_ssrf_url, infer_download_extension, is_hls_or_dash_url};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

const MAX_JOBS: usize = 20;
const MAX_MANIFEST_BYTES: usize = 10 * 1024 * 1024;

static JOB_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
pub struct DlJob {
    pub id: String,
    pub filename: String,
    /// "file" | "hls" | "dash"
    pub kind: String,
    /// queued|downloading|merging|completed|failed|cancelled
    pub status: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed_bps: f64,
    pub eta_secs: Option<u64>,
    /// 0-100 overall progress (segment/explicit-percent based when total unknown)
    pub progress: f64,
    pub error: Option<String>,
    #[serde(skip)]
    pub url: String,
    #[serde(skip)]
    pub headers: Vec<(String, String)>,
    #[serde(skip)]
    pub cancel: Arc<AtomicBool>,
    #[serde(skip)]
    pub quality: Option<u16>,
}

impl DlJob {
    fn new(
        id: String,
        filename: String,
        kind: &str,
        url: String,
        headers: Vec<(String, String)>,
        quality: Option<u16>,
    ) -> Self {
        Self {
            id,
            filename,
            kind: kind.to_string(),
            status: "queued".to_string(),
            downloaded: 0,
            total: None,
            speed_bps: 0.0,
            eta_secs: None,
            progress: 0.0,
            error: None,
            url,
            headers,
            cancel: Arc::new(AtomicBool::new(false)),
            quality,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled")
    }
}

#[derive(Clone)]
pub struct DlStore {
    inner: Arc<RwLock<HashMap<String, DlJob>>>,
    order: Arc<RwLock<VecDeque<String>>>,
    dir: PathBuf,
}

impl DlStore {
    pub fn new() -> Self {
        let dir = moviebox_tui::config::data_dir()
            .map(|d| d.join("downloads").join("web"))
            .unwrap_or_else(|| std::env::temp_dir().join("moviebox_web_downloads"));
        let _ = std::fs::create_dir_all(&dir);
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            order: Arc::new(RwLock::new(VecDeque::new())),
            dir,
        }
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    pub fn file_path(&self, job: &DlJob) -> PathBuf {
        self.dir.join(&job.filename)
    }

    /// Validate + register a job, spawn its worker. Returns the initial snapshot.
    pub async fn start(
        &self,
        http: reqwest::Client,
        url: String,
        headers: Vec<(String, String)>,
        filename: Option<String>,
        quality: Option<u16>,
    ) -> Result<DlJob, (axum::http::StatusCode, String)> {
        let parsed = check_ssrf_url(&url)?;
        let stem_raw = filename
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(moviebox_tui::download::DEFAULT_STREAM_NAME);
        let stem = moviebox_tui::download::safe_file_stem(stem_raw);
        let kind = if is_hls_or_dash_url(parsed.as_str()) {
            let lower = parsed.as_str().to_ascii_lowercase();
            let path = lower.split(['?', '#']).next().unwrap_or("");
            if path.ends_with(".m3u8") {
                "hls"
            } else {
                "dash"
            }
        } else {
            "file"
        };
        let ext = match kind {
            "hls" | "dash" => "mp4",
            _ => infer_download_extension(parsed.as_str(), None),
        };
        let id = format!(
            "dl_{}_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            JOB_SEQ.fetch_add(1, Ordering::Relaxed) % 10000
        );
        let job = DlJob::new(
            id.clone(),
            format!("{stem}.{ext}"),
            kind,
            url,
            headers,
            quality,
        );

        // Cap stored jobs (drop oldest terminal ones + their files).
        // Active jobs are never evicted, so the list can briefly exceed the cap.
        {
            let mut order = self.order.write().await;
            let mut jobs = self.inner.write().await;
            order.push_back(id.clone());
            jobs.insert(id.clone(), job.clone());
            let mut guard = 0;
            while order.len() > MAX_JOBS && guard < MAX_JOBS + 4 {
                guard += 1;
                let Some(old_id) = order.pop_front() else {
                    break;
                };
                let terminal = jobs.get(&old_id).map(|o| o.is_terminal()).unwrap_or(true);
                if !terminal {
                    order.push_back(old_id);
                    break;
                }
                if let Some(old) = jobs.remove(&old_id) {
                    let _ = std::fs::remove_file(self.file_path(&old));
                }
            }
        }

        let store = self.clone();
        tokio::spawn(async move {
            run_job(store, http, id).await;
        });
        Ok(job)
    }

    pub async fn list(&self) -> Vec<DlJob> {
        let order = self.order.read().await;
        let jobs = self.inner.read().await;
        order
            .iter()
            .filter_map(|id| jobs.get(id).cloned())
            .collect()
    }

    pub async fn get(&self, id: &str) -> Option<DlJob> {
        self.inner.read().await.get(id).cloned()
    }

    /// Reset a terminal/failed job and run it again (reuses stored URL + headers).
    pub async fn retry(&self, http: reqwest::Client, id: &str) -> Option<DlJob> {
        {
            let mut jobs = self.inner.write().await;
            let job = jobs.get_mut(id)?;
            if !job.is_terminal() {
                return Some(job.clone());
            }
            job.status = "queued".to_string();
            job.downloaded = 0;
            job.total = None;
            job.speed_bps = 0.0;
            job.eta_secs = None;
            job.progress = 0.0;
            job.error = None;
            job.cancel = Arc::new(AtomicBool::new(false));
            // Drop any stale partial file so the retry starts clean.
            let _ = std::fs::remove_file(self.file_path(job));
            let _ = std::fs::remove_file(self.dir.join(format!("{id}.ts")));
            let _ = std::fs::remove_file(self.dir.join(format!("{id}.part")));
        }
        // Move retry to the front so it is polled first.
        {
            let mut order = self.order.write().await;
            order.retain(|x| x != id);
            order.push_back(id.to_string());
        }
        let store = self.clone();
        let owned = id.to_string();
        tokio::spawn(async move {
            run_job(store, http, owned).await;
        });
        self.get(id).await
    }

    async fn mutate(&self, id: &str, f: impl FnOnce(&mut DlJob)) {
        if let Some(job) = self.inner.write().await.get_mut(id) {
            f(job);
        }
    }

    /// Cancel a running job (sets flag; worker cleans up) — always succeeds.
    pub async fn cancel(&self, id: &str) -> bool {
        if let Some(job) = self.get(id).await {
            job.cancel.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Remove job record + any (partial) file.
    pub async fn remove(&self, id: &str) -> bool {
        let job = self.inner.write().await.remove(id);
        self.order.write().await.retain(|x| x != id);
        if let Some(job) = job {
            job.cancel.store(true, Ordering::Relaxed);
            let _ = std::fs::remove_file(self.file_path(&job));
            // HLS intermediate .ts
            let ts = self.dir.join(format!("{}.ts", job.id));
            let _ = std::fs::remove_file(ts);
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------- workers

fn build_req_headers(headers: &[(String, String)]) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(v),
        ) {
            map.insert(name, val);
        }
    }
    map
}

/// Sliding-window speedometer shared by file + HLS workers.
struct Speedo {
    samples: VecDeque<(Instant, u64)>,
    last_emit: Instant,
}

impl Speedo {
    fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            last_emit: Instant::now(),
        }
    }

    fn push(&mut self, total_bytes: u64) -> Option<(f64, bool)> {
        let now = Instant::now();
        self.samples.push_back((now, total_bytes));
        while self.samples.len() > 2
            && now.duration_since(self.samples[0].0) > Duration::from_secs(3)
        {
            self.samples.pop_front();
        }
        if now.duration_since(self.last_emit) >= Duration::from_millis(300) {
            self.last_emit = now;
            let (t0, b0) = self.samples[0];
            let dt = now.duration_since(t0).as_secs_f64();
            let bps = if dt > 0.05 {
                (total_bytes.saturating_sub(b0)) as f64 / dt
            } else {
                0.0
            };
            Some((bps, true))
        } else {
            None
        }
    }
}

async fn run_job(store: DlStore, http: reqwest::Client, id: String) {
    let Some(job) = store.get(&id).await else {
        return;
    };
    store
        .mutate(&id, |j| {
            j.status = "downloading".to_string();
        })
        .await;
    let result = match job.kind.as_str() {
        "hls" => run_hls(&store, &http, &id).await,
        "dash" => run_dash(&store, &http, &id).await,
        _ => run_direct(&store, &http, &id).await,
    };
    match result {
        Ok(()) => {
            // Worker sets completed itself (filename may have changed, e.g. .ts fallback).
            store
                .mutate(&id, |j| {
                    if !j.is_terminal() {
                        j.status = "completed".to_string();
                        j.progress = 100.0;
                    }
                })
                .await;
        }
        Err(e) if e == "cancelled" => {
            store
                .mutate(&id, |j| {
                    j.status = "cancelled".to_string();
                    j.error = None;
                })
                .await;
        }
        Err(e) => {
            // Drop partial HLS segments on failure so failed jobs don't leak
            // hundreds of MB (cancel/retry paths already clean their own).
            if let Some(job) = store.get(&id).await {
                if job.kind == "hls" {
                    let _ = std::fs::remove_file(store.dir().join(format!("{id}.ts")));
                }
            }
            store
                .mutate(&id, |j| {
                    j.status = "failed".to_string();
                    j.error = Some(e);
                })
                .await;
        }
    }
}

// ---------------- direct files (mp4/mkv/webm/ts) — mirrors crate::download single-shot

async fn run_direct(store: &DlStore, http: &reqwest::Client, id: &str) -> Result<(), String> {
    let Some(job) = store.get(id).await else {
        return Err("job gone".to_string());
    };
    let req_headers = build_req_headers(&job.headers);
    let res = http
        .get(job.url.clone())
        .headers(req_headers)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    // Post-redirect SSRF + playlist sniffing: a "direct" URL may resolve to HLS/DASH.
    let final_url = res.url().clone();
    if let Some(host) = final_url.host_str() {
        if super::is_blocked_host(host) {
            return Err("blocked destination after redirect".to_string());
        }
    }
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ct.contains("mpegurl") || ct.contains("dash+xml") {
        // Hand over to the right engine inside the same job.
        store
            .mutate(id, |j| {
                j.kind = if ct.contains("mpegurl") {
                    "hls".to_string()
                } else {
                    "dash".to_string()
                };
            })
            .await;
        drop(res);
        let kind = store.get(id).await.map(|j| j.kind).unwrap_or_default();
        if kind == "hls" {
            return run_hls(store, http, id).await;
        } else {
            return run_dash(store, http, id).await;
        }
    }
    let status = res.status();
    if !(status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT) {
        return Err(format!("server replied {status}"));
    }
    let total = res.content_length();
    // Refresh extension from real content-type when we guessed wrong.
    if total.is_some() || !ct.is_empty() {
        let ext = infer_download_extension(final_url.as_str(), Some(&ct));
        store
            .mutate(id, |j| {
                if let Some(dot) = j.filename.rfind('.') {
                    j.filename.replace_range(dot + 1.., ext);
                }
                j.total = total;
            })
            .await;
    } else {
        store
            .mutate(id, |j| {
                j.total = total;
            })
            .await;
    }

    let dest = store.file_path(&store.get(id).await.unwrap());
    let part = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| format!("cannot write file: {e}"))?;
    let mut downloaded: u64 = 0;
    let mut speedo = Speedo::new();
    use tokio::io::AsyncWriteExt;
    let mut stream = res.bytes_stream();
    use futures::StreamExt;
    // 60s idle watchdog like the sidecar proxy.
    loop {
        if store
            .get(id)
            .await
            .map(|j| j.cancel.load(Ordering::Relaxed))
            .unwrap_or(true)
        {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err("cancelled".to_string());
        }
        let chunk = tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .map_err(|_| "download stalled (60s without data)".to_string())?;
        let Some(chunk) = chunk else { break };
        let bytes = chunk.map_err(|e| format!("network error: {e}"))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("write failed: {e}"))?;
        downloaded += bytes.len() as u64;
        if let Some((bps, _)) = speedo.push(downloaded) {
            let eta = total.filter(|t| *t > downloaded).map(|t| {
                if bps > 1.0 {
                    ((t - downloaded) as f64 / bps) as u64
                } else {
                    0
                }
            });
            let pct = total.map(|t| {
                if t > 0 {
                    downloaded as f64 / t as f64 * 100.0
                } else {
                    0.0
                }
            });
            store
                .mutate(id, |j| {
                    j.downloaded = downloaded;
                    j.speed_bps = bps;
                    j.eta_secs = eta;
                    if let Some(p) = pct {
                        j.progress = p.min(100.0);
                    }
                })
                .await;
        }
    }
    file.flush()
        .await
        .map_err(|e| format!("flush failed: {e}"))?;
    drop(file);
    if let Some(t) = total {
        if downloaded != t {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(format!("incomplete download ({downloaded}/{t} bytes)"));
        }
    }
    tokio::fs::rename(&part, &dest)
        .await
        .map_err(|e| format!("finalize failed: {e}"))?;
    store
        .mutate(id, |j| {
            j.downloaded = downloaded;
            j.speed_bps = 0.0;
            j.eta_secs = Some(0);
            j.progress = 100.0;
            j.status = "completed".to_string();
        })
        .await;
    Ok(())
}

// ---------------- HLS: fetch playlist (auth headers), segments -> .ts -> ffmpeg mp4

fn pick_hls_variant(master: &str, quality: Option<u16>) -> Option<String> {
    // Master playlist: pick the URI after the EXT-X-STREAM-INF.
    // If quality is Some, find the variant with resolution height closest to it.
    // Otherwise, pick the highest bandwidth.
    let mut best_bw: u64 = 0;
    let mut best_height_diff: Option<u16> = None;
    let mut best_uri: Option<String> = None;
    let mut pending_bw: Option<u64> = None;
    let mut pending_height: Option<u16> = None;
    for line in master.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("#EXT-X-STREAM-INF:") {
            pending_bw = rest.split(',').find_map(|kv| {
                let kv = kv.trim();
                kv.strip_prefix("BANDWIDTH=")
                    .and_then(|v| v.parse::<u64>().ok())
            });
            pending_height = rest.split(',').find_map(|kv| {
                let kv = kv.trim();
                kv.strip_prefix("RESOLUTION=")
                    .and_then(|v| v.split('x').nth(1))
                    .and_then(|v| v.parse::<u16>().ok())
            });
        } else if !t.is_empty() && !t.starts_with('#') {
            if let Some(bw) = pending_bw.take() {
                let height = pending_height.take();
                let is_better = if let Some(q) = quality {
                    if let Some(h) = height {
                        let diff = h.abs_diff(q);
                        if let Some(best_diff) = best_height_diff {
                            if diff < best_diff {
                                true
                            } else {
                                diff == best_diff && bw > best_bw
                            }
                        } else {
                            true
                        }
                    } else {
                        best_uri.is_none()
                    }
                } else {
                    best_uri.is_none() || bw >= best_bw
                };

                if is_better {
                    best_bw = bw;
                    if let (Some(q), Some(h)) = (quality, height) {
                        best_height_diff = Some(h.abs_diff(q));
                    }
                    best_uri = Some(t.to_string());
                }
            }
        }
    }
    best_uri
}

async fn fetch_text(
    http: &reqwest::Client,
    headers: &[(String, String)],
    url: &url::Url,
) -> Result<String, String> {
    let res = http
        .get(url.clone())
        .headers(build_req_headers(headers))
        .send()
        .await
        .map_err(|e| format!("playlist request failed: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("playlist server replied {}", res.status()));
    }
    if let Some(len) = res.content_length() {
        if len > MAX_MANIFEST_BYTES as u64 {
            return Err("playlist too large".to_string());
        }
    }
    let bytes = res
        .bytes()
        .await
        .map_err(|e| format!("playlist read failed: {e}"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("playlist too large".to_string());
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| "playlist is not valid text".to_string())
}

async fn run_hls(store: &DlStore, http: &reqwest::Client, id: &str) -> Result<(), String> {
    let Some(job) = store.get(id).await else {
        return Err("job gone".to_string());
    };
    let base = url::Url::parse(&job.url).map_err(|_| "invalid url".to_string())?;
    if super::is_blocked_host(base.host_str().unwrap_or("")) {
        return Err("blocked destination".to_string());
    }
    let mut text = fetch_text(http, &job.headers, &base).await?;
    // Master playlist? descend into the best variant (same auth headers).
    if text.contains("#EXT-X-STREAM-INF") {
        let rel = pick_hls_variant(&text, job.quality)
            .ok_or("cannot find variant playlist".to_string())?;
        let variant_url = base.join(&rel).map_err(|_| "bad variant url".to_string())?;
        if super::is_blocked_host(variant_url.host_str().unwrap_or("")) {
            return Err("blocked destination after redirect".to_string());
        }
        text = fetch_text(http, &job.headers, &variant_url).await?;
        store
            .mutate(id, |j| {
                j.progress = 2.0;
            })
            .await;
        let base2 = variant_url;
        return run_hls_media(store, http, id, &text, &base2).await;
    }
    run_hls_media(store, http, id, &text, &base).await
}

/// Fetch one HLS segment into the open output file. Returns bytes written.
/// Single attempt — callers retry with backoff (see SEG_ATTEMPTS).
async fn fetch_one_segment(
    http: &reqwest::Client,
    headers: &[(String, String)],
    seg_url: &url::Url,
    _index: usize,
    out: &mut tokio::fs::File,
) -> Result<u64, String> {
    use tokio::io::AsyncWriteExt;
    let res = http
        .get(seg_url.clone())
        .headers(build_req_headers(headers))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("server replied {}", res.status()));
    }
    let mut stream = res.bytes_stream();
    use futures::StreamExt;
    let mut wrote: u64 = 0;
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .map_err(|_| "stalled (60s without data)".to_string())?;
        let Some(chunk) = chunk else { break };
        let bytes = chunk.map_err(|e| format!("body error: {e}"))?;
        out.write_all(&bytes)
            .await
            .map_err(|e| format!("write failed: {e}"))?;
        wrote += bytes.len() as u64;
    }
    Ok(wrote)
}

async fn run_hls_media(
    store: &DlStore,
    http: &reqwest::Client,
    id: &str,
    playlist: &str,
    base: &url::Url,
) -> Result<(), String> {
    // Encrypted streams cannot be reassembled from raw segments.
    for line in playlist.lines() {
        let t = line.trim();
        if t.starts_with("#EXT-X-KEY") && !t.contains("METHOD=NONE") {
            return Err(
                "this HLS stream is encrypted — open it with Play in browser or VLC instead"
                    .to_string(),
            );
        }
    }
    let mut segs: Vec<url::Url> = vec![];
    for line in playlist.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        // Skip subtitles/rendition playlists referenced via URI="..." (handled by tags above).
        if let Ok(abs) = base.join(t) {
            if super::is_blocked_host(abs.host_str().unwrap_or("")) {
                return Err("blocked segment host".to_string());
            }
            segs.push(abs);
        }
    }
    if segs.is_empty() {
        return Err("playlist has no segments".to_string());
    }
    let total_segs = segs.len();
    // Per-segment retries mirror crate::download::MAX_ATTEMPTS semantics:
    // transient CDN blips must not kill a 500MB download.
    const SEG_ATTEMPTS: u32 = 4;
    let ts_path = store.dir().join(format!("{id}.ts"));
    let mut out = tokio::fs::File::create(&ts_path)
        .await
        .map_err(|e| format!("cannot write file: {e}"))?;
    use tokio::io::AsyncWriteExt;
    let mut downloaded: u64 = 0;
    let mut speedo = Speedo::new();
    let headers_snapshot = store.get(id).await.map(|j| j.headers).unwrap_or_default();
    for (i, seg_url) in segs.iter().enumerate() {
        if store
            .get(id)
            .await
            .map(|j| j.cancel.load(Ordering::Relaxed))
            .unwrap_or(true)
        {
            drop(out);
            let _ = tokio::fs::remove_file(&ts_path).await;
            return Err("cancelled".to_string());
        }
        let mut seg_bytes: u64 = 0;
        let mut last_err = String::new();
        let mut ok = false;
        for attempt in 0..SEG_ATTEMPTS {
            if attempt > 0 {
                // Exponential backoff before retrying the same segment.
                tokio::time::sleep(Duration::from_secs(1 << (attempt - 1).min(3))).await;
            }
            if store
                .get(id)
                .await
                .map(|j| j.cancel.load(Ordering::Relaxed))
                .unwrap_or(true)
            {
                drop(out);
                let _ = tokio::fs::remove_file(&ts_path).await;
                return Err("cancelled".to_string());
            }
            match fetch_one_segment(http, &headers_snapshot, seg_url, i, &mut out).await {
                Ok(n) => {
                    seg_bytes = n;
                    ok = true;
                    break;
                }
                Err(e) => {
                    last_err = e;
                }
            }
        }
        if !ok {
            drop(out);
            return Err(format!(
                "segment {} failed after {SEG_ATTEMPTS} tries ({last_err}) — retry the download to resume",
                i + 1
            ));
        }
        {
            downloaded += seg_bytes;
            if let Some((bps, _)) = speedo.push(downloaded) {
                let done_frac = (i as f64 + 0.5) / total_segs as f64;
                // Reserve last 10% for remux.
                let pct = (done_frac * 90.0).min(90.0);
                store
                    .mutate(id, |j| {
                        j.downloaded = downloaded;
                        j.speed_bps = bps;
                        j.progress = pct;
                        // ETA from byte-rate once we have a meaningful sample.
                        if bps > 1.0 && done_frac > 0.02 {
                            let est_total = downloaded as f64 / done_frac;
                            j.eta_secs =
                                Some(((est_total - downloaded as f64) / bps).max(0.0) as u64);
                        } else {
                            j.eta_secs = None;
                        }
                    })
                    .await;
            }
        }
        // Per-segment progress bump even when speedo throttles.
        let pct = ((i + 1) as f64 / total_segs as f64 * 90.0).min(90.0);
        store
            .mutate(id, |j| {
                j.downloaded = downloaded;
                j.progress = pct;
            })
            .await;
    }
    out.flush()
        .await
        .map_err(|e| format!("flush failed: {e}"))?;
    drop(out);

    // Remux .ts -> .mp4 (stream copy, fast). Fallback: keep .ts.
    store
        .mutate(id, |j| {
            j.status = "merging".to_string();
            j.progress = 92.0;
        })
        .await;
    let mp4_name = {
        let j = store.get(id).await.unwrap();
        j.filename
    };
    let mp4_path = store.dir().join(&mp4_name);
    match remux_ts_to_mp4(&ts_path, &mp4_path).await {
        Ok(()) => {
            let _ = tokio::fs::remove_file(&ts_path).await;
            let size = tokio::fs::metadata(&mp4_path)
                .await
                .map(|m| m.len())
                .unwrap_or(downloaded);
            store
                .mutate(id, |j| {
                    j.downloaded = size;
                    j.total = Some(size);
                    j.speed_bps = 0.0;
                    j.eta_secs = Some(0);
                    j.progress = 100.0;
                    j.status = "completed".to_string();
                })
                .await;
        }
        Err(e) => {
            // ffmpeg missing/failed: deliver the .ts itself.
            let ts_name = {
                let j = store.get(id).await.unwrap();
                let stem = j
                    .filename
                    .rsplit_once('.')
                    .map(|(s, _)| s)
                    .unwrap_or(&j.filename);
                format!("{stem}.ts")
            };
            let ts_final = store.dir().join(&ts_name);
            let _ = tokio::fs::rename(&ts_path, &ts_final).await;
            let size = tokio::fs::metadata(&ts_final)
                .await
                .map(|m| m.len())
                .unwrap_or(downloaded);
            store
                .mutate(id, |j| {
                    j.filename = ts_name.clone();
                    j.downloaded = size;
                    j.total = Some(size);
                    j.progress = 100.0;
                    j.status = "completed".to_string();
                    j.error = Some(format!("saved as .ts ({e})"));
                })
                .await;
        }
    }
    Ok(())
}

fn find_on_path(name: &str) -> Option<String> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let cand = dir.join(&exe);
            if cand.is_file() {
                return cand.to_string_lossy().into_owned().into();
            }
            #[cfg(windows)]
            {
                // PATHEXT-less check: `ffmpeg` shim without extension, plus .cmd/.bat.
                let bare = dir.join(name);
                if bare.is_file() {
                    return bare.to_string_lossy().into_owned().into();
                }
            }
        }
    }
    None
}

async fn remux_ts_to_mp4(ts: &std::path::Path, mp4: &std::path::Path) -> Result<(), String> {
    let ffmpeg = find_on_path("ffmpeg")
        .ok_or("ffmpeg not found (install with: winget install Gyan.FFmpeg)".to_string())?;
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.arg("-y")
        .arg("-i")
        .arg(ts)
        .arg("-c")
        .arg("copy")
        .arg("-bsf:a")
        .arg("aac_adtstoasc")
        .arg(mp4);
    #[cfg(windows)]
    {
        // tokio::process::Command has an inherent Windows creation_flags.
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let status = tokio::time::timeout(Duration::from_secs(30 * 60), cmd.status())
        .await
        .map_err(|_| "ffmpeg timed out".to_string())?
        .map_err(|e| format!("ffmpeg failed to run: {e}"))?;
    if status.success() && mp4.is_file() {
        Ok(())
    } else {
        Err(format!(
            "ffmpeg exited with {status} (is ffmpeg installed?)"
        ))
    }
}

// ---------------- DASH via yt-dlp — mirrors tui/app/download.rs

fn parse_speed_bps(token: &str) -> Option<f64> {
    let t = token.trim().trim_end_matches("/s");
    let (num, mult) = if let Some(v) = t.strip_suffix("GiB") {
        (v, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(v) = t.strip_suffix("MiB") {
        (v, 1024.0 * 1024.0)
    } else if let Some(v) = t.strip_suffix("KiB") {
        (v, 1024.0)
    } else if let Some(v) = t.strip_suffix('G') {
        (v, 1_000_000_000.0)
    } else if let Some(v) = t.strip_suffix('M') {
        (v, 1_000_000.0)
    } else if let Some(v) = t.strip_suffix('K') {
        (v, 1_000.0)
    } else if let Some(v) = t.strip_suffix('B') {
        (v, 1.0)
    } else {
        (t, 1.0)
    };
    num.trim().parse::<f64>().ok().map(|n| n * mult)
}

fn parse_eta_secs(token: &str) -> Option<u64> {
    let clean = token.trim().trim_matches(['(', ')', ',']);
    if clean.eq_ignore_ascii_case("unknown") || clean.is_empty() {
        return None;
    }
    let parts: Vec<&str> = clean.split(':').collect();
    let mut total: u64 = 0;
    for p in parts {
        total = total * 60 + p.parse::<u64>().ok()?;
    }
    Some(total)
}

/// Parse a yt-dlp `--newline` progress line.
/// Returns (percent, speed_bps, eta_secs, eta_text).
fn parse_ytdlp_line(line: &str) -> Option<(f64, Option<f64>, Option<u64>, String)> {
    if !line.contains("[download]") || line.contains("Destination:") {
        return None;
    }
    let trimmed = line.split("[download]").nth(1)?.trim();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let pct: f64 = words
        .iter()
        .find(|w| w.ends_with('%'))?
        .trim_end_matches('%')
        .parse()
        .ok()?;
    let mut speed = None;
    let mut eta = None;
    let mut eta_text = String::new();
    if let Some(at_idx) = words.iter().position(|&w| w == "at") {
        if let Some(&s) = words.get(at_idx + 1) {
            speed = parse_speed_bps(s);
        }
    }
    if let Some(eta_idx) = words.iter().position(|&w| w == "ETA") {
        if let Some(&e) = words.get(eta_idx + 1) {
            eta = parse_eta_secs(e);
            let c = e.trim_matches(['(', ')', ',']);
            if !c.eq_ignore_ascii_case("unknown") {
                eta_text = c.to_string();
            }
        }
    }
    Some((pct, speed, eta, eta_text))
}

async fn run_dash(store: &DlStore, _http: &reqwest::Client, id: &str) -> Result<(), String> {
    let Some(job) = store.get(id).await else {
        return Err("job gone".to_string());
    };
    let ytdlp = find_on_path("yt-dlp").ok_or_else(|| {
        if cfg!(target_os = "windows") {
            "DASH download needs yt-dlp. Install it with: winget install yt-dlp.yt-dlp Gyan.FFmpeg"
                .to_string()
        } else {
            "DASH download needs yt-dlp + ffmpeg installed on the server.".to_string()
        }
    })?;
    let dest = store.file_path(&job);
    // yt-dlp honors the output extension for the merged file.
    let mut cmd = tokio::process::Command::new(ytdlp);
    for (k, v) in &job.headers {
        if k.eq_ignore_ascii_case("user-agent") {
            cmd.arg("--user-agent").arg(v);
        } else {
            cmd.arg("--add-header").arg(format!("{k}: {v}"));
        }
    }
    let format_arg = if let Some(q) = job.quality {
        format!("bestvideo[height<={}]+bestaudio/best", q)
    } else {
        "bestvideo+bestaudio/best".to_string()
    };
    cmd.arg("-f")
        .arg(&format_arg)
        .arg("--newline")
        .arg("--merge-output-format")
        .arg("mp4")
        .arg("--part")
        .arg("-o")
        .arg(&dest)
        .arg("--force-overwrites")
        .arg(&job.url);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.kill_on_drop(true);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start yt-dlp: {e}"))?;
    let mut stream_index: usize = 0;
    let mut max_progress: f64 = 0.0;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    if let Some(stdout) = child.stdout.take() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut lines = BufReader::new(stdout).lines();
        loop {
            if store
                .get(id)
                .await
                .map(|j| j.cancel.load(Ordering::Relaxed))
                .unwrap_or(true)
            {
                let _ = child.kill().await;
                cleanup_ytdlp_part(&dest).await;
                return Err("cancelled".to_string());
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(250)) => {
                    if store.get(id).await.map(|j| j.cancel.load(Ordering::Relaxed)).unwrap_or(true) {
                        let _ = child.kill().await;
                        cleanup_ytdlp_part(&dest).await;
                        return Err("cancelled".to_string());
                    }
                    // Poll file size so bytes climb even between % lines.
                    poll_dest_size(store, id, &dest).await;
                }
                line_res = lines.next_line() => {
                    match line_res {
                        Ok(Some(line)) => {
                            if line.contains("[download] Destination:") {
                                stream_index = stream_index.saturating_add(1);
                            } else if let Some((raw_pct, speed, eta, _eta_text)) = parse_ytdlp_line(&line) {
                                // Same normalization as the TUI: video 0-90, audio 90-98.
                                let current = stream_index.max(1);
                                let norm = if current <= 1 { raw_pct * 0.90 } else { 90.0 + raw_pct * 0.08 };
                                max_progress = max_progress.max(norm);
                                if last_emit.elapsed() >= Duration::from_millis(300) || raw_pct >= 99.9 {
                                    poll_dest_size(store, id, &dest).await;
                                    store.mutate(id, |j| {
                                        j.progress = max_progress.min(99.0);
                                        if j.status == "downloading" { /* keep */ }
                                        if let Some(bps) = speed { j.speed_bps = bps; }
                                        if eta.is_some() { j.eta_secs = eta; }
                                    }).await;
                                    last_emit = Instant::now();
                                }
                            } else if line.contains("[Merger]") || line.contains("[ffmpeg]") {
                                max_progress = max_progress.max(99.0);
                                store.mutate(id, |j| {
                                    j.status = "merging".to_string();
                                    j.progress = 99.0;
                                }).await;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|e| format!("yt-dlp wait failed: {e}"))?;
    if !status.success() {
        if store
            .get(id)
            .await
            .map(|j| j.cancel.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            cleanup_ytdlp_part(&dest).await;
            return Err("cancelled".to_string());
        }
        cleanup_ytdlp_part(&dest).await;
        return Err(format!("yt-dlp exited with status {status}"));
    }
    // Resolve the real output (yt-dlp may adjust the extension).
    let final_path = if dest.is_file() {
        dest.clone()
    } else {
        find_sibling_output(store.dir(), id, &job.filename)
            .await
            .ok_or("yt-dlp finished but the file is missing".to_string())?
    };
    let size = tokio::fs::metadata(&final_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let final_name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(job.filename.clone());
    cleanup_ytdlp_part(&dest).await;
    store
        .mutate(id, |j| {
            j.filename = final_name;
            j.downloaded = size;
            j.total = Some(size);
            j.speed_bps = 0.0;
            j.eta_secs = Some(0);
            j.progress = 100.0;
            j.status = "completed".to_string();
        })
        .await;
    Ok(())
}

async fn poll_dest_size(store: &DlStore, id: &str, dest: &std::path::Path) {
    // yt-dlp writes `<dest>.part` while working; count either.
    let mut best = 0u64;
    for cand in [dest.to_path_buf(), dest.with_extension("part"), {
        let mut p = dest.as_os_str().to_owned();
        p.push(".part");
        PathBuf::from(p)
    }] {
        if let Ok(m) = tokio::fs::metadata(&cand).await {
            best = best.max(m.len());
        }
    }
    if best > 0 {
        store
            .mutate(id, |j| {
                if best > j.downloaded {
                    j.downloaded = best;
                }
            })
            .await;
    }
}

async fn cleanup_ytdlp_part(dest: &std::path::Path) {
    let _ = tokio::fs::remove_file(dest.with_extension("part")).await;
    let mut p = dest.as_os_str().to_owned();
    p.push(".part");
    let _ = tokio::fs::remove_file(PathBuf::from(p)).await;
}

async fn find_sibling_output(dir: &PathBuf, _id: &str, wanted: &str) -> Option<PathBuf> {
    let stem = wanted.rsplit_once('.').map(|(s, _)| s).unwrap_or(wanted);
    let mut rd = tokio::fs::read_dir(dir).await.ok()?;
    let mut best: Option<(SystemTime, PathBuf)> = None;
    use tokio::fs as tfs;
    while let Ok(Some(ent)) = rd.next_entry().await {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with(stem) && name != wanted {
            if let Ok(m) = tfs::metadata(ent.path()).await {
                if let Ok(mt) = m.modified() {
                    if best.as_ref().map(|(t, _)| *t < mt).unwrap_or(true) {
                        best = Some((mt, ent.path()));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ytdlp_line_parses_pct_speed_eta() {
        let (pct, speed, eta, _) =
            parse_ytdlp_line("[download]  45.2% of 1.20GiB at  3.50MiB/s ETA 04:12").unwrap();
        assert!((pct - 45.2).abs() < 0.01);
        assert!(speed.unwrap() > 3_000_000.0);
        assert_eq!(eta, Some(252));
    }

    #[test]
    fn ytdlp_line_ignores_destination_and_done_lines() {
        assert!(parse_ytdlp_line("[download] Destination: foo.mp4").is_none());
        assert!(parse_ytdlp_line("[download] 100% of 10MiB in 00:02").is_some());
    }

    #[test]
    fn speed_tokens_parse() {
        assert_eq!(parse_speed_bps("850KiB/s").unwrap() as u64, 850 * 1024);
        assert!((parse_speed_bps("1.20GiB/s").unwrap() - 1.2 * 1024.0f64.powi(3)).abs() < 1.0);
        assert!(parse_speed_bps("nonsense").is_none());
    }

    #[test]
    fn eta_tokens_parse() {
        assert_eq!(parse_eta_secs("04:12"), Some(252));
        assert_eq!(parse_eta_secs("01:02:03"), Some(3723));
        assert_eq!(parse_eta_secs("Unknown"), None);
    }

    #[test]
    fn master_playlist_picks_highest_bandwidth() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhi.m3u8\n";
        assert_eq!(pick_hls_variant(master).as_deref(), Some("hi.m3u8"));
    }

    #[test]
    fn master_playlist_none_without_variants() {
        let media = "#EXTM3U\n#EXT-X-TARGETDURATION:10\nseg1.ts\nseg2.ts\n";
        assert!(pick_hls_variant(media).is_none());
    }
}
