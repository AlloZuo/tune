mod download;
mod i18n;
mod log;
mod lyrics;
mod lyrics_online;
mod message;
mod player;
mod server;
mod store;
mod ui;

use std::io::stdout;
use std::sync::Arc;
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

use server::{create_server_pool, MusicEntry, MusicServer};
use player::{
    PlayMode, Player, PlayerState, SharedAudioBuf, ShuffleState, TrackInfo,
};
use store::{load_playlists, load_servers, save_playlists, save_servers};
use ui::{draw, format_duration, App, AppEvent, PlayingSource, ViewMode};

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
        refresh_music_list(&app, &msg_tx);
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
                    // Restore last playback position, if any
                    if app.playing_source.is_none() {
                        if let Some(last) = store::load_last_played() {
                            let music = app.all_music.iter().find(|m| {
                                m.server_id == last.server_id && m.absolute_path == last.absolute_path
                            }).cloned();
                            if let Some(music) = music {
                                // Set playing_source so GoToPlaying ("g") works
                                if let Some(idx) = app.all_music.iter().position(|m| m.absolute_path == music.absolute_path) {
                                    app.playing_source = Some(PlayingSource::Browse(idx));
                                }
                                app.resume_position_ms = if last.position_ms > 0 {
                                    app.status_message = tf!("status.resuming", &music.name);
                                    Some(last.position_ms)
                                } else {
                                    None
                                };
                                start_playback_entry(&mut app, msg_tx.clone(), music);
                                let _ = store::clear_last_played();
                            }
                        }
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
        // Guard: don't re-fire while a download is in progress, otherwise
        // is_finished() stays true and we'd advance playing_source multiple
        // times (the cause of GoToPlaying being off by N).
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
                            break 'main;
                        }

                        // If a playback action was triggered, yield so the
                        // background download can make progress before the
                        // next render.
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

// ── Auto-next dispatch ──

fn handle_auto_next(app: &mut App, tx: &mpsc::Sender<MainMessage>) {
    // 1. Queue takes priority — consume queued songs first.
    if !app.player.queue.is_empty() {
        let music = app.player.queue.pop_front().unwrap();
        // Try to find the queued song in the current view so playing_source
        // stays correct for GoToPlaying ("g") and fallback auto-next.
        let queued_path = &music.absolute_path;
        match app.playing_source {
            Some(PlayingSource::Browse(_)) => {
                if let Some(idx) = app.filtered_music.iter().position(|m| m.absolute_path == *queued_path) {
                    app.playing_source = Some(PlayingSource::Browse(idx));
                } else {
                    app.playing_source = None;
                }
            }
            Some(PlayingSource::PlaylistContent(pl_idx, _)) => {
                if let Some(pl) = app.playlists.get(pl_idx) {
                    if let Some(si) = pl.songs.iter().position(|s| s.absolute_path == *queued_path) {
                        app.playing_source = Some(PlayingSource::PlaylistContent(pl_idx, si));
                    } else {
                        app.playing_source = None;
                    }
                } else {
                    app.playing_source = None;
                }
            }
            None => {}
        }
        start_playback_entry(app, tx.clone(), music);
        return;
    }

    // 2. Fall back to play mode logic.
    match app.player.play_mode {
        PlayMode::SingleRepeat => {
            if app.player.has_audio_data() {
                let _ = app.player.seek_to_ms(0);
                app.status_message = tf!("status.single_repeat");
            }
        }
        PlayMode::Sequential => {
            if let Some(music) = advance_sequential(app) {
                start_playback_entry(app, tx.clone(), music);
            } else {
                app.player.stop();
                app.status_message = tf!("status.playlist_end");
            }
        }
        PlayMode::Shuffle => {
            if let Some(music) = advance_shuffle(app) {
                start_playback_entry(app, tx.clone(), music);
            } else {
                app.player.stop();
                app.status_message = tf!("status.shuffle_end");
            }
        }
    }
}

