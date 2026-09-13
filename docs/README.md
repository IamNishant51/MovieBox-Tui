<div align="center">

# Introduction

**Terminal interface to find, download, and stream movies, TV shows, and live TV using local media players.**

[ English ](../README.md) • [ বাংলা ](../README_BN.md) • [ हिन्दी ](../README_HI.md) • [ Español ](../README_ES.md)

[![Telegram](https://telegram-badge.vercel.app/api/telegram-badge?channelId=@getfromme&style=flat&logo=true)](https://t.me/getfromme)
[![Donate](https://img.shields.io/badge/Donate-Crypto-F7931A?style=flat&logo=bitcoin&logoColor=white)](../README.md#optional-support)
</div>

[moviebox-tui-walkthrough.webm](https://github.com/user-attachments/assets/7554a7e5-6ff5-49ec-9d87-f821ea99950e)

## Features

- **On Demand Streaming**: Stream movies, series, and anime across multiple providers and community Stremio addons.
- **Live TV and IPTV**: Import custom M3U playlist URLs to search channels, browse categories, and stream live television.
- **Native Video Playback**: Plays directly in your favorite player (`mpv`, `IINA`, `VLC`, or Android video players) with smooth hardware acceleration.
- **Automatic Subtitles**: Automatically searches and loads subtitles in your preferred language into your player.
- **Fast Downloads**: Save single episodes or entire seasons to your computer with pause and resume support.
- **Visual Posters**: Displays cover art and movie posters directly inside your terminal window.
- **Library and History**: Bookmark your favorite titles and pick up watching right where you left off.
- **Custom Themes**: Built in color themes and settings to match your personal terminal look and feel.
- **Cross Platform**: Works identically on macOS, Linux, Windows, and Android.

## Prerequisites

Requires at least one media player for streaming:

- **mpv** (recommended across Linux, macOS, and Windows)
- **IINA** (macOS)
- **VLC** (cross platform)
- **Any Android Video Player** via Termux (VLC, Just Player, MX Player)

*Poster graphics:* Image rendering requires a graphics capable terminal (Ghostty, Kitty, WezTerm, or iTerm2). Standard terminals display clean text layouts automatically.

*Optional for MovieBox downloads:* `yt-dlp` and `ffmpeg` are required only for downloading DASH streams from the MovieBox provider. All other providers download directly with the built in engine.

---

## Documentation Directory Map

### Getting Started

| Guide | Description |
| :--- | :--- |
| [Installation](installation.md) | Platform installation instructions, package managers, and binary verification |
| [Keyboard & Controls](controls.md) | Complete keybindings, vim navigation, text editing, and slash commands |
| [Configuration Guide](config.md) | `config.json` schema, settings hub options, and environment variables |

### Features & Modes

| Guide | Description |
| :--- | :--- |
| [Content Providers](providers.md) | Built-in providers, scrapers, stream extractors, and authentication headers |
| [Hardware Players](players.md) | Media player detection, launch flags, stream headers, and watch tracking |
| [Batch Downloads](downloads.md) | Multi-segment download engine, range resume, and folder layout |
| [Stremio Addons](addons-mode.md) | Addon manifest installation, catalog browsing, and stream resolution |
| [Live TV & IPTV](tv-mode.md) | M3U playlist manager, channel parsing, and live stream playback |

### Architecture & Internals

| Guide | Description |
| :--- | :--- |
| [System Architecture](architecture.md) | Subsystem diagrams, async event loop, and task cancellation |
| [Module Breakdown](modules.md) | Crate structure, module responsibilities, and call boundaries |
| [Caching Strategy](cache.md) | Binary disk caching, TTL policies, and LRU memory management |
| [Logging System](logging.md) | File logging, log rotation, and tracing diagnostics |
| [Cross-Platform Operations](cross-platform.md) | Platform compatibility matrix across macOS, Linux, Windows, and Termux |

### Reference & Maintenance

| Guide | Description |
| :--- | :--- |
| [Testing Suite](testing.md) | Unit tests, integration tests, and verification gates |
| [Debugging Guide](debugging.md) | Troubleshooting common issues, terminal rendering, and player errors |
| [Release Checklist](release-checklist.md) | Pre-release validation, binary packaging, and deployment workflow |
| [Known Issues](known-issues.md) | Tracked limitations, terminal quirks, and workarounds |
| [Contributing Guide](contributing.md) | Contribution guidelines, code standards, and PR process |
| [Changelog](changelog.md) | Complete release history and unreleased changes |
