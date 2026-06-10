# agents.md — tune (terminal music player)

## Build & Run

- **Build**: `cargo build --release`
- **Run**: `cargo run --release` (debug mode works but audio may stutter)
- **Test**: `cargo test` — 21 tests (6 lyrics + 15 PlayQueue); run before shipping any queue/lyrics changes
- **No `rustfmt.toml` or `clippy.toml`** — use default Rust formatting; `cargo clippy` not CI-enforced
- **Linux prereq**: `libasound2-dev` + `pkg-config` (see `.github/workflows/build.yml`)
- **Rust edition**: 2024 — requires very recent rustc; do not downgrade to 2021

## Architecture

### Module boundaries (readable entrypoint order)

```
main.rs        — tokio main loop, event dispatch, auto-next logic
message.rs     — MainMessage enum (inter-task protocol)
download.rs    — download_http_stream (progressive), AudioCache (disk cache)
server/
├── mod.rs     — MusicServer trait + ServerConfig + ServerFeatures, MusicEntry, ServerPool, create_server_pool() factory
├── navidrome.rs  — Subsonic API adapter (SubsonicServer)
├── local.rs      — Local directory adapter (LocalServer)
└── file_transfer.rs — 文件闪传 adapter (FileTransferServer)
ui/
├── mod.rs     — App struct (state), AppEvent enum, navigation/playlist helpers
├── input.rs   — handle_key_event (all keyboard input)
└── render.rs  — draw() + all render functions + format_duration
player.rs      — Player (rodio Sink), PlayMode, ShuffleState, PlayQueue, SharedAudioBuf, StreamingCursor
lyrics.rs      — LRC parser, USLT/Vorbis extraction via lofty, lines_at() for sync display
store.rs       — Config + Playlist + Language persistence to unified tune_config.json
log.rs         — Simple file-based logger (appends to tune.log); log_error! macro for error tracking
i18n.rs        — Chinese / English translation module with t!() and tf!() macros
```

### Key data flow

1. `main()` loads config (`TuneConfig`) → `create_server_pool(&config.servers)` → `App::new(player, server, config.servers)`
2. `refresh_music_list()` spawns async task → `ServerPool::fetch_list()` calls each server, merges results, sets `server_id` → sends `MainMessage::MusicListLoaded` via mpsc channel
3. `start_playback_inner()`:
   - Cache hit → `tokio::fs::read` → `AudioDownloaded` → `play_bytes`
   - Cache miss (HTTP) → `download_http_stream` → `StreamReady` → `play_streaming` (progressive) → `AudioDownloaded` → `finalize_streaming`
   - Local file → `fetch_audio` → `AudioDownloaded` → `play_bytes`
4. Loop: render → drain channel messages → auto-next check → poll keyboard events (100ms timeout)

## Configuration persistence

- **`tune_config.json`** — unified single file:
  ```json
  { "language": "zh", "servers": [...], "playlists": [...] }
  ```
- Old `tune_playlists.json` and `tune_lang.json` are auto-merged and deleted on first read
- Old single-object and array-only config formats are auto-migrated
- Read/written synchronously (`std::fs`) — no async needed
- Config is lazily created; empty server list triggers config overlay on startup

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
    fn stream_url(&self, music: &MusicEntry) -> String;
    async fn search(&self, _query: &str) -> Result<Vec<MusicEntry>>;
    fn cover_url(&self, _music: &MusicEntry) -> Option<String>;
    async fn fetch_lyrics(&self, _music: &MusicEntry) -> Option<Lyrics>;
    async fn fetch_audio(&self, music: &MusicEntry) -> Result<Vec<u8>>;
}
```

- `ServerPool` wraps `Vec<(String, bool, Arc<dyn MusicServer>)>` with `disabled` support
- Multi-server: `fetch_list()` and `search()` fan out to all non-disabled servers, tag results with `server_id`, merge; one failing server doesn't block others
- All HTTP calls use a shared `reqwest::Client` with 15-second timeout (`crate::server::HTTP`)
- `ServerConfig` has `name`, `server_url`, `server_type`, `username`, `password`, `disabled`
- `FileTransferServer`: `GET /musicsV2` (list) and `GET /file?path=` (stream)
- `SubsonicServer`: recursive album→song fetch, search3, getCoverArt, getLyrics
- `LocalServer`: directory scan with `lofty` duration probing, `tokio::fs::read` for audio
- `MusicEntry.absolute_path` is serde-renamed `absoultePath` (typo from the upstream API — preserve it)

## Known gotchas

- **`MusicServer::name()` and `base_url()` trigger dead_code warnings** — they are only used via `Arc<dyn ...>` interface, never called directly.
- **Auto-next guard**: `app.downloading` flag prevents re-firing `is_finished()` checks. If a track ends while download is in-flight, the auto-next is silently skipped (expected).
- **Seeking FLAC**: rodio decoder doesn't support `.try_seek()` for FLAC → falls back to `skip_duration()` which re-decodes from start. Seek may be slow on long FLAC tracks.
- **Seeking during streaming**: if `seek_to_ms` is called while progressive streaming is still in progress, it blocks until the download completes before re-decoding.
- **Volume quantization**: `set_volume()` rounds to nearest 5% via `(vol * 20.0).round() / 20.0`.
- **Volume formatting**: the `tf!()` macro uses `replacen("{}", ...)` — translation strings must use `{}` not `{:.0}` or other Rust format specifiers.
- **Search scope**: only available in `ViewMode::Browse` — not in playlist views.
- **Progressive streaming threshold**: 256 KB — playback starts after this much data is buffered in `SharedAudioBuf`. The decoder blocks on `read()` via `Condvar` when the buffer is empty (safe because rodio runs on its own thread).
- **Config overlay**: press `R` (shift-R) — same key as refresh (`r` lower-case). Now supports 5 fields with Tab/Shift-Tab to switch focus. Password field shows asterisks when not focused.
- **Playlist picker**: when `a` is pressed and >1 playlist exists, the picker overlay intercepts key events. Actual dispatch happens on subsequent Enter.
- **Quit confirmation**: press `q` or `Esc` in browse view shows a confirmation overlay — `y` to quit, `n`/`Esc` to cancel.
- **Language toggle**: press `L` (shift-L) to cycle between Chinese and English. Persisted to `tune_config.json`.
- **Play queue**: `x` to play next (insert at front), `w` to add to end, `u` to view/manage. Auto-next checks queue before normal order. Enter in queue overlay plays the selected queued song immediately.
- **Audio disk cache**: `<temp>/tune-cache/`, max 2 GB, files older than 30 days purged on startup. Keyed by hash of `(server_id, absolute_path)`.

## Cargo.toml notes

- Edition 2024
- No `[dev-dependencies]` section
- `rodio = "0.19"` with symphonia backend (default)
- `reqwest` with `json` + `stream` features
- `ratatui = "0.28"` with `crossterm` feature
- `crossterm = "0.28"` — must match ratatui's crossterm version
- `futures-util` for `bytes_stream()` iteration
- `chrono` for log timestamps

## CI

- GitHub Actions: builds on ubuntu/windows/macos, only on tag pushes (`v*`)
- Release workflow: when tag `v*` is pushed, creates GitHub Release with cross-platform binaries
- Artifacts named `tune-${{ runner.os }}` with `target/release/tune` (or `tune.exe`)
