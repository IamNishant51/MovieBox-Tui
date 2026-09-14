use axum::{
    extract::{Query, State, Json},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
    middleware,
    http::{HeaderMap, HeaderValue, header},
};
use reqwest::Client;
use moviebox_tui::providers::models::{CatalogItem, MediaType, ProviderKind};
use moviebox_tui::providers::ReleaseProvider;
use moviebox_tui::service::MovieBoxService;
use moviebox_tui::player::command;
use moviebox_tui::history::{HistoryManager, WatchHistoryItem};
use moviebox_tui::favorites::{FavoritesManager, FavoriteItem};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use axum::http::StatusCode;
use axum::body::Body;

mod dl_jobs;

// ---------------------------------------------------------------- App state

#[derive(Clone)]
struct AuthConfig {
    google_client_id: String,
    google_client_secret: String,
    google_redirect_uri: String,
    session_secret: String,
}

impl AuthConfig {
    fn from_env() -> Self {
        Self {
            google_client_id: std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default(),
            google_client_secret: std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default(),
            google_redirect_uri: std::env::var("GOOGLE_REDIRECT_URI")
                .unwrap_or_else(|_| "http://127.0.0.1:3000/api/auth/callback".to_string()),
            session_secret: std::env::var("SESSION_SECRET").unwrap_or_default(),
        }
    }
    fn configured(&self) -> bool {
        !self.google_client_id.is_empty() && !self.google_client_secret.is_empty()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    sub: String,
    email: String,
    name: String,
    picture: String,
    exp: u64,
}

#[derive(Clone)]
struct AppState {
    service: Arc<MovieBoxService>,
    http_client: Client,
    suggest_cache: Arc<std::sync::Mutex<HashMap<String, (Instant, serde_json::Value)>>>,
    auth: AuthConfig,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    dl: dl_jobs::DlStore,
}

// ---------------------------------------------------------------- Query types

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    page: Option<usize>,
    provider: Option<String>,
}

#[derive(Deserialize)]
struct DetailsQuery {
    id: String,
    provider: String,
}

#[derive(Deserialize)]
struct StreamsQuery {
    id: String,
    provider: String,
    season: usize,
    episode: usize,
}

#[derive(Deserialize)]
struct PlayQuery {
    url: String,
    headers: Option<String>,
    subtitle_url: Option<String>,
}

#[derive(Deserialize)]
struct DownloadQuery {
    url: String,
    headers: Option<String>,
    filename: Option<String>,
}

#[derive(Deserialize)]
struct SuggestQuery {
    q: String,
}

#[derive(Deserialize)]
struct RecsQuery {
    limit: Option<usize>,
}

// ---------------------------------------------------------------- main

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--proxy-for-vlc") {
        let target_url = args.get(pos + 1).cloned().unwrap_or_default();
        let headers_json = args
            .get(pos + 2)
            .cloned()
            .unwrap_or_else(|| "[]".to_string());
        let sub_url = args.get(pos + 3).cloned().filter(|s| !s.is_empty());
        let headers: Vec<(String, String)> =
            serde_json::from_str(&headers_json).unwrap_or_default();
        moviebox_tui::proxy::run_sidecar(target_url, headers, sub_url).await;
        return;
    }

    let service = MovieBoxService::new();
    let state = AppState {
        service: Arc::new(service),
        http_client: Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap(),
        suggest_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        auth: AuthConfig::from_env(),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        dl: dl_jobs::DlStore::new(),
    };

    if !state.auth.configured() {
        println!("[auth] Google OAuth not configured (set GOOGLE_CLIENT_ID/SECRET in .env). Auth routes will return 503.");
    }

    let app = Router::new()
        .route("/api/config", get(config_handler))
        .route("/", get(index_handler))
        .route("/api/search", get(search_handler))
        .route("/api/suggest", get(suggest_handler))
        .route("/api/details", get(details_handler))
        .route("/api/streams", get(streams_handler))
        .route("/api/subtitles", get(subtitles_handler))
        .route("/api/play", get(play_handler))
        .route("/api/proxy", get(proxy_handler))
        .route("/api/download", get(download_handler))
        .route("/api/downloads", get(list_downloads_handler).post(start_download_handler))
        .route("/api/downloads/:id", get(get_download_handler).delete(delete_download_handler))
        .route("/api/downloads/:id/retry", post(retry_download_handler))
        .route("/api/downloads/:id/file", get(download_file_handler))
        .route("/api/homepage", get(homepage_handler))
        .route("/api/recommendations", get(recommendations_handler))
        .route("/api/history", get(get_history_handler).post(post_history_handler))
        .route("/api/favorites", get(get_favorites_handler).post(add_favorite_handler).delete(remove_favorite_handler))
        .route("/api/auth/login", get(auth_login_handler))
        .route("/api/auth/callback", get(auth_callback_handler))
        .route("/api/auth/me", get(auth_me_handler))
        .route("/api/auth/logout", post(auth_logout_handler))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(1024 * 1024))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(20)))
        .layer(tower_http::compression::CompressionLayer::new().gzip(true).br(true).zstd(true))
        .layer(middleware::from_fn(security_headers_mw))
        .with_state(state);

    println!("[mode] APP_MODE={} (login_required={})", app_mode(), login_required());
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let listener = TcpListener::bind(format!("{host}:{port}")).await.unwrap();
    println!("Web server running at http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}

async fn security_headers_mw(
    req: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(
        "Permissions-Policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    res
}

// ---------------------------------------------------------------- index

fn index_path() -> std::path::PathBuf {
    let candidates = [
        std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("index.html"))),
        std::env::current_dir().ok().map(|c| c.join("src/bin/index.html")),
        std::env::current_dir().ok().map(|c| c.join("index.html")),
        option_env!("CARGO_MANIFEST_DIR").map(|m| std::path::PathBuf::from(m).join("src/bin/index.html")),
    ];
    for cand in candidates.into_iter().flatten() {
        if cand.is_file() {
            return cand;
        }
    }
    std::path::PathBuf::from("src/bin/index.html")
}

