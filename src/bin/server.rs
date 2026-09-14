use axum::{
    extract::{Query, State, Json},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use reqwest::Client;
use moviebox_tui::providers::models::ProviderKind;
use moviebox_tui::providers::ReleaseProvider;
use moviebox_tui::service::MovieBoxService;
use moviebox_tui::player::command;
use moviebox_tui::history::{HistoryManager, WatchHistoryItem};
use moviebox_tui::favorites::{FavoritesManager, FavoriteItem};
use serde::Deserialize;
use std::sync::Arc;
use tokio::net::TcpListener;
use axum::http::StatusCode;
use axum::body::Body;

#[derive(Clone)]
struct AppState {
    service: Arc<MovieBoxService>,
    http_client: Client,
}

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

#[tokio::main]
async fn main() {
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
            .timeout(std::time::Duration::from_secs(30))
            .build().unwrap(),
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/search", get(search_handler))
        .route("/api/details", get(details_handler))
        .route("/api/streams", get(streams_handler))
        .route("/api/subtitles", get(subtitles_handler))
        .route("/api/play", get(play_handler))
        .route("/api/proxy", get(proxy_handler))
        .route("/api/homepage", get(homepage_handler))
        .route("/api/history", get(get_history_handler).post(post_history_handler))
        .route("/api/favorites", get(get_favorites_handler).post(add_favorite_handler).delete(remove_favorite_handler))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Web server running at http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> impl IntoResponse {
    match std::fs::read_to_string("src/bin/index.html") {
        Ok(content) => Html(content).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load index.html").into_response(),
    }
}

#[derive(Deserialize)]
struct HomepageQuery {
    page: Option<usize>,
    provider: Option<String>,
}

async fn homepage_handler(
    State(state): State<AppState>,
    Query(query): Query<HomepageQuery>,
) -> impl IntoResponse {
    let page = query.page.unwrap_or(1);
    let provider = query.provider.unwrap_or_else(|| "moviebox".to_string());
    let provider_kind = ProviderKind::parse(&provider).unwrap_or(ProviderKind::MovieBox);
    
    if provider_kind == ProviderKind::MovieBox {
        match state.service.homepage("", page).await {
            Ok((items, _)) => Json(items).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        match state.service.search_typed(provider_kind, "", page).await {
            Ok(items) => Json(items).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }
}

async fn search_handler(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let mut all_results = Vec::new();
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
    
    let mut futures = Vec::new();
    for provider in providers {
        let service = state.service.clone();
        let q = query.q.clone();
        futures.push(tokio::spawn(async move {
            service.search_typed(provider, &q, page).await
        }));
    }
    
    for f in futures {
        if let Ok(Ok(items)) = f.await {
            all_results.extend(items);
        }
    }
    
    Json(all_results).into_response()
}

async fn details_handler(
    State(state): State<AppState>,
    Query(query): Query<DetailsQuery>,
) -> impl IntoResponse {
    let provider = ProviderKind::parse(&query.provider).unwrap_or(ProviderKind::MovieBox);
    match state.service.details_typed(provider, &query.id).await {
        Ok(details) => Json(details).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn streams_handler(
    State(state): State<AppState>,
    Query(query): Query<StreamsQuery>,
) -> impl IntoResponse {
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Provider error: {:?}", e)).into_response(),
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
    Query(query): Query<SubtitlesQuery>,
) -> impl IntoResponse {
    let empty_siblings: Vec<String> = vec![];
    let result = state.service.get_ext_captions(&query.id, &query.resource_id, &empty_siblings, query.season, query.episode).await;
    match result {
        Ok(subs) => Json(subs).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

fn parse_headers(h_str: &str) -> Vec<(String, String)> {
    serde_json::from_str::<Vec<(String, String)>>(h_str).unwrap_or_default()
}

async fn play_handler(
    Query(query): Query<PlayQuery>,
) -> impl IntoResponse {
    let headers = query.headers.as_ref().map(|s| parse_headers(s)).unwrap_or_default();
    
    // Use the exact same proxy sidecar the TUI uses for VLC playback.
    // This spawns a lightweight local proxy on a random port that handles all auth headers,
    // and gives VLC a clean short URL like http://127.0.0.1:PORT/https/cdn.example.com/video.m3u8
    let effective_url = match moviebox_tui::proxy::spawn_sidecar(&query.url, &headers, None) {
        Ok(local_url) => {
            println!("[play] Sidecar proxy started: {}", local_url);
            local_url
        }
        Err(e) => {
            println!("[play] Sidecar failed ({}), falling back to direct proxy URL", e);
            // Fallback: build a URL through our own web server proxy
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

    // Detect which players are actually installed
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
        println!("[play] Fetching subtitle from: {}", sub_url);
        if let Ok(resp) = reqwest::get(sub_url).await {
            if let Ok(bytes) = resp.bytes().await {
                let temp_dir = std::env::temp_dir();
                let sub_path = temp_dir.join(format!("moviebox_web_sub_{}.srt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()));
                if std::fs::write(&sub_path, bytes).is_ok() {
                    local_subtitle_path = Some(sub_path.to_string_lossy().to_string());
                    println!("[play] Saved subtitle to: {:?}", local_subtitle_path);
                }
            }
        }
    }

    // Try each detected player
    for player_kind in &available {
        let empty_h: Vec<(String, String)> = vec![];
        
        let mut cmd = command(
            *player_kind,
            &effective_url,
            local_subtitle_path.as_deref(),
            &empty_h,  // headers are handled by the sidecar proxy
            None,
            None,
            None
        );
        
        // Set up stdio like the TUI does
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        
        println!("[play] Trying {:?}...", player_kind);
        
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NEW_CONSOLE (0x00000010) breaks the child out of the parent's hidden window state,
            // forcing the GUI window to become visible!
            cmd.creation_flags(0x0000_0010);
        }
        
        match cmd.spawn() {
            Ok(_child) => {
                println!("[play] Successfully spawned {:?}", player_kind);
                return (StatusCode::OK, format!("Launched {:?} successfully", player_kind)).into_response();
            }
            Err(e) => {
                println!("[play] Failed to spawn {:?}: {}", player_kind, e);
                continue;
            }
        }
    }
    
    let msg = format!("All detected players ({:?}) failed to launch.", available);
    println!("[play] {}", msg);
    (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
}

async fn proxy_handler(
    State(state): State<AppState>,
    Query(query): Query<PlayQuery>,
) -> impl IntoResponse {
    let base_url = query.url.clone();
    
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
    
    let res = match state.http_client.get(&base_url).headers(req_headers).send().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response()
    };
    
    let final_url = res.url().as_str().to_string();
    
    let is_m3u8 = final_url.contains(".m3u8") || 
                  res.headers().get("content-type").map_or(false, |v| v.to_str().unwrap_or("").to_lowercase().contains("mpegurl"));

    if is_m3u8 {
        if let Ok(text) = res.text().await {
            let headers_param = if let Some(ref h) = query.headers {
                format!("&headers={}", urlencoding::encode(h))
            } else {
                String::new()
            };
            
            let mut rewritten = String::new();
            for line in text.lines() {
                if line.starts_with("#") || line.trim().is_empty() {
                    rewritten.push_str(line);
                    rewritten.push('\n');
                } else {
                    let full_url = if line.starts_with("http") {
                        line.to_string()
                    } else if line.starts_with("/") {
                        let parsed = url::Url::parse(&final_url).unwrap();
                        format!("{}://{}{}", parsed.scheme(), parsed.host_str().unwrap(), line)
                    } else {
                        let parsed = url::Url::parse(&final_url).unwrap();
                        let base_path = parsed.path();
                        let dir = &base_path[..base_path.rfind('/').unwrap_or(0) + 1];
                        format!("{}://{}{}{}", parsed.scheme(), parsed.host_str().unwrap(), dir, line)
                    };
                    rewritten.push_str(&format!("/api/proxy?url={}{}\n", urlencoding::encode(&full_url), headers_param));
                }
            }
            let response = axum::response::Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/vnd.apple.mpegurl")
                .header("Access-Control-Allow-Origin", "*");
            response.body(Body::from(rewritten)).unwrap()
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read m3u8").into_response()
        }
    } else {
        let mut response = axum::response::Response::builder()
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
}

async fn get_history_handler() -> impl IntoResponse {
    let history = HistoryManager::new();
    Json(history.recent).into_response()
}

#[derive(Deserialize)]
struct PostHistoryReq {
    item: WatchHistoryItem,
    progress: u64,
    duration: Option<u64>,
    completed: bool,
}

async fn post_history_handler(Json(req): Json<PostHistoryReq>) -> impl IntoResponse {
    let mut history = HistoryManager::new();
    history.update_progress(req.item, req.progress, req.duration, req.completed);
    StatusCode::OK.into_response()
}

async fn get_favorites_handler() -> impl IntoResponse {
    let favs = FavoritesManager::new();
    Json(favs.items).into_response()
}

#[derive(Deserialize)]
struct FavoriteReq {
    item: FavoriteItem,
}

async fn add_favorite_handler(Json(req): Json<FavoriteReq>) -> impl IntoResponse {
    let mut favs = FavoritesManager::new();
    let now_favorited = favs.toggle(req.item);
    Json(serde_json::json!({ "is_favorite": now_favorited })).into_response()
}

async fn remove_favorite_handler(Json(req): Json<FavoriteReq>) -> impl IntoResponse {
    let mut favs = FavoritesManager::new();
    favs.remove(&req.item.identity());
    StatusCode::OK.into_response()
}