/// Return the next song in the current list without moving the cursor.
fn advance_sequential(app: &mut App) -> Option<MusicEntry> {
    match app.playing_source {
        Some(PlayingSource::Browse(idx)) => {
            let next = idx + 1;
            if next < app.filtered_music.len() {
                app.playing_source = Some(PlayingSource::Browse(next));
                Some(app.filtered_music[next].clone())
            } else {
                None
            }
        }
        Some(PlayingSource::PlaylistContent(pl_idx, song_idx)) => {
            let next = song_idx + 1;
            if let Some(pl) = app.playlists.get(pl_idx) {
                if next < pl.songs.len() {
                    app.playing_source =
                        Some(PlayingSource::PlaylistContent(pl_idx, next));
                    Some(pl.songs[next].clone())
                } else {
                    None
                }
            } else {
                None
            }
        }
        None => None,
    }
}

/// Return a shuffled next song without moving the cursor.
fn advance_shuffle(app: &mut App) -> Option<MusicEntry> {
    // Try to pop from the existing queue
    if let Some(idx) = app.player.next_shuffle_index() {
        let music = resolve_song_by_index(app, idx);
        if music.is_some() {
            update_playing_source_index(app, idx);
        }
        return music;
    }
    // Queue exhausted → reshuffle.
    // Pass the last-played index as `exclude` so the new permutation's
    // first-popped element is guaranteed to be a different song.
    let count = current_list_len(app);
    if count > 1 {
        let exclude = app.playing_source.map(|ps| match ps {
            PlayingSource::Browse(i) => i,
            PlayingSource::PlaylistContent(_, si) => si,
        });
        app.player.shuffle_state = Some(ShuffleState::new(count, exclude));
        if let Some(idx) = app.player.next_shuffle_index() {
            let music = resolve_song_by_index(app, idx);
            if music.is_some() {
                update_playing_source_index(app, idx);
            }
            return music;
        }
    }
    None
}

fn current_list_len(app: &App) -> usize {
    match app.playing_source {
        Some(PlayingSource::Browse(_)) => app.filtered_music.len(),
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => app
            .playlists
            .get(pl_idx)
            .map(|pl| pl.songs.len())
            .unwrap_or(0),
        None => 0,
    }
}

fn resolve_song_by_index(app: &App, idx: usize) -> Option<MusicEntry> {
    match app.playing_source {
        Some(PlayingSource::Browse(_)) => app.filtered_music.get(idx).cloned(),
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => app
            .playlists
            .get(pl_idx)
            .and_then(|pl| pl.songs.get(idx).cloned()),
        None => None,
    }
}

fn update_playing_source_index(app: &mut App, idx: usize) {
    match app.playing_source {
        Some(PlayingSource::Browse(_)) => {
            app.playing_source = Some(PlayingSource::Browse(idx));
        }
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => {
            app.playing_source =
                Some(PlayingSource::PlaylistContent(pl_idx, idx));
        }
        None => {}
    }
}

/// Called after a track starts playing – initialises shuffle if needed.
fn on_play_started(app: &mut App) {
    if app.player.play_mode == PlayMode::Shuffle && app.player.shuffle_state.is_none() {
        let count = match app.playing_source {
            Some(PlayingSource::Browse(_)) => app.filtered_music.len(),
            Some(PlayingSource::PlaylistContent(pl_idx, _)) => app
                .playlists
                .get(pl_idx)
                .map(|pl| pl.songs.len())
                .unwrap_or(0),
            None => 0,
        };
        if count > 1 {
            app.player
                .shuffle_state
                .get_or_insert_with(|| ShuffleState::new(count, None));
        }
    }
}

// ── Event dispatch ──