async fn index_handler() -> impl IntoResponse {
    // Fall back to compile-time embed so `cargo install` binaries still serve UI.
    match std::fs::read(index_path()) {
        Ok(b) => Html(b).into_response(),
        Err(_) => {
            const EMBEDDED: &str = include_str!("index.html");
            if !EMBEDDED.trim().is_empty() {
                return Html(EMBEDDED.as_bytes().to_vec()).into_response();
            }
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load index.html").into_response()
        }
    }
}

// ---------------------------------------------------------------- SSRF guard

fn is_blocked_host(host: &str) -> bool {
    let h = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    if h == "localhost"
        || h == "metadata.google.internal"
        || h == "metadata.google"
        || h == "169.254.169.254"
        || h == "::1"
        || h == "::ffff:127.0.0.1"
        || h == "::"
    {
        return true;
    }
    if h == "127.0.0.1" || h == "0.0.0.0" || h.starts_with("127.") || h.starts_with("0.") {
        return true;
    }
    if h.starts_with("10.") || h.starts_with("192.168.") {
        return true;
    }
    if let Some(rest) = h.strip_prefix("172.") {
        if let Some(octet) = rest.split('.').next().and_then(|s| s.parse::<u8>().ok()) {
            if (16..=31).contains(&octet) {
                return true;
            }
        }
    }
    if h.starts_with("fe80:") || h.starts_with("fec0:") || h.starts_with("fc") || h.starts_with("fd") {
        return true;
    }
    if h.ends_with(".internal") || h.ends_with(".local") || h.ends_with(".lan") || !h.contains('.') {
        return true;
    }
    false
}

fn check_ssrf_url(raw: &str) -> Result<url::Url, (StatusCode, String)> {
    let u = url::Url::parse(raw).map_err(|_| (StatusCode::BAD_REQUEST, "invalid url".to_string()))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err((StatusCode::BAD_REQUEST, "only http/https allowed".to_string()));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err((StatusCode::BAD_REQUEST, "userinfo not allowed".to_string()));
    }
    match u.host_str() {
        Some(host) if !is_blocked_host(host) => Ok(u),
        _ => Err((StatusCode::FORBIDDEN, "blocked destination".to_string())),
    }
}

// ---------------------------------------------------------------- homepage / search

#[derive(Deserialize)]
struct HomepageQuery {
    page: Option<usize>,
    provider: Option<String>,
}

async fn homepage_handler(
    State(state): State<AppState>,
    Query(query): Query<HomepageQuery>,
) -> impl IntoResponse {
    // Open catalog: browsing is public, watching (streams/play/proxy/download) requires login.
    let page = query.page.unwrap_or(1);
    let provider = query.provider.unwrap_or_else(|| "moviebox".to_string());
    let provider_kind = ProviderKind::parse(&provider).unwrap_or(ProviderKind::MovieBox);

    if provider_kind == ProviderKind::MovieBox {
        match state.service.homepage("", page).await {
            Ok((items, _)) => Json(items).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        match tokio::time::timeout(
            Duration::from_secs(8),
            state.service.search_typed(provider_kind, "", page),
        )
        .await
        {
            Ok(Ok(items)) => Json(items).into_response(),
            Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            Err(_) => (StatusCode::GATEWAY_TIMEOUT, "provider timed out").into_response(),
        }
    }
}

async fn search_handler(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    // Open catalog: browsing is public.
    let page = query.page.unwrap_or(1);

    let providers = if let Some(p) = query.provider {
        vec![ProviderKind::parse(&p).unwrap_or(ProviderKind::MovieBox)]
    } else {
        vec![
            ProviderKind::MovieBox,
            ProviderKind::FourKHdHub,
            ProviderKind::BdixCircleFtp,
            ProviderKind::BdixDhakaFlix,
        ]
    };

    // Per-provider 8s timeout so one slow BDIX host can't block the whole page.
    let futs = providers.into_iter().map(|provider| {
        let (service, q) = (state.service.clone(), query.q.clone());
        async move {
            match tokio::time::timeout(Duration::from_secs(8), service.search_typed(provider, &q, page)).await {
                Ok(Ok(items)) => items,
                Ok(Err(e)) => {
                    eprintln!("[search] {provider:?}: {e}");
                    vec![]
                }
                Err(_) => {
                    eprintln!("[search] {provider:?}: timeout");
                    vec![]
                }
            }
        }
    });
    let all_results: Vec<CatalogItem> =
        futures::future::join_all(futs).await.into_iter().flatten().collect();
    Json(all_results).into_response()
}

async fn suggest_handler(
    State(state): State<AppState>,
    Query(query): Query<SuggestQuery>,
) -> impl IntoResponse {
    // Open catalog: browsing is public.
    let q = query.q.trim().to_string();
    if q.chars().count() < 2 {
        return Json(serde_json::json!({ "suggestions": [], "results": [] })).into_response();
    }
    if let Ok(cache) = state.suggest_cache.lock() {
        if let Some((at, val)) = cache.get(&q) {
            if at.elapsed() < Duration::from_millis(3000) {
                return Json(val.clone()).into_response();
            }
        }
    }
    let work = async {
        let (service, q2) = (state.service.clone(), q.clone());
        let (s, r) = tokio::join!(
            service.suggest(&q2),
            service.search_typed(ProviderKind::MovieBox, &q2, 1),
        );
        let suggestions: Vec<String> = s.unwrap_or_default().into_iter().take(8).collect();
        let mut results: Vec<CatalogItem> = r.unwrap_or_default().into_iter().take(8).collect();
        let mut seen = HashSet::new();
        results.retain(|c| seen.insert(c.id.value.clone()));
        serde_json::json!({ "suggestions": suggestions, "results": results })
    };
    let val = tokio::time::timeout(Duration::from_secs(4), work)
        .await
        .unwrap_or_else(|_| serde_json::json!({ "suggestions": [], "results": [] }));
    if let Ok(mut cache) = state.suggest_cache.lock() {
        cache.insert(q, (Instant::now(), val.clone()));
        if cache.len() > 200 {
            cache.clear();
        }
    }
    Json(val).into_response()
}

// ---------------------------------------------------------------- details / streams / subtitles

async fn details_handler(
    State(state): State<AppState>,
    Query(query): Query<DetailsQuery>,
) -> impl IntoResponse {
    // Open catalog: details browsing is public; streams/playback stay gated.
    let provider = ProviderKind::parse(&query.provider).unwrap_or(ProviderKind::MovieBox);
    match state.service.details_typed(provider, &query.id).await {
        Ok(details) => Json(details).into_response(),
        Err(e) => json_error(
            StatusCode::BAD_GATEWAY,
            &format!("details_unavailable: {e}"),
            "This title may have been removed or the provider is down. Try another title or provider.",
        ),
    }
}

async fn streams_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StreamsQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers).await {
        return r;
    }
    let provider = ProviderKind::parse(&query.provider).unwrap_or(ProviderKind::MovieBox);

    let releases_result = match provider {
        ProviderKind::MovieBox => state.service.client.episode_streams(&query.id, query.season, query.episode).await,
        ProviderKind::FourKHdHub => if let Some(ref c) = state.service.fourk_client {
            c.episode_streams(&query.id, query.season, query.episode).await
        } else {
            Err(moviebox_tui::providers::models::ProviderError::Unavailable("FourKHdHub not configured".to_string()))
        },
        ProviderKind::BdixCircleFtp => state.service.circleftp_client.episode_streams(&query.id, query.season, query.episode).await,
        ProviderKind::BdixDhakaFlix => state.service.dhakaflix_client.episode_streams(&query.id, query.season, query.episode).await,
        _ => Err(moviebox_tui::providers::models::ProviderError::Unavailable("Provider not implemented for streams".to_string()))
    };

    match releases_result {
        Ok(releases) => Json(releases).into_response(),
        Err(e) => json_error(
            StatusCode::BAD_GATEWAY,
            &format!("streams_unavailable: {:?}", e),
            "No playable sources right now. Try another episode, quality, or provider.",
        ),
    }
}

