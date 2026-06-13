mod dispatch;
mod download;
mod i18n;
mod log;
mod lyrics;
mod lyrics_online;
mod message;
mod playback;
mod player;
mod server;
mod store;
mod ui;

use std::io::stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use message::MainMessage;
use lyrics::Lyrics;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use server::create_server_pool;
use player::{Player, PlayerState, TrackInfo};
use store::{load_playlists, load_servers};
use ui::{draw, format_duration, App, AppEvent, PlayingSource};

use dispatch::handle_app_action;
use playback::{handle_auto_next, refresh_music_list, on_play_started};

// ── Background messages ──
// (defined in download.rs)

#[tokio::main]
async fn main() -> Result<()> {
    // ── Terminal ──
    enable_raw_mode()?;
    let mut stdout = stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Initialise file logger (appends to tune.log in the working directory).
    log::init("tune.log");

    // Initialise i18n (load from config, default to Chinese).
    i18n::init(&store::load_language());

    let (msg_tx, mut msg_rx) = mpsc::channel::<MainMessage>(32);

    let player = Player::new()?;

    // ── Load configs & playlists ──
    let configs = load_servers();
    let server = create_server_pool(&configs);
    let mut app = App::new(player, server, configs);
    // Restore saved volume
    app.player.set_volume(store::load_volume());
    let saved_playlists = load_playlists();
    if !saved_playlists.is_empty() {
        app.playlists = saved_playlists;
    }

    // ── If no server configured, prompt on first screen ──
    if app.server_configs.is_empty() || app.server_configs.iter().all(|c| c.server_url.is_empty()) {
        app.config_mode = true;
        app.status_message = tf!("app.config_prompt");
    } else {
        refresh_music_list(&mut app, &msg_tx);
    }

    // ── Main loop ──
    'main: loop {
        // 1. Render
        let _ = terminal.draw(|f| draw(f, &mut app));

        // 2. Background messages
        while let Ok(msg) = msg_rx.try_recv() {
            match msg {
                MainMessage::MusicListLoaded(list) => {
                    app.set_music_list(list);
                    // Remember which song was last played (so "g" works) but
                    // don't auto-play — wait for the user to press Enter.
                    if app.playing_source.is_none()
                        && let Some(last) = store::load_last_played() {
                            if let Some(idx) = app.all_music.iter().position(|m| {
                                m.server_id == last.server_id && m.absolute_path == last.absolute_path
                            }) {
                                app.playing_source = Some(PlayingSource::Browse(idx));
                                app.status_message = tf!("status.last_played", &app.all_music[idx].name);
                            }
                            let _ = store::clear_last_played();
                        }
                }
                MainMessage::MusicListLoadFailed(err) => {
                    app.error_message = Some(err);
                    app.status_message = tf!("status.load_failed");
                }
                MainMessage::DownloadProgress(received, total) => {
                    app.download_progress = if total > 0 {
                        let pct = (received as f64 / total as f64 * 100.0) as u8;
                        Some(format!("{}%", pct))
                    } else {
                        let kb = received / 1024;
                        if kb > 1024 {
                            Some(format!("{:.1} MB", kb as f64 / 1024.0))
                        } else {
                            Some(format!("{} KB", kb))
                        }
                    };
                }
                MainMessage::StreamReady(buf, track) => {
                    app.download_progress = None;
                    match app.player.play_streaming(buf, track) {
                        Ok(()) => {
                            app.status_message =
                                app.player.current_track().map_or_else(
                                    || tf!("status.playing", ""),
                                    |t| tf!("status.playing", &t.title),
                                );
                            app.error_message = None;
                            on_play_started(&mut app);
                        }
                        Err(e) => {
                            app.downloading = false;
                            app.error_message =
                                Some(tf!("status.stream_fail", e));
                        }
                    }
                }
                MainMessage::AudioDownloaded(music, data, server_lyrics) => {
                    // Priority: 1) server lyrics that are timed (LRC), 2) embedded
                    // lyrics from audio tags (parsed in blocking pool), 3) server plain text.
                    let lyrics = match server_lyrics {
                        Some(Lyrics::Timed(_)) => server_lyrics, // server LRC → trust it
                        _ => {
                            // Parse ID3/Vorbis tags in blocking pool to avoid
                            // blocking the async runtime with lofty probing.
                            let data_for_lyrics = data.clone();
                            let embedded = tokio::task::spawn_blocking(move || {
                                lyrics::extract_lyrics(&data_for_lyrics)
                            })
                            .await
                            .unwrap_or(None);
                            embedded.or(server_lyrics)
                        }
                    };

                    if app.player.streaming_buf.is_some() {
                        // Progressive streaming: the player is already consuming
                        // from the shared buffer. Just finalize: store seek data
                        // and update lyrics.
                        app.player.finalize_streaming(data, lyrics);
                        app.downloading = false;
                        app.download_progress = None;
                        // Resume seek after streaming completes
                        if let Some(pos) = app.resume_position_ms.take() {
                            let _ = app.player.seek_to_ms_async(pos).await;
                        }
                    } else if app.player.state() == &PlayerState::Stopped
                        && app.playing_source.is_none()
                    {
                        // User pressed stop — this download was orphaned.
                        // Clean up without playing.
                        app.downloading = false;
                        app.download_progress = None;
                    } else {
                        // Non-streaming path (local files or small files).
                        app.downloading = false;
                        app.download_progress = None;

                        let track = TrackInfo {
                            title: music.name.clone(),
                            artist: music.artist.clone(),
                            absolute_path: music.absolute_path.clone(),
                            total_duration_ms: music.duration,
                            lyrics,
                        };
                        // Decode audio in blocking pool, then play
                        match app.player.play_bytes_async(data, track).await {
                            Ok(()) => {
                                app.status_message = tf!("status.playing", music.name);
                                app.error_message = None;
                                on_play_started(&mut app);
                                if let Some(pos) = app.resume_position_ms.take() {
                                    let _ = app.player.seek_to_ms_async(pos).await;
                                }
                            }
                            Err(e) => {
                                app.error_message = Some(tf!("status.decode_fail", e));
                            }
                        }
                    }
                }
                MainMessage::LyricsReady(lyrics) => {
                    app.player.set_lyrics(lyrics);
                }
                MainMessage::AudioDownloadFailed(err) => {
                    app.downloading = false;
                    app.download_progress = None;
                    app.error_message = Some(err);
                }
                MainMessage::SeekPrepared(source, pos_ms, is_forward) => {
                    app.player.apply_seek_source(source, pos_ms);
                    let pos_str = format_duration(pos_ms);
                    if is_forward {
                        app.status_message = tf!("status.seek_forward", pos_str);
                    } else {
                        app.status_message = tf!("status.seek_backward", pos_str);
                    }
                    app.error_message = None;
                }
                MainMessage::SeekFailed(err) => {
                    app.error_message = Some(err);
                }
            }
        }

        // 3. Auto-next based on play mode
        if !app.downloading
            && app.player.state() == &PlayerState::Playing
            && app.player.is_finished()
        {
            handle_auto_next(&mut app, &msg_tx);
        }

        // 4. Keyboard events
        if event::poll(Duration::from_millis(100)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(key)) => {
                    if let Some(action) = app.handle_key_event(key) {
                        let needs_yield = handle_app_action(action, &mut app, &msg_tx);

                        if action == AppEvent::Quit {
                            // Cancel all background tasks first
                            app.cancel_background_tasks();
                            // Save playback position before quitting
                            let music = match app.playing_source {
                                Some(PlayingSource::Browse(idx)) => app.filtered_music.get(idx),
                                Some(PlayingSource::PlaylistContent(pl_idx, si)) => {
                                    app.playlists.get(pl_idx).and_then(|pl| pl.songs.get(si))
                                }
                                None => None,
                            };
                            if let Some(m) = music {
                                let _ = store::save_last_played(&store::LastPlayed {
                                    server_id: m.server_id.clone(),
                                    absolute_path: m.absolute_path.clone(),
                                    position_ms: app.player.position_ms(),
                                });
                            }
                            let _ = store::save_volume(app.player.volume());
                            store::flush_config();
                            break 'main;
                        }

                        if needs_yield {
                            tokio::task::yield_now().await;
                        }
                    }
                }
                Ok(Event::Resize(_, _)) => {
                    let _ = terminal.draw(|f| draw(f, &mut app));
                }
                Err(_) => break,
                _ => {}
            }
        }
    }

    // ── Cleanup ──
    drop(terminal);
    let _ = std::io::stdout().execute(LeaveAlternateScreen);
    disable_raw_mode()?;
    println!("{}", tf!("app.quit"));
    Ok(())
}