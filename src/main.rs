mod api;
mod local;
mod log;
mod lyrics;
mod navidrome;
mod player;
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
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use api::{create_server_pool, MusicEntry};
use lyrics::Lyrics;
use player::{PlayMode, Player, PlayerState, ShuffleState, TrackInfo};
use store::{load_configs, load_playlists, save_configs, save_playlists};
use ui::{draw, App, AppEvent, PlayingSource, ViewMode};

// ── Background messages ──

enum MainMessage {
    MusicListLoaded(Vec<MusicEntry>),
    MusicListLoadFailed(String),
    AudioDownloaded(MusicEntry, Vec<u8>, Option<Lyrics>),
    AudioDownloadFailed(String),
}

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

    let (msg_tx, mut msg_rx) = mpsc::channel::<MainMessage>(32);

    let player = Player::new()?;

    // ── Load configs & playlists ──
    let configs = load_configs();
    let server = create_server_pool(&configs);
    let mut app = App::new(player, server, configs);
    let saved_playlists = load_playlists();
    if !saved_playlists.is_empty() {
        app.playlists = saved_playlists;
    }

    // ── If no server configured, prompt on first screen ──
    if app.server_configs.is_empty() || app.server_configs.iter().all(|c| c.server_url.is_empty()) {
        app.config_mode = true;
        app.status_message = "请先添加服务器 (按 Enter 编辑, Tab 切换字段)".to_string();
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
                }
                MainMessage::MusicListLoadFailed(err) => {
                    app.error_message = Some(err);
                    app.status_message = "加载失败".to_string();
                }
                MainMessage::AudioDownloaded(music, data, server_lyrics) => {
                    app.downloading = false;

                    // Priority: 1) server lyrics that are timed (LRC), 2) embedded
                    // lyrics from audio tags, 3) server plain text as last resort.
                    let lyrics = match server_lyrics {
                        Some(Lyrics::Timed(_)) => server_lyrics,  // server LRC → trust it
                        _ => lyrics::extract_lyrics(&data).or(server_lyrics), // or embedded, or server plain
                    };

                    let track = TrackInfo {
                        title: music.name.clone(),
                        artist: music.artist.clone(),
                        total_duration_ms: music.duration,
                        lyrics,
                    };
                    match app.player.play_bytes(data, track) {
                        Ok(()) => {
                            app.status_message = format!("▶ 正在播放: {}", music.name);
                            app.error_message = None;
                            on_play_started(&mut app);
                        }
                        Err(e) => {
                            app.error_message =
                                Some(format!("解码失败 (可能是不支持的格式): {}", e));
                        }
                    }
                }
                MainMessage::AudioDownloadFailed(err) => {
                    app.downloading = false;
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
    println!("已退出音源播放器");
    Ok(())
}

// ── Auto-next dispatch ──