#[derive(Deserialize)]
struct SubtitlesQuery {
    id: String,
    resource_id: String,
    season: usize,
    episode: usize,
}

async fn subtitles_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SubtitlesQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers).await {
        return r;
    }
    let empty_siblings: Vec<String> = vec![];
    let result = state.service.get_ext_captions(&query.id, &query.resource_id, &empty_siblings, query.season, query.episode).await;
    match result {
        Ok(subs) => Json(subs).into_response(),
        Err(e) => json_error(StatusCode::BAD_GATEWAY, &format!("subtitles_unavailable: {e}"), "Subtitles could not be loaded. Playback will continue without them."),
    }
}

// ---------------------------------------------------------------- recommendations

const REC_STOPWORDS: &[&str] = &[
    "the","a","an","and","or","of","in","on","at","to","for","with","vs","part",
    "season","episode","movie","film","series","show","hd","4k","full","dubbed",
    "hindi","english","tamil","telugu","ii","iii","iv",
];

fn rec_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| {
            let w = w.trim().to_ascii_lowercase();
            if w.len() < 3 || REC_STOPWORDS.contains(&w.as_str()) {
                None
            } else {
                Some(w)
            }
        })
        .collect()
}

struct RecAffinity {
    kw_freq: HashMap<String, usize>,
    movie_ratio: f32,
    median_year: Option<i32>,
}

fn parse_rec_year(s: &str) -> Option<i32> {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .get(0..4)
        .and_then(|y| y.parse::<i32>().ok())
        .filter(|y| (1900..=2100).contains(y))
}

fn build_rec_affinity(history: &[WatchHistoryItem], favs: &[FavoriteItem]) -> RecAffinity {
    let mut kw_freq: HashMap<String, usize> = HashMap::new();
    let mut movies = 0usize;
    let mut total = 0usize;
    let mut years = vec![];
    for h in history.iter().take(100) {
        for t in rec_tokens(&h.title) {
            *kw_freq.entry(t).or_insert(0) += 1;
        }
        if h.stype == 1 { movies += 1; }
        total += 1;
        if let Some(y) = parse_rec_year(&h.release_year) { years.push(y); }
    }
    for f in favs.iter().take(250) {
        for t in rec_tokens(&f.title) {
            *kw_freq.entry(t).or_insert(0) += 2;
        }
        if f.stype == 1 { movies += 1; }
        total += 1;
        if let Some(y) = parse_rec_year(&f.release_year) { years.push(y); }
    }
    years.sort_unstable();
    let median_year = if years.is_empty() { None } else { years.get(years.len() / 2).copied() };
    RecAffinity {
        kw_freq,
        movie_ratio: if total == 0 { 0.5 } else { movies as f32 / total as f32 },
        median_year,
    }
}

fn score_rec_candidate(c: &CatalogItem, aff: &RecAffinity, rank_idx: usize, pool_len: usize) -> f32 {
    let genre_tokens: HashSet<String> = c.genre.as_deref().unwrap_or("")
        .split(',').flat_map(|g| rec_tokens(g)).collect();
    let title_tokens: HashSet<String> = rec_tokens(&c.title).into_iter().collect();
    let mut overlap: f32 = 0.0;
    for (kw, freq) in &aff.kw_freq {
        let w = (*freq).min(5) as f32 / 5.0;
        if genre_tokens.contains(kw) { overlap += 3.0 * w + 2.0; }
        else if title_tokens.contains(kw) { overlap += w + 1.0; }
    }
    let genre_score = overlap * 1.5;
    let is_movie = c.media_type == MediaType::Movie;
    let type_score = if (is_movie && aff.movie_ratio >= 0.5) || (!is_movie && aff.movie_ratio < 0.5) { 2.0 } else { 0.0 };
    let year_score = match (c.year.as_deref().map(parse_rec_year).unwrap_or(None), aff.median_year) {
        (Some(cy), Some(my)) => {
            let d = (cy - my).abs() as f32;
            (1.0 - d / 15.0).clamp(0.0, 1.0) * 2.0
        }
        _ => 0.5,
    };
    let popularity = if pool_len > 0 { 1.0 - (rank_idx as f32 / pool_len as f32) } else { 0.0 };
    genre_score + type_score + year_score + popularity
}