/// Returns `true` if the action may have started a background download
/// (so the caller should yield to let tokio poll it).
fn handle_app_action(action: AppEvent, app: &mut App, tx: &mpsc::Sender<MainMessage>) -> bool {
    match action {
        // ── Playback ──
        AppEvent::PlaySelected => {
            start_playback(app, tx.clone());
            true
        }
        AppEvent::TogglePlayback => {
            app.player.toggle_playback();
            match app.player.state() {
                PlayerState::Playing => app.status_message = tf!("status.resumed"),
                PlayerState::Paused => app.status_message = tf!("status.paused"),
                _ => {}
            }
            app.error_message = None;
            false
        }
        AppEvent::Stop => {
            app.player.stop();
            app.playing_source = None;
            app.status_message = tf!("status.stopped");
            false
        }
        AppEvent::SeekForward => {
            // Spawn background decode so the UI stays responsive
            if let Some((data, pos_ms)) = app.player.extract_seek_data(5) {
                let tx = tx.clone();
                tokio::spawn(async move {
                    match crate::player::decode_seek_source(data, pos_ms).await {
                        Ok(source) => {
                            let _ = tx
                                .send(MainMessage::SeekPrepared(source, pos_ms, true))
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(MainMessage::SeekFailed(format!("seek forward failed: {}", e)))
                                .await;
                        }
                    }
                });
            } else {
                app.status_message = tf!("status.seek_unavailable");
            }
            false
        }
        AppEvent::SeekBackward => {
            if let Some((data, pos_ms)) = app.player.extract_seek_data(-5) {
                let tx = tx.clone();
                tokio::spawn(async move {
                    match crate::player::decode_seek_source(data, pos_ms).await {
                        Ok(source) => {
                            let _ = tx
                                .send(MainMessage::SeekPrepared(source, pos_ms, false))
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(MainMessage::SeekFailed(format!("seek backward failed: {}", e)))
                                .await;
                        }
                    }
                });
            } else {
                app.status_message = tf!("status.seek_unavailable");
            }
            false
        }
        AppEvent::CyclePlayMode => {
            app.player.cycle_play_mode();
            app.status_message = tf!("status.play_mode", app.player.play_mode.label());
            false
        }

        // ── Volume ──
        AppEvent::VolumeUp => {
            app.player.adjust_volume(0.05);
            app.status_message = tf!("status.volume", (app.player.volume() * 100.0).round() as u8);
            false
        }
        AppEvent::VolumeDown => {
            app.player.adjust_volume(-0.05);
            app.status_message = tf!("status.volume", (app.player.volume() * 100.0).round() as u8);
            false
        }

        // ── Navigation ──
        AppEvent::MoveUp => {
            app.navigate_up();
            false
        }
        AppEvent::MoveDown => {
            app.navigate_down();
            false
        }
        AppEvent::ScrollUp => {
            app.scroll_up();
            false
        }
        AppEvent::ScrollDown => {
            app.scroll_down();
            false
        }

        // ── Go to playing ──
        AppEvent::GoToPlaying => {
            if app.player.is_stopped() {
                app.status_message = tf!("app.no_playing");
                return false;
            }
            match app.playing_source {
                Some(PlayingSource::Browse(idx)) => {
                    app.view_mode = ViewMode::Browse;
                    if idx < app.filtered_music.len() {
                        app.list_state.select(Some(idx));
                        app.status_message =
                            tf!("status.jumped_to", &app.filtered_music[idx].name);
                    }
                }
                Some(PlayingSource::PlaylistContent(pl_idx, song_idx)) => {
                    app.view_mode = ViewMode::PlaylistContent(pl_idx);
                    if let Some(pl) = app.playlists.get(pl_idx) {
                        if song_idx < pl.songs.len() {
                            app.pl_content_state.select(Some(song_idx));
                            app.status_message = tf!("status.jumped_to_playlist", &pl.name, &pl.songs[song_idx].name);
                        }
                    }
                }
                None => {
                    app.status_message = tf!("app.unknown_source");
                }
            }
            false
        }

        // ── Search ──
        AppEvent::EnterSearch => {
            app.search_mode = true;
            false
        }
        AppEvent::ConfirmSearch => {
            app.search_mode = false;
            app.apply_filter();
            let count = match app.view_mode {
                ViewMode::PlaylistContent(pl_idx) => app.current_playlist_songs(pl_idx).len(),
                _ => app.filtered_music.len(),
            };
            app.status_message = tf!("status.search_done", count);
            false
        }
        AppEvent::CancelSearch => {
            app.search_mode = false;
            app.search_query.clear();
            app.apply_filter();
            false
        }
        AppEvent::DeleteSearchChar => {
            app.search_query.pop();
            app.apply_filter();
            false
        }
        AppEvent::PushSearchChar(c) => {
            app.search_query.push(c);
            app.apply_filter();
            false
        }

        // ── Playlists ──
        AppEvent::CreatePlaylist => {
            app.creating_playlist = true;
            app.new_playlist_name.clear();
            false
        }
        AppEvent::ConfirmCreatePlaylist => {
            let name = app.new_playlist_name.clone();
            app.create_playlist(name);
            app.creating_playlist = false;
            app.new_playlist_name.clear();
            let name = app.playlists.last().map(|p| p.name.as_str()).unwrap_or("");
            app.status_message = tf!("playlist.created", name);
            let _ = save_playlists(&app.playlists);
            false
        }
        AppEvent::AddToPlaylist => {
            if let Some((music, _src)) = app.selected_in_current_view() {
                if app.playlists.is_empty() {
                    app.create_playlist(tf!("playlist.default_name", 1));
                }
                let idx = app.pick_index.min(app.playlists.len().saturating_sub(1));
                let name = app.playlists[idx].name.clone();
                app.add_to_playlist(idx, music);
                app.status_message = tf!("playlist.added", &name);
                let _ = save_playlists(&app.playlists);
            }
            false
        }
        AppEvent::DeleteItem => {
            match app.view_mode {
                ViewMode::PlaylistList => {
                    let idx = app.pl_list_state.selected().unwrap_or(0);
                    if app.delete_playlist(idx) {
                        app.status_message = tf!("playlist.deleted");
                        let _ = save_playlists(&app.playlists);
                    }
                }
                ViewMode::PlaylistContent(_) => {
                    let idx = app.pl_content_state.selected().unwrap_or(0);
                    if app.remove_song_from_current_playlist(idx) {
                        app.status_message = tf!("playlist.removed");
                        let _ = save_playlists(&app.playlists);
                    }
                }
                _ => {}
            }
            false
        }

        AppEvent::Quit => false,
        // ── Help ──
        AppEvent::ShowHelp => {
            app.show_help = true;
            false
        }

        // ── Refresh ──
        AppEvent::Refresh => {
            if app.server_configs.is_empty() || app.server_configs.iter().all(|c| c.server_url.is_empty()) {
                app.status_message = tf!("app.no_server");
                false
            } else {
                refresh_music_list(app, tx);
                app.status_message = tf!("status.refreshing");
                true
            }
        }
        AppEvent::ConfigureServer => {
            app.config_mode = true;
            app.config_phase = 0; // list
            app.config_focus = 0;
            app.config_edit_idx = 0;
            app.config_inputs.clear();
            // Work on a snapshot of configs
            app.config_servers = app.server_configs.clone();
            false
        }
        AppEvent::ConfirmConfig => {
            // Save all configs and rebuild server pool
            app.config_mode = false;
            app.config_phase = 0;
            app.config_inputs.clear();
            app.server_configs = std::mem::take(&mut app.config_servers);
            app.server = create_server_pool(&app.server_configs);
            let _ = save_servers(&app.server_configs);
            let count = app.server_configs.len();
            app.status_message = tf!("status.servers_saved", count);
            // Re-fetch with new config
            if app.server_configs.iter().any(|c| !c.server_url.is_empty()) {
                refresh_music_list(app, tx);
                return true;
            }
            false
        }
        // ── Play queue ──
        AppEvent::PlayNext => {
            let (music, _src_label) = match app.selected_in_current_view() {
                Some(m) => m,
                None => return false,
            };
            app.player.queue.push_front(music);
            app.status_message = tf!("queue.added_front", app.player.queue.len());
            false
        }
        AppEvent::AddToQueue => {
            let (music, _src_label) = match app.selected_in_current_view() {
                Some(m) => m,
                None => return false,
            };
            app.player.queue.push_back(music);
            app.status_message = tf!("queue.added_back", app.player.queue.len());
            false
        }
        AppEvent::ToggleQueue => {
            app.showing_queue = !app.showing_queue;
            if app.showing_queue {
                app.queue_selected = 0;
            }
            false
        }
        AppEvent::QueuePlaySelected => {
            let music = app.player.queue.remove(app.queue_selected);
            if let Some(music) = music {
                // Try to find in current view so playing_source stays accurate
                let path = &music.absolute_path;
                match app.playing_source {
                    Some(PlayingSource::Browse(_)) => {
                        if let Some(idx) = app.filtered_music.iter().position(|m| m.absolute_path == *path) {
                            app.playing_source = Some(PlayingSource::Browse(idx));
                        }
                    }
                    Some(PlayingSource::PlaylistContent(pl_idx, _)) => {
                        if let Some(pl) = app.playlists.get(pl_idx) {
                            if let Some(si) = pl.songs.iter().position(|s| s.absolute_path == *path) {
                                app.playing_source = Some(PlayingSource::PlaylistContent(pl_idx, si));
                            }
                        }
                    }
                    None => {}
                }
                start_playback_inner(app, tx.clone(), music);
                true
            } else {
                false
            }
        }
        AppEvent::CycleSort => {
            app.sort_mode = app.sort_mode.next();
            app.apply_sort();
            app.status_message = tf!("status.sort_changed", app.sort_mode.label());
            false
        }
        AppEvent::ToggleLanguage => {
            let new = crate::i18n::current().toggle();
            crate::i18n::init(new.as_str());
            let _ = store::save_language(new.as_str());
            app.status_message = tf!("status.lang_switched");
            false
        }
        AppEvent::None => false,
    }
}