fn handle_auto_next(app: &mut App, tx: &mpsc::Sender<MainMessage>) {
    match app.player.play_mode {
        PlayMode::SingleRepeat => {
            if app.player.has_audio_data() {
                let _ = app.player.seek_to_ms(0);
                app.status_message = "单曲循环".to_string();
            }
        }
        PlayMode::Sequential => {
            if let Some(music) = advance_sequential(app) {
                start_playback_entry(app, tx.clone(), music);
            } else {
                app.player.stop();
                app.status_message = "播放完毕".to_string();
            }
        }
        PlayMode::Shuffle => {
            if let Some(music) = advance_shuffle(app) {
                start_playback_entry(app, tx.clone(), music);
            } else {
                app.player.stop();
                app.status_message = "随机播放已播完".to_string();
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
    // Queue exhausted → reshuffle (replace, never insert, because the
    // existing state is already empty).
    let count = current_list_len(app);
    if count > 1 {
        app.player.shuffle_state = Some(ShuffleState::new(count));
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
                .get_or_insert_with(|| ShuffleState::new(count));
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
                PlayerState::Playing => app.status_message = "继续播放".to_string(),
                PlayerState::Paused => app.status_message = "已暂停".to_string(),
                _ => {}
            }
            app.error_message = None;
            false
        }
        AppEvent::Stop => {
            app.player.stop();
            app.status_message = "已停止".to_string();
            false
        }
        AppEvent::SeekForward => {
            match app.player.seek_relative(5) {
                Ok(()) => {
                    let pos = format_duration(app.player.position_ms());
                    app.status_message = format!("快进 → {}", pos);
                }
                Err(e) => app.error_message = Some(format!("快进失败: {}", e)),
            }
            false
        }
        AppEvent::SeekBackward => {
            match app.player.seek_relative(-5) {
                Ok(()) => {
                    let pos = format_duration(app.player.position_ms());
                    app.status_message = format!("快退 ← {}", pos);
                }
                Err(e) => app.error_message = Some(format!("快退失败: {}", e)),
            }
            false
        }
        AppEvent::CyclePlayMode => {
            app.player.cycle_play_mode();
            app.status_message = format!("播放模式: {}", app.player.play_mode.label());
            false
        }

        // ── Volume ──
        AppEvent::VolumeUp => {
            app.player.adjust_volume(0.05);
            app.status_message = format!("音量: {:.0}%", app.player.volume() * 100.0);
            false
        }
        AppEvent::VolumeDown => {
            app.player.adjust_volume(-0.05);
            app.status_message = format!("音量: {:.0}%", app.player.volume() * 100.0);
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
                app.status_message = "当前没有正在播放的歌曲".to_string();
                return false;
            }
            match app.playing_source {
                Some(PlayingSource::Browse(idx)) => {
                    app.view_mode = ViewMode::Browse;
                    if idx < app.filtered_music.len() {
                        app.list_state.select(Some(idx));
                        app.status_message =
                            format!("已跳转到: {}", app.filtered_music[idx].name);
                    }
                }
                Some(PlayingSource::PlaylistContent(pl_idx, song_idx)) => {
                    app.view_mode = ViewMode::PlaylistContent(pl_idx);
                    if let Some(pl) = app.playlists.get(pl_idx) {
                        if song_idx < pl.songs.len() {
                            app.pl_content_state.select(Some(song_idx));
                            app.status_message =
                                format!("已跳转到: {} — {}", pl.name, pl.songs[song_idx].name);
                        }
                    }
                }
                None => {
                    app.status_message = "播放来源未知".to_string();
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
            app.status_message = format!("搜索完成: {} 个结果", app.filtered_music.len());
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
            app.status_message = format!("已创建歌单: {}", name);
            let _ = save_playlists(&app.playlists);
            false
        }
        AppEvent::AddToPlaylist => {
            if let Some((music, _src)) = app.selected_in_current_view() {
                if app.playlists.is_empty() {
                    app.create_playlist("默认歌单".to_string());
                }
                let idx = app.pick_index.min(app.playlists.len().saturating_sub(1));
                let name = app.playlists[idx].name.clone();
                app.add_to_playlist(idx, music);
                app.status_message = format!("已添加到歌单「{}」", name);
                let _ = save_playlists(&app.playlists);
            }
            false
        }
        AppEvent::DeleteItem => {
            match app.view_mode {
                ViewMode::PlaylistList => {
                    let idx = app.pl_list_state.selected().unwrap_or(0);
                    if app.delete_playlist(idx) {
                        app.status_message = "已删除歌单".to_string();
                        let _ = save_playlists(&app.playlists);
                    }
                }
                ViewMode::PlaylistContent(_) => {
                    let idx = app.pl_content_state.selected().unwrap_or(0);
                    if app.remove_song_from_current_playlist(idx) {
                        app.status_message = "已从歌单移除".to_string();
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
                app.status_message = "请先按 R 配置服务器地址".to_string();
                false
            } else {
                refresh_music_list(app, tx);
                app.status_message = "正在刷新音乐列表...".to_string();
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
            let _ = save_configs(&app.server_configs);
            let count = app.server_configs.len();
            app.status_message = format!(
                "已保存 {} 个服务器配置",
                count
            );
            // Re-fetch with new config
            if app.server_configs.iter().any(|c| !c.server_url.is_empty()) {
                refresh_music_list(app, tx);
                return true;
            }
            false
        }
        AppEvent::None => {
            app.status_message =
                "按 h/? 查看帮助 | ↑/↓ 选择 | Enter 播放".to_string();
            false
        }
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

    app.downloading = true;
    app.status_message = format!("⏳ 正在加载: {}...", music.name);
    app.error_message = None;

    let server = app.server.clone();
    let tx = tx.clone();

    tokio::spawn(async move {
        match server.fetch_audio(&music).await {
            Ok(data) => {
                // Try server-side lyrics (e.g. Subsonic getLyrics API)
                let server_lyrics = server.fetch_lyrics(&music).await;
                let _ = tx
                    .send(MainMessage::AudioDownloaded(music, data, server_lyrics))
                    .await;
            }
            Err(e) => {
                let _ = tx
                    .send(MainMessage::AudioDownloadFailed(format!("加载失败: {}", e)))
                    .await;
            }
        }
    });
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
                    .send(MainMessage::MusicListLoadFailed(format!(
                        "获取音乐列表失败: {}",
                        e
                    )))
                    .await;
            }
        }
    });
}

fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}
