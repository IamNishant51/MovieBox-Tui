# AGENTS.md: MovieBox-Web Architecture & Developer Guide

Welcome, future AI Agent! This document explains the architecture, design decisions, and solutions implemented in this project to help you navigate and modify the codebase effectively.

## 📌 Project Overview
**MovieBox-Web** (codenamed "NISHANTFLIX") is a web-based movie and TV show streaming application. It was originally adapted from the terminal-based `moviebox-tui` project but transformed into a premium, Netflix-like web experience. It supports multi-provider search, streaming via browser, streaming via desktop apps (VLC), and downloading.

## 🏗️ Architecture

### 1. Backend (Rust / Axum)
- **File:** `src/bin/server.rs`
- **Purpose:** An HTTP server using `axum` that wraps the core API logic of `moviebox_tui`.
- **Key Endpoints:**
  - `GET /` -> Serves the Vue frontend (`index.html`).
  - `GET /api/homepage` -> Fetches trending movies/series.
  - `GET /api/search` -> Searches across all providers.
  - `GET /api/details` -> Fetches seasons/episodes for a specific movie/series.
  - `GET /api/streams` -> Fetches available playback mirrors for a title.
  - `GET /api/proxy` -> **Crucial Component!** Bypasses CloudFront/CORS restrictions by proxying `.m3u8` playlists and rewriting relative URLs to point back through the proxy with injected auth headers (Cookie, Referer).
  - `GET /api/play` -> Spawns a local video player (VLC) to play streams outside the browser.

### 2. Frontend (HTML / Vue 2 / Tailwind CSS)
- **File:** `src/bin/index.html`
- **Purpose:** A single-page application heavily stylized to look like Netflix. It uses Vue 2 (via CDN) for reactivity and Tailwind CSS for styling.
- **Features:** 
  - Shimmering skeleton loaders for all network requests.
  - `Plyr.io` integrated with `hls.js` for premium browser playback with quality and audio track selection.
  - Direct integration with the `/api/play` endpoint for launching VLC locally.

## 🚨 Critical Problems Solved (Do Not Revert!)

### 1. The VLC "Invisible Window" / Immediate Close Problem (Windows)
**The Issue:** When spawning VLC from a background Rust process or from pseudo-terminals like Git Bash (mintty), VLC would inherit a hidden console state (`SW_HIDE`). The audio would play, but the user couldn't see the video window. Furthermore, if a hidden instance got stuck in the background, clicking play would spawn a new VLC which simply forwarded the URL to the hidden instance and closed immediately. Finally, if the server was run with `HOST=0.0.0.0`, VLC on Windows would fail instantly (and close) because Windows cannot route to `0.0.0.0`.
**The Solution:** 
1. **Force a New Console:** In `src/bin/server.rs`'s `play_handler`, we use `cmd.creation_flags(0x0000_0010)` (`CREATE_NEW_CONSOLE`). This forces Windows to allocate a brand new, visible window station for VLC, bypassing any hidden terminal inheritance (like from Git Bash).
2. **Prevent IPC Forwarding:** In `src/player.rs`'s `vlc_command`, we explicitly pass `--no-one-instance`. This guarantees VLC opens a new, visible window instead of forwarding the video to a stuck, hidden background instance and closing immediately.
3. **Rewrite 0.0.0.0:** In `src/proxy.rs`, we intercept `0.0.0.0` host addresses and rewrite them to `127.0.0.1` for proxy URLs and DASH manifests so that VLC on Windows can connect successfully.
*Do not try to remove `CREATE_NEW_CONSOLE` or `--no-one-instance`! Removing them brings back the invisible window and immediate-close bugs.*

### 2. The CloudFront HLS Proxy
**The Issue:** MovieBox streams use signed CloudFront cookies. VLC natively cannot send custom `Cookie` headers via its CLI. Browsers block cross-origin requests to CloudFront.
**The Solution:**
- The `/api/proxy` endpoint intercepts `.m3u8` files.
- It parses the playlist, finds all relative or absolute `.ts` chunk URLs, and rewrites them to also pass through `/api/proxy`.
- It dynamically injects the `Cookie`, `Referer`, and `User-Agent` headers into the backend `reqwest` client, tricking CloudFront into allowing the download.
- For VLC playback, we utilize the TUI's battle-tested `moviebox_tui::proxy::spawn_sidecar`, which spins up a dedicated lightweight proxy on a random port specifically for VLC.

### 3. DASH (.mpd) vs HLS (.m3u8)
**The Issue:** The web proxy (`/api/proxy`) was originally only built to parse and rewrite `.m3u8` playlists. If it receives an `.mpd` DASH manifest, it passes it through as raw bytes. 
**The Solution:** For the web browser, DASH requires something like `dash.js`. Currently, the frontend relies on `hls.js`, so it prefers HLS streams. For VLC, the `spawn_sidecar` proxy perfectly handles both HLS and DASH.

## 📝 Guidelines for Future Agents
- **Frontend Changes:** `index.html` is loaded dynamically via `std::fs::read_to_string` in `server.rs`. You do not need to recompile the Rust server to test frontend HTML/JS/CSS changes. Just refresh the browser!
- **CSS:** Use Tailwind classes.
- **State Management:** Keep Vue data clean. Ensure you handle `loadingStreams`, `loadingDetails`, and `loadingSearch` separately to trigger the correct Skeleton loaders.
- **Do not break the headers JSON encoding:** When passing headers from the stream API to the `playInBrowser` or `playLocal` methods, they must be correctly JSON stringified and URL-encoded.

Good luck!