// ── Playback download ──

/// Start playing the song at the current cursor position.
fn start_playback(app: &mut App, tx: mpsc::Sender<MainMessage>) {
    let (music, _src_label) = match app.selected_in_current_view() {
        Some(m) => m,
        None => return,
    };
    // Set playing_source for auto-next tracking
    match app.view_mode {
        ViewMode::Browse => {
            app.playing_source =
                app.list_state.selected().map(PlayingSource::Browse);
        }
        ViewMode::PlaylistContent(pl_idx) => {
            app.playing_source = app
                .pl_content_state
                .selected()
                .map(|si| PlayingSource::PlaylistContent(pl_idx, si));
        }
        _ => {}
    }
    start_playback_inner(app, tx, music);
}

/// Spawn a background task that fetches server lyrics (with online fallback)
/// and sends `LyricsReady` back to the main loop.
/// Used so audio playback isn't blocked by lyrics network requests.
fn spawn_lyrics_fetch(
    server: Arc<dyn MusicServer>,
    music: MusicEntry,
    tx: mpsc::Sender<MainMessage>,
) {
    tokio::spawn(async move {
        let server_lyrics = server.fetch_lyrics(&music).await;
        let lyrics = if server_lyrics.is_some() {
            server_lyrics
        } else {
            crate::lyrics_online::search(&music.name, &music.artist, music.duration).await
        };
        let _ = tx.send(MainMessage::LyricsReady(lyrics)).await;
    });
}