async fn build_recommendations(svc: Arc<MovieBoxService>, uid: &str, limit: usize) -> Vec<CatalogItem> {
    let history = load_history_scoped(Some(uid));
    let favs = load_favorites_scoped(Some(uid));
    if history.recent.is_empty() && favs.items.is_empty() {
        return svc.homepage("", 1).await.map(|(i, _)| i.into_iter().take(limit).collect()).unwrap_or_default();
    }
    let aff = build_rec_affinity(&history.recent, &favs.items);
    let mut pool: Vec<CatalogItem> = vec![];
    for p in 1..=3 {
        if let Ok((items, _)) = svc.homepage("", p).await {
            pool.extend(items);
        }
    }
    let mut top_kw: Vec<(String, usize)> = aff.kw_freq.clone().into_iter().collect();
    top_kw.sort_by_key(|(_, f)| std::cmp::Reverse(*f));
    for (kw, _) in top_kw.into_iter().take(4) {
        if let Ok(items) = svc.search_typed(ProviderKind::MovieBox, &kw, 1).await {
            pool.extend(items.into_iter().take(20));
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    for h in history.recent.iter() {
        seen.insert(format!("{}::{}", h.provider.to_ascii_lowercase(), h.subject_id));
        seen.insert(format!("{}|{}|{}", h.title.trim().to_ascii_lowercase(), h.stype, h.release_year.trim()));
    }
    for f in favs.items.iter() {
        seen.insert(format!("{}::{}", f.provider.to_ascii_lowercase(), f.subject_id));
        seen.insert(format!("{}|{}|{}", f.title.trim().to_ascii_lowercase(), f.stype, f.release_year.trim()));
    }
    let n = pool.len().max(1);
    let mut scored: Vec<(f32, CatalogItem)> = vec![];
    let mut emitted: HashSet<String> = HashSet::new();
    for (i, c) in pool.into_iter().enumerate() {
        let key1 = format!("{}::{}", c.id.provider.cache_key(), c.id.value);
        let stype = if c.media_type == MediaType::Series { 2 } else { 1 };
        let key2 = format!("{}|{}|{}", c.title.trim().to_ascii_lowercase(), stype, c.year.clone().unwrap_or_default().trim());
        if seen.contains(&key1) || seen.contains(&key2) { continue; }
        if !emitted.insert(key1) { continue; }
        let s = score_rec_candidate(&c, &aff, i, n);
        scored.push((s, c));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(limit).map(|(_, c)| c).collect()
}

async fn recommendations_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<RecsQuery>,
) -> impl IntoResponse {
    // Anonymous users get generic trending picks; logged-in users get personalized recs.
    let uid_opt = current_user(&state, &headers).await.map(|s| s.sub);
    let limit = q.limit.unwrap_or(24).clamp(1, 60);
    let Some(uid) = uid_opt else {
        let items = state
            .service
            .homepage("", 1)
            .await
            .map(|(i, _)| i.into_iter().take(limit).collect::<Vec<_>>())
            .unwrap_or_default();
        return Json(items).into_response();
    };
    match tokio::time::timeout(Duration::from_secs(10), build_recommendations(state.service.clone(), &uid, limit)).await {
        Ok(items) => Json(items).into_response(),
        Err(_) => json_error(StatusCode::GATEWAY_TIMEOUT, "recommendations_unavailable", "Recommendations timed out. Pull to retry."),
    }
}

// ---------------------------------------------------------------- headers / play

fn parse_headers(h_str: &str) -> Vec<(String, String)> {
    serde_json::from_str::<Vec<(String, String)>>(h_str).unwrap_or_default()
}

async fn play_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
    Query(query): Query<PlayQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    let headers = query.headers.as_ref().map(|s| parse_headers(s)).unwrap_or_default();

    let effective_url = match moviebox_tui::proxy::spawn_sidecar(&query.url, &headers, None) {
        Ok(local_url) => {
            println!("[play] Sidecar proxy started: {}", local_url);
            local_url
        }
        Err(e) => {
            println!("[play] Sidecar failed ({}), falling back to direct proxy URL", e);
            let headers_param = if let Some(ref h) = query.headers {
                format!("&headers={}", urlencoding::encode(h))
            } else {
                String::new()
            };
            format!(
                "http://127.0.0.1:3000/api/proxy?url={}{}",
                urlencoding::encode(&query.url),
                headers_param
            )
        }
    };

    let available = moviebox_tui::player::detect();
    println!("[play] Available players: {:?}", available);
    println!("[play] Playing URL: {}", &effective_url);

    if available.is_empty() {
        println!("[play] No players detected, trying open::that");
        return match open::that(&effective_url) {
            Ok(_) => (StatusCode::OK, "Opened in default application").into_response(),
            Err(e) => {
                let msg = format!("No video player found. Install VLC and try again. Error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        };
    }

    let mut local_subtitle_path: Option<String> = None;
    if let Some(sub_url) = &query.subtitle_url {
        if check_ssrf_url(sub_url).is_ok() {
            println!("[play] Fetching subtitle from: {}", sub_url);
            if let Ok(resp) = reqwest::get(sub_url).await {
                if let Ok(bytes) = resp.bytes().await {
                    let temp_dir = std::env::temp_dir();
                    let sub_path = temp_dir.join(format!("moviebox_web_sub_{}.srt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()));
                    if std::fs::write(&sub_path, bytes).is_ok() {
                        local_subtitle_path = Some(sub_path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    for player_kind in &available {
        let empty_h: Vec<(String, String)> = vec![];
        let mut cmd = command(
            *player_kind,
            &effective_url,
            local_subtitle_path.as_deref(),
            &empty_h,
            None,
            None,
            None,
        );
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0000_0010);
        }

        match cmd.spawn() {
            Ok(_child) => {
                return (StatusCode::OK, format!("Launched {:?} successfully", player_kind)).into_response();
            }
            Err(e) => {
                println!("[play] Failed to spawn {:?}: {}", player_kind, e);
                continue;
            }
        }
    }

    let msg = format!("All detected players ({:?}) failed to launch.", available);
    (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
}

// ---------------------------------------------------------------- proxy + download

fn proxify_line(base: &url::Url, line: &str, headers_param: &str) -> String {
    let t = line.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.starts_with('#') {
        // Rewrite URIs embedded in HLS tags (KEY/MAP/MEDIA) so auth still flows through proxy.
        if let Some(s) = t.find("URI=\"") {
            let rest = &t[s + 5..];
            if let Some(e) = rest.find('"') {
                let raw_uri = &rest[..e];
                if let Ok(abs) = base.join(raw_uri) {
                    let prox = format!("/api/proxy?url={}{}", urlencoding::encode(abs.as_str()), headers_param);
                    return format!("{}{}{}", &t[..s + 5], prox, &t[s + 5 + e..]);
                }
            }
        }
        return t.to_string();
    }
    match base.join(t) {
        Ok(abs) => format!("/api/proxy?url={}{}", urlencoding::encode(abs.as_str()), headers_param),
        Err(_) => t.to_string(),
    }
}

async fn proxy_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
    Query(query): Query<PlayQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    let target = match check_ssrf_url(&query.url) {
        Ok(u) => u,
        Err((c, m)) => return (c, m).into_response(),
    };

    let mut req_headers = reqwest::header::HeaderMap::new();
    if let Some(ref h_str) = query.headers {
        for (k, v) in parse_headers(h_str) {
            if let Ok(k_name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(v_val) = reqwest::header::HeaderValue::from_str(&v) {
                    req_headers.insert(k_name, v_val);
                }
            }
        }
    }

    let res = match state.http_client.get(target).headers(req_headers).send().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };

    let final_url_str = res.url().as_str().to_string();
    let final_url = match url::Url::parse(&final_url_str) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_GATEWAY, "invalid upstream url").into_response(),
    };
    // Post-redirect SSRF check.
    if let Some(host) = final_url.host_str() {
        if is_blocked_host(host) {
            return (StatusCode::FORBIDDEN, "blocked destination").into_response();
        }
    }

    let is_m3u8 = final_url_str.contains(".m3u8")
        || res.headers().get("content-type").map_or(false, |v| {
            v.to_str().unwrap_or("").to_lowercase().contains("mpegurl")
        });

    if is_m3u8 {
        const MAX_MANIFEST: usize = 10 * 1024 * 1024;
        if let Some(len) = res.content_length() {
            if len > MAX_MANIFEST as u64 {
                return (StatusCode::PAYLOAD_TOO_LARGE, "manifest too large").into_response();
            }
        }
        let bytes = match res.bytes().await {
            Ok(b) => b,
            Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
        };
        if bytes.len() > MAX_MANIFEST {
            return (StatusCode::PAYLOAD_TOO_LARGE, "manifest too large").into_response();
        }
        let text = String::from_utf8_lossy(&bytes);
        let headers_param = if let Some(ref h) = query.headers {
            format!("&headers={}", urlencoding::encode(h))
        } else {
            String::new()
        };
        let mut rewritten = String::new();
        for line in text.lines() {
            rewritten.push_str(&proxify_line(&final_url, line, &headers_param));
            rewritten.push('\n');
        }
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/vnd.apple.mpegurl")
            .header("Access-Control-Allow-Origin", "*");
        return response.body(Body::from(rewritten)).unwrap();
    }

    let mut response = Response::builder()
        .status(res.status().as_u16())
        .header("Access-Control-Allow-Origin", "*");
    for (name, value) in res.headers() {
        if name == "content-type" || name == "content-length" || name == "accept-ranges" {
            response = response.header(name.as_str(), value.as_bytes());
        }
    }
    let stream = res.bytes_stream();
    response.body(Body::from_stream(stream)).unwrap()
}

fn is_hls_or_dash_url(u: &str) -> bool {
    let lower = u.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    path.ends_with(".m3u8") || path.ends_with(".mpd") || lower.contains("/dash/")
}

fn infer_download_extension(url: &str, content_type: Option<&str>) -> &'static str {
    if let Some(ct) = content_type.map(str::to_ascii_lowercase) {
        if ct.contains("matroska") || ct.contains("x-matroska") {
            return "mkv";
        }
        if ct.contains("webm") {
            return "webm";
        }
        if ct.contains("mpeg2") || ct.contains("mp2t") {
            return "ts";
        }
        if ct.contains("mp4") {
            return "mp4";
        }
    }
    url.split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .filter(|e| matches!(e.as_str(), "mp4" | "mkv" | "webm" | "ts"))
        .map(|e| match e.as_str() {
            "mkv" => "mkv",
            "webm" => "webm",
            "ts" => "ts",
            _ => "mp4",
        })
        .unwrap_or("mp4")
}

async fn download_handler(
    State(state): State<AppState>,
    incoming: HeaderMap,
    Query(query): Query<DownloadQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &incoming).await {
        return r;
    }
    let parsed = match check_ssrf_url(&query.url) {
        Ok(u) => u,
        Err((c, m)) => {
            return (c, Json(serde_json::json!({ "error": m }))).into_response();
        }
    };
    if is_hls_or_dash_url(parsed.as_str()) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "HLS/DASH stream cannot be downloaded directly",
                "hint": "Use Play in browser or App (VLC). Direct download only supports mp4/mkv/webm/ts files."
            })),
        )
            .into_response();
    }
    let mut req_headers = reqwest::header::HeaderMap::new();
    if let Some(ref h_str) = query.headers {
        for (k, v) in parse_headers(h_str) {
            if k.eq_ignore_ascii_case("range") || k.eq_ignore_ascii_case("if-range") {
                continue;
            }
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(&v),
            ) {
                req_headers.insert(name, val);
            }
        }
    }
    for key in [header::RANGE, header::IF_RANGE] {
        if let Some(v) = incoming.get(&key) {
            if let Ok(n) = reqwest::header::HeaderName::from_bytes(key.as_str().as_bytes()) {
                if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                    req_headers.insert(n, val);
                }
            }
        }
    }
    let res = match state.http_client.get(parsed).headers(req_headers).send().await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };
    let final_url = res.url().clone();
    if let Some(host) = final_url.host_str() {
        if is_blocked_host(host) {
            return (StatusCode::FORBIDDEN, Json(serde_json::json!({ "error": "blocked destination" }))).into_response();
        }
    }
    let ct = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if ct.contains("mpegurl") || ct.contains("dash+xml") || is_hls_or_dash_url(final_url.as_str()) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "HLS/DASH stream cannot be downloaded directly",
                "hint": "Use Play in browser or App (VLC). Direct download only supports mp4/mkv/webm/ts files."
            })),
        )
            .into_response();
    }
    let ext = infer_download_extension(final_url.as_str(), Some(&ct));
    let stem_raw = query.filename.as_deref().unwrap_or(moviebox_tui::download::DEFAULT_STREAM_NAME);
    let stem = moviebox_tui::download::safe_file_stem(stem_raw);
    let filename = format!("{stem}.{ext}");
    let disposition = format!(
        "attachment; filename=\"{filename}\"; filename*=UTF-8''{}",
        urlencoding::encode(&filename)
    );
    let status = res.status();
    let mut builder = Response::builder()
        .status(status.as_u16())
        .header(
            "Content-Type",
            match ext {
                "mkv" => "video/x-matroska",
                "webm" => "video/webm",
                "ts" => "video/mp2t",
                _ => "video/mp4",
            },
        )
        .header("Content-Disposition", disposition)
        .header("Access-Control-Allow-Origin", "*")
        .header("X-Content-Type-Options", "nosniff")
        .header("Accept-Ranges", "bytes");
    for name in ["content-length", "content-range", "accept-ranges", "etag", "last-modified"] {
        if let Some(v) = res.headers().get(name) {
            builder = builder.header(name, v.as_bytes());
        }
    }
    let stream = res.bytes_stream();
    builder.body(Body::from_stream(stream)).unwrap()
}

