# agents.md — tune (terminal music player)

## Build & Run

- **Build**: `cargo build --release`
- **Run**: `cargo run --release` (debug mode works but audio may stutter)
- **Test**: `cargo test` — 6 tests in `src/lyrics.rs` only; run before shipping any lyrics changes
- **No `rustfmt.toml` or `clippy.toml`** — use default Rust formatting; `cargo clippy` not CI-enforced
- **Linux prereq**: `libasound2-dev` + `pkg-config` (see `.github/workflows/build.yml`)
- **Rust edition**: 2024 — requires very recent rustc; do not downgrade to 2021

## Architecture

### Module boundaries (readable entrypoint order)

```
main.rs   — tokio main loop, event dispatch, auto-next logic, background download spawn
ui.rs     — App struct (state), AppEvent enum, handle_key_event, draw() + all render fns (~1225 lines)
api.rs    — MusicServer trait + ServerConfig + ServerFeatures, MusicEntry struct, FileTransferServer adapter, create_server_pool() factory
player.rs — Player struct wrapping rodio Sink, seeking, volume, PlayMode enum, ShuffleState
lyrics.rs — LRC parser, USLT/Vorbis extraction via lofty, lines_at() for sync display
store.rs  — Config + Playlist persistence to tune_config.json / tune_playlists.json
navidrome.rs — Subsonic API adapter (SubsonicServer) — wired into create_server_pool() factory
local.rs  — Local directory adapter (LocalServer) — scans local folder, reads files directly
log.rs    — Simple file-based logger (appends to tune.log); log_error! macro for error tracking
```

### Key data flow

1. `main()` loads configs (`Vec<ServerConfig>`) → `create_server_pool(&configs)` → `App::new(player, server, configs, ...)`
2. `refresh_music_list()` spawns async task → `ServerPool::fetch_list()` calls each server, merges results, sets `server_id` → sends `MainMessage::MusicListLoaded` via mpsc channel
3. `start_playback_inner()` spawns download task → sends `MainMessage::AudioDownloaded(MusicEntry, Vec<u8>)` → `player.play_bytes(data, track)`
4. Loop: render → drain channel messages → auto-next check → poll keyboard events (100ms timeout)

## Configuration persistence

- **`tune_config.json`** — `[{ "name": "...", "server_url": "...", "server_type": "file-transfer", "username": "", "password": "" }, ...]` (array, was single object in v1)
- **`tune_playlists.json`** — `[{ "name": "...", "songs": [...] }]`
- Both are read/written synchronously (`std::fs`) — no async needed
- `.gitignore` lists `listen_config.json` and `listen_playlists.json` (old names) — do not reintroduce
- Config is lazily created; empty config list triggers config overlay on startup
- Old single-object `tune_config.json` is auto-migrated to array format on read

## Error logging

- **`tune.log`** — appends timestamped ERROR lines; errors from `ServerPool::fetch_list` and per-album fetch failures go here
- Use `crate::log_error!("msg {}", err)` to log; writes to `tune.log` in the working directory
- No external logging dependency; pure std implementation via `OnceLock<Mutex<File>>`

## Server adapter pattern

```rust
#[async_trait]
pub trait MusicServer: Send + Sync {
    fn name(&self) -> &str;
    fn base_url(&self) -> &str;
    fn features(&self) -> ServerFeatures;
    async fn fetch_list(&self) -> Result<Vec<MusicEntry>>;
    fn stream_url(&self, music: &MusicEntry) -> String;  // takes MusicEntry, not &str
    async fn search(&self, _query: &str) -> Result<Vec<MusicEntry>>;
    fn cover_url(&self, _music: &MusicEntry) -> Option<String>;
    async fn fetch_lyrics(&self, _music: &MusicEntry) -> Option<Lyrics>;
}
```

- `ServerPool` wraps `Vec<(String, Arc<dyn MusicServer>)>` and dispatches by `server_id` (which is `ServerConfig.name`); `create_server_pool(configs)` registers all adapters
- Multi-server: `fetch_list()` and `search()` fan out to all servers, tag results with `server_id`, merge; `stream_url()`/`cover_url()`/`fetch_lyrics()` route by `MusicEntry.server_id`; one failing server doesn't block others
- All HTTP calls use a shared `reqwest::Client` with 15-second timeout (stored in `crate::api::HTTP`)
- `ServerConfig` has `name`, `server_url`, `server_type`, `username`, `password` — shared across all adapters
- `ServerFeatures` advertises optional capabilities: `search`, `cover_art`
- `FileTransferServer` is wired; key endpoints: `GET /musicsV2` (list) and `GET /file?path=` (stream)
- `navidrome.rs` has a complete `SubsonicServer` — recursive album→song fetch, search3, getCoverArt, getLyrics
- `MusicEntry.absolute_path` is serde-renamed `absoultePath` (typo from the upstream API — preserve it)

## Gotchas

- **`MusicServer::name()` and `base_url()` trigger dead_code warnings** — they are only used via `Arc<dyn ...>` interface, never called directly. Can `#[allow(dead_code)]` on the impl or #[expect] in Rust 2024.
- **Auto-next guard**: `app.downloading` flag prevents re-firing `is_finished()` checks. If a track ends while download is in-flight, the auto-next is silently skipped (expected). Removing this guard causes GoToPlaying to be off by N.
- **Seeking FLAC**: rodio decoder doesn't support `.try_seek()` for FLAC → falls back to `skip_duration()` which re-decodes from start. Seek may be slow on long FLAC tracks.
- **Volume quantization**: `set_volume()` rounds to nearest 5% via `(vol * 20.0).round() / 20.0` to prevent floating-point drift.
- **Search scope**: only available in `ViewMode::Browse` — not in playlist views. `handle_key_event()` returns `None` for `/` in non-Browse modes.
- **`download_progress` is never populated** — the `Option<String>` field exists but nothing writes to it. Status bar shows "⏳ 下载中..." without percentage.
- **Shuffle reshuffle**: when queue is exhausted, `advance_shuffle()` calls `ShuffleState::new(count)` which replaces the queue (not inserting into existing). This means the same song could play twice before the queue is fully drained.
- **`PlayingSource` tracks both source and index** — used for auto-next cursor management. Initialized in `start_playback()` based on current `view_mode`.
- **Config overlay**: press `R` (shift-R) — same key as refresh (`r` lower-case). Easy to miss. Now supports 3 fields (URL/用户名/密码) with Tab/Shift-Tab to switch focus. Password field shows asterisks when not focused.
- **Playlist picker**: when `a` is pressed and >1 playlist exists, `picking_playlist=true` is set but `None` is returned from `handle_key_event`, meaning no event reaches `handle_app_action`. The actual dispatch happens on subsequent Enter in the picker overlay handler.

## Cargo.toml notes

- Edition 2024
- No `[dev-dependencies]` section
- Only `lofty` for lyrics extraction — no separate LRC library
- `rodio = "0.19"` with symphonia backend (default)
- `reqwest` with `json` + `stream` features
- `ratatui = "0.28"` with `crossterm` feature
- `crossterm = "0.28"` — must match ratatui's crossterm version

## CI

- GitHub Actions: builds on ubuntu/windows/macos for pushes to main and PRs
- Release workflow: when tag `v*` is pushed, creates GitHub Release with cross-platform binaries
- Artifacts named `tune-${{ runner.os }}` with `target/release/tune` (or `tune.exe`)