/// Start playing an explicit song (used by auto-next — doesn't touch cursor).
fn start_playback_entry(app: &mut App, tx: mpsc::Sender<MainMessage>, music: MusicEntry) {
    start_playback_inner(app, tx, music);
}

fn start_playback_inner(app: &mut App, tx: mpsc::Sender<MainMessage>, music: MusicEntry) {
    if app.downloading {
        return;
    }

    // If it's the same track already playing, just re-seek (single-repeat case)
    if let Some(cur) = app.player.current_track() {
        if cur.title == music.name && cur.artist == music.artist {
            if app.player.has_audio_data() {
                let _ = app.player.seek_to_ms(0);
                return;
            }
        }
    }

    app.error_message = None;
    app.download_progress = None;

    let stream_url = app.server.stream_url(&music);
    let server = app.server.clone();
    let tx_clone = tx.clone();

    if stream_url.starts_with("http") {
        let cache = crate::download::AudioCache::new();

        // ── Cache hit: read from disk ──
        if cache.has(&music.server_id, &music.absolute_path) {
            app.downloading = true;
            app.status_message = tf!("status.cache_reading", &music.name);
            let tx_for_lyrics = tx_clone.clone();
            let music_for_lyrics = music.clone();
            let server_for_lyrics = server.clone();
            tokio::spawn(async move {
                match tokio::fs::read(cache.path_for(&music.server_id, &music.absolute_path)).await
                {
                    Ok(data) => {
                        // Send audio immediately — don't block on lyrics network calls
                        let _ = tx_clone
                            .send(MainMessage::AudioDownloaded(music, data, None))
                            .await;
                        // Fetch lyrics in background
                        spawn_lyrics_fetch(server_for_lyrics, music_for_lyrics, tx_for_lyrics);
                    }
                    Err(e) => {
                        // Corrupted cache entry — delete so next play retries
                        let _ = std::fs::remove_file(cache.path_for(&music.server_id, &music.absolute_path));
                        let _ = tx_clone
                            .send(MainMessage::AudioDownloadFailed(tf!(
                                "status.cache_read_fail", e
                            )))
                            .await;
                    }
                }
            });
            return;
        }

        // ── Cache miss: progressive streaming with cache-on-completion ──
        app.downloading = true;
            app.status_message = tf!("status.downloading", &music.name);
        let buf = SharedAudioBuf::new();
        let buf_for_download = buf.clone();

        let track = TrackInfo {
            title: music.name.clone(),
            artist: music.artist.clone(),
            total_duration_ms: music.duration,
            lyrics: None, // will be set via finalize_streaming()
        };

        let music_for_lyrics = music.clone();
        let server_for_lyrics = server.clone();
        let tx_for_lyrics = tx_clone.clone();
        tokio::spawn(async move {
            let result =
                crate::download::download_http_stream(&stream_url, &tx_clone, buf_for_download, track).await;

            match result {
                Ok(data) => {
                    // Write to disk cache for next play
                    let cache = crate::download::AudioCache::new();
                    cache.init_async().await;
                    cache.put(&music.server_id, &music.absolute_path, &data).await;

                    // Send audio data immediately; lyrics arrive separately
                    let _ = tx_clone
                        .send(MainMessage::AudioDownloaded(music, data, None))
                        .await;
                    spawn_lyrics_fetch(server_for_lyrics, music_for_lyrics, tx_for_lyrics);
                }
                Err(e) => {
                    let _ = tx_clone
                        .send(MainMessage::AudioDownloadFailed(tf!("status.download_fail", e)))
                        .await;
                }
            }
        });
    } else {
        // ── Non-HTTP fallback (local files) ──
        app.downloading = true;
        app.status_message = tf!("status.loading", &music.name);
        let tx_for_lyrics = tx_clone.clone();
        let music_for_lyrics = music.clone();
        let server_for_lyrics = server.clone();
        tokio::spawn(async move {
            match server.fetch_audio(&music).await {
                Ok(data) => {
                    // Send audio immediately; lyrics arrive separately
                    let _ = tx_clone
                        .send(MainMessage::AudioDownloaded(music, data, None))
                        .await;
                    spawn_lyrics_fetch(server_for_lyrics, music_for_lyrics, tx_for_lyrics);
                }
                Err(e) => {
                    let _ = tx_clone
                        .send(MainMessage::AudioDownloadFailed(tf!("status.download_fail", e)))
                        .await;
                }
            }
        });
    }
}

/// Spawn a background task to re-fetch the music list.
fn refresh_music_list(app: &App, tx: &mpsc::Sender<MainMessage>) {
    let server = app.server.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        match server.fetch_list().await {
            Ok(list) => {
                let _ = tx.send(MainMessage::MusicListLoaded(list)).await;
            }
            Err(e) => {
                    let _ = tx
                        .send(MainMessage::MusicListLoadFailed(tf!(
                            "status.fetch_list_fail", e
                        )))
                        .await;
            }
        }
    });
}