// ---------------------------------------------------------------- background downloads (TUI parity)

#[derive(Deserialize)]
struct StartDlReq {
    url: String,
    headers: Option<Vec<(String, String)>>,
    filename: Option<String>,
}

async fn start_download_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
    Json(req): Json<StartDlReq>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    if req.url.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "missing url", "No stream URL was provided.");
    }
    match state
        .dl
        .start(
            state.http_client.clone(),
            req.url,
            req.headers.unwrap_or_default(),
            req.filename,
        )
        .await
    {
        Ok(job) => Json(job).into_response(),
        Err((c, m)) => json_error(c, &m, "Could not start the download."),
    }
}

async fn list_downloads_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    Json(state.dl.list().await).into_response()
}

async fn get_download_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    match state.dl.get(&id).await {
        Some(job) => Json(job).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "download not found", "It may have been dismissed."),
    }
}

async fn retry_download_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    match state.dl.retry(state.http_client.clone(), &id).await {
        Some(job) => Json(job).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "download not found", "It may have been dismissed."),
    }
}

async fn delete_download_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    // Cancel first so a running worker stops, then drop record + files.
    state.dl.cancel(&id).await;
    // Give the worker a beat to observe the flag before we delete files.
    tokio::time::sleep(Duration::from_millis(150)).await;
    if state.dl.remove(&id).await {
        StatusCode::OK.into_response()
    } else {
        json_error(StatusCode::NOT_FOUND, "download not found", "It may have been dismissed.")
    }
}

async fn download_file_handler(
    State(state): State<AppState>,
    headers_map: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(r) = require_auth(&state, &headers_map).await {
        return r;
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return json_error(StatusCode::BAD_REQUEST, "invalid id", "Bad download id.");
    }
    let Some(job) = state.dl.get(&id).await else {
        return json_error(StatusCode::NOT_FOUND, "download not found", "It may have been dismissed.");
    };
    if job.status != "completed" {
        return json_error(
            StatusCode::CONFLICT,
            "download not ready",
            "Wait until the download reaches 100% before saving.",
        );
    }
    let path = state.dl.file_path(&job);
    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => {
            return json_error(StatusCode::NOT_FOUND, "file missing", "The file was removed from disk.");
        }
    };
    let len = meta.len();
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("mp4") {
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "ts" => "video/mp2t",
        _ => "video/mp4",
    };
    let disposition = format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        job.filename,
        urlencoding::encode(&job.filename)
    );

    // Honor Range so large saves can resume (browsers / download managers).
    let (start, end, ranged) = match headers_map.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        Some(spec) => {
            let spec = spec.trim().strip_prefix("bytes=").unwrap_or("").trim();
            let (s, e) = spec.split_once('-').unwrap_or((spec, ""));
            let s: u64 = s.parse().unwrap_or(0);
            let e: u64 = if e.is_empty() { len.saturating_sub(1) } else { e.parse().unwrap_or(len.saturating_sub(1)) };
            let e = e.min(len.saturating_sub(1));
            if s > e || s >= len {
                return (StatusCode::RANGE_NOT_SATISFIABLE, "invalid range").into_response();
            }
            (s, e, true)
        }
        None => (0, len.saturating_sub(1), false),
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => {
            return json_error(StatusCode::NOT_FOUND, "file missing", "The file was removed from disk.");
        }
    };
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = file;
    if start > 0 {
        if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "seek failed", "Could not read the file.");
        }
    }
    let remaining = end - start + 1;
    let stream = futures::stream::unfold((file, remaining), |(mut f, mut left)| async move {
        if left == 0 {
            return None;
        }
        let mut buf = vec![0u8; 64 * 1024];
        if (left as usize) < buf.len() {
            buf.resize(left as usize, 0);
        }
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                left -= n as u64;
                buf.truncate(n);
                Some((Ok::<_, axum::Error>(bytes::Bytes::from(buf)), (f, left)))
            }
            Err(e) => Some((Err(axum::Error::new(e)), (f, 0))),
        }
    });
    let mut builder = Response::builder()
        .header("Content-Type", mime)
        .header("Content-Disposition", disposition)
        .header("Accept-Ranges", "bytes")
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Length", remaining.to_string());
    if ranged {
        builder = builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Range", format!("bytes {start}-{end}/{len}"));
    }
    builder.body(Body::from_stream(stream)).unwrap()
}

// ---------------------------------------------------------------- auth + per-user storage

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{:02x}", b)).collect()
}

fn short_hash(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex(h.finalize()).chars().take(16).collect()
}

fn scoped_history_path(uid: Option<&str>) -> Option<std::path::PathBuf> {
    let base = moviebox_tui::config::data_dir()?;
    match uid {
        Some(u) => Some(base.join(format!("history_{}.json", short_hash(u)))),
        None => Some(base.join("history.json")),
    }
}

fn scoped_favorites_path(uid: Option<&str>) -> Option<std::path::PathBuf> {
    let base = moviebox_tui::config::data_dir()?;
    match uid {
        Some(u) => Some(base.join(format!("favorites_{}.json", short_hash(u)))),
        None => Some(base.join("favorites.json")),
    }
}

fn load_history_scoped(uid: Option<&str>) -> HistoryManager {
    if uid.is_none() {
        return HistoryManager::new();
    }
    let Some(path) = scoped_history_path(uid) else {
        return HistoryManager::new();
    };
    if !path.exists() {
        return HistoryManager::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(c) => serde_json::from_str::<HistoryManager>(&c).unwrap_or_else(|_| HistoryManager::new()),
        Err(_) => HistoryManager::new(),
    }
}

fn load_favorites_scoped(uid: Option<&str>) -> FavoritesManager {
    if uid.is_none() {
        return FavoritesManager::new();
    }
    let Some(path) = scoped_favorites_path(uid) else {
        return FavoritesManager::new();
    };
    FavoritesManager::load_from_path(&path)
}

fn session_token(secret: &str, payload_b64: &str) -> String {
    use sha2::{Digest, Sha256};
    let eff = if secret.len() >= 16 { secret } else { "moviebox-dev-fallback-secret-please-set-SESSION_SECRET" };
    let mut h = Sha256::new();
    h.update(eff.as_bytes());
    h.update(b".");
    h.update(payload_b64.as_bytes());
    format!("{}.{}", payload_b64, hex(h.finalize()))
}

fn verify_session_token(secret: &str, token: &str) -> Option<serde_json::Value> {
    let (payload_b64, sig) = token.split_once('.')?;
    let expected = session_token(secret, payload_b64);
    let expected_sig = expected.split_once('.')?.1;
    if expected_sig.len() != sig.len() {
        return None;
    }
    // constant-time-ish compare
    let mut diff = 0u8;
    for (a, b) in expected_sig.bytes().zip(sig.bytes()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return None;
    }
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok()
}

fn session_from_cookie(auth: &AuthConfig, headers: &HeaderMap) -> Option<Session> {
    let cookie_hdr = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_hdr.split(';') {
        let part = part.trim();
        let Some(val) = part.strip_prefix("mb_session=") else { continue };
        let payload = verify_session_token(&auth.session_secret, val)?;
        let exp = payload.get("exp")?.as_u64()?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        if exp < now {
            return None;
        }
        return serde_json::from_value::<Session>(payload).ok();
    }
    None
}

async fn current_user(state: &AppState, headers: &HeaderMap) -> Option<Session> {
    let sess = session_from_cookie(&state.auth, headers)?;
    // ensure still in live map (supports logout invalidation)
    let map = state.sessions.read().await;
    let token = headers.get(header::COOKIE)?.to_str().ok()?;
    // find mb_session value
    for part in token.split(';') {
        let v = part.trim().strip_prefix("mb_session=")?;
        if map.contains_key(v) {
            return Some(sess);
        }
    }
    None
}

/// Two modes:
/// - development (default, `APP_MODE` unset/`development`): everything open, no login needed.
/// - production (`APP_MODE=production`): watching + library require Google login.
fn app_mode() -> String {
    std::env::var("APP_MODE")
        .or_else(|_| std::env::var("MOVIEBOX_MODE"))
        .unwrap_or_else(|_| "development".to_string())
        .to_ascii_lowercase()
}

fn login_required() -> bool {
    matches!(app_mode().as_str(), "production" | "prod")
}

fn dev_session() -> Session {
    Session {
        sub: "dev-local".to_string(),
        email: String::new(),
        name: "Developer".to_string(),
        picture: String::new(),
        exp: u64::MAX,
    }
}

/// Auth gate: in development mode everything is open (returns a local dev
/// session); in production mode a valid Google session is required.
async fn require_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Session, Response> {
    if !login_required() {
        return Ok(dev_session());
    }
    match current_user(state, headers).await {
        Some(s) => Ok(s),
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "login_required", "hint": "Sign in with Google to continue." })),
        )
            .into_response()),
    }
}

async fn config_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mode = app_mode();
    Json(serde_json::json!({
        "mode": mode,
        "login_required": login_required(),
        "auth_configured": state.auth.configured(),
    }))
}

fn json_error(code: StatusCode, error: &str, hint: &str) -> Response {
    (code, Json(serde_json::json!({ "error": error, "hint": hint }))).into_response()
}

async fn auth_login_handler(State(state): State<AppState>) -> impl IntoResponse {
    if !state.auth.configured() {
        return (StatusCode::SERVICE_UNAVAILABLE, "Google OAuth not configured. Set GOOGLE_CLIENT_ID/SECRET in .env").into_response();
    }
    let params = [
        ("client_id", state.auth.google_client_id.as_str()),
        ("redirect_uri", state.auth.google_redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", "openid email profile"),
        ("access_type", "online"),
        ("prompt", "select_account"),
    ];
    let qs: Vec<String> = params.iter().map(|(k, v)| format!("{}={}", k, urlencoding::encode(v))).collect();
    let url = format!("https://accounts.google.com/o/oauth2/v2/auth?{}", qs.join("&"));
    Redirect::temporary(&url).into_response()
}

async fn auth_callback_handler(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.auth.configured() {
        return (StatusCode::SERVICE_UNAVAILABLE, "Google OAuth not configured").into_response();
    }
    let Some(code) = q.get("code") else {
        return (StatusCode::BAD_REQUEST, "missing code").into_response();
    };
    // Exchange code for tokens.
    let token_res = state
        .http_client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code.as_str()),
            ("client_id", state.auth.google_client_id.as_str()),
            ("client_secret", state.auth.google_client_secret.as_str()),
            ("redirect_uri", state.auth.google_redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await;
    let token_res = match token_res {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("token exchange failed: {e}")).into_response(),
    };
    let token_json: serde_json::Value = match token_res.json().await {
        Ok(j) => j,
        Err(_) => return (StatusCode::BAD_GATEWAY, "invalid token response").into_response(),
    };
    let Some(access) = token_json.get("access_token").and_then(|v| v.as_str()) else {
        return (StatusCode::BAD_GATEWAY, "no access_token").into_response();
    };
    let user_res = state
        .http_client
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .bearer_auth(access)
        .send()
        .await;
    let user_res = match user_res {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("userinfo failed: {e}")).into_response(),
    };
    let user_json: serde_json::Value = match user_res.json().await {
        Ok(j) => j,
        Err(_) => return (StatusCode::BAD_GATEWAY, "invalid userinfo").into_response(),
    };
    if user_json.get("email_verified").and_then(|v| v.as_bool()) == Some(false) {
        return (StatusCode::FORBIDDEN, "email not verified").into_response();
    }
    let sub = user_json.get("sub").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    if sub.is_empty() || state.auth.session_secret.len() < 16 {
        return (StatusCode::INTERNAL_SERVER_ERROR, "server session misconfigured").into_response();
    }
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 30 * 24 * 3600)
        .unwrap_or(0);
    let sess = Session {
        sub: sub.clone(),
        email: user_json.get("email").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        name: user_json.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        picture: user_json.get("picture").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        exp,
    };
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&sess).unwrap_or_default());
    let token = session_token(&state.auth.session_secret, &payload);
    state.sessions.write().await.insert(token.clone(), sess);
    let cookie = format!("mb_session={token}; HttpOnly; Path=/; SameSite=Lax; Max-Age=2592000");
    let mut resp = Redirect::temporary("/").into_response();
    resp.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    resp
}

async fn auth_me_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match current_user(&state, &headers).await {
        Some(s) => Json(serde_json::json!({ "sub": s.sub, "email": s.email, "name": s.name, "picture": s.picture })).into_response(),
        None => (StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    }
}

async fn auth_logout_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(cookie_hdr) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in cookie_hdr.split(';') {
            if let Some(v) = part.trim().strip_prefix("mb_session=") {
                state.sessions.write().await.remove(v);
            }
        }
    }
    let mut resp = StatusCode::OK.into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("mb_session=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0"),
    );
    resp
}

// ---------------------------------------------------------------- history / favorites (per-user aware)

async fn get_history_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let session = match require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let history = load_history_scoped(Some(&session.sub));
    Json(history.recent).into_response()
}

#[derive(Deserialize)]
struct PostHistoryReq {
    item: WatchHistoryItem,
    progress: u64,
    duration: Option<u64>,
    completed: bool,
}

async fn post_history_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PostHistoryReq>,
) -> impl IntoResponse {
    let session = match require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let uid_str = session.sub;
    let mut history = load_history_scoped(Some(&uid_str));
    history.update_progress(req.item.clone(), req.progress, req.duration, req.completed);
    // persist to scoped path (HistoryManager::save writes global; do scoped manually)
    if let Some(path) = scoped_history_path(Some(&uid_str)) {
        if let Ok(content) = serde_json::to_string(&history) {
            let _ = moviebox_tui::cache::atomic_write_file(&path, content.as_bytes());
        }
    }
    StatusCode::OK.into_response()
}

async fn get_favorites_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let session = match require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let favs = load_favorites_scoped(Some(&session.sub));
    Json(favs.items).into_response()
}

#[derive(Deserialize)]
struct FavoriteReq {
    item: FavoriteItem,
}

async fn add_favorite_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FavoriteReq>,
) -> impl IntoResponse {
    let session = match require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let uid_str = session.sub;
    let mut favs = load_favorites_scoped(Some(&uid_str));
    let now_favorited = favs.toggle(req.item);
    if let Some(path) = scoped_favorites_path(Some(&uid_str)) {
        favs.save_to_path(&path);
    }
    Json(serde_json::json!({ "is_favorite": now_favorited })).into_response()
}

async fn remove_favorite_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FavoriteReq>,
) -> impl IntoResponse {
    let session = match require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let uid_str = session.sub;
    let mut favs = load_favorites_scoped(Some(&uid_str));
    favs.remove(&req.item.identity());
    if let Some(path) = scoped_favorites_path(Some(&uid_str)) {
        favs.save_to_path(&path);
    }
    StatusCode::OK.into_response()
}
