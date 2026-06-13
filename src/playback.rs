// ── Playback orchestration ──
//
// Handles: starting playback, auto-advance, shuffle/sequential logic,
// lyrics fetching, and music list refresh.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::message::MainMessage;
use crate::player::{PlayMode, SharedAudioBuf, ShuffleState, TrackInfo};
use crate::server::{MusicEntry, MusicServer};
use crate::ui::{App, PlayingSource, ViewMode};

// ── Auto-next dispatch ──

pub(crate) fn handle_auto_next(app: &mut App, tx: &mpsc::Sender<MainMessage>) {
    // 1. Queue takes priority — consume queued songs first.
    if !app.player.queue.is_empty() {
        let music = app.player.queue.pop_front().expect("queue was empty after is_empty check");
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
                app.status_message = crate::tf!("status.single_repeat");
            }
        }
        PlayMode::Sequential => {
            if let Some(music) = advance_sequential(app) {
                start_playback_entry(app, tx.clone(), music);
            } else {
                app.player.stop();
                app.status_message = crate::tf!("status.playlist_end");
            }
        }
        PlayMode::Shuffle => {
            if let Some(music) = advance_shuffle(app) {
                start_playback_entry(app, tx.clone(), music);
            } else {
                app.player.stop();
                app.status_message = crate::tf!("status.shuffle_end");
            }
        }
    }
}

/// Return the next song in the current list without moving the cursor.
pub(crate) fn advance_sequential(app: &mut App) -> Option<MusicEntry> {
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
            if let Some(pl) = app.playlists.get(pl_idx) {
                if app.search_query.is_empty() {
                    // No search: simple sequential on full list
                    let next = song_idx + 1;
                    if next < pl.songs.len() {
                        app.playing_source =
                            Some(PlayingSource::PlaylistContent(pl_idx, next));
                        Some(pl.songs[next].clone())
                    } else {
                        None
                    }
                } else {
                    // Search active: follow filtered order.
                    // song_idx is the real pl.songs index. Find current song's
                    // position in the filtered list, advance in filtered order,
                    // then resolve back to real index.
                    let filtered = app.current_playlist_songs(pl_idx);
                    let current = &pl.songs[song_idx];
                    filtered
                        .iter()
                        .position(|m| m.absolute_path == current.absolute_path)
                        .and_then(|pos_in_filtered| {
                            let next_filtered = pos_in_filtered + 1;
                            if next_filtered < filtered.len() {
                                let next_song = &filtered[next_filtered];
                                pl.songs
                                    .iter()
                                    .position(|s| s.absolute_path == next_song.absolute_path)
                                    .map(|real_idx| {
                                        app.playing_source = Some(
                                            PlayingSource::PlaylistContent(pl_idx, real_idx),
                                        );
                                        next_song.clone()
                                    })
                            } else {
                                None
                            }
                        })
                }
            } else {
                None
            }
        }
        None => None,
    }
}

/// Return a shuffled next song without moving the cursor.
pub(crate) fn advance_shuffle(app: &mut App) -> Option<MusicEntry> {
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
        let exclude: Option<usize> = app.playing_source.and_then(|ps| match ps {
            PlayingSource::Browse(i) => Some(i),
            PlayingSource::PlaylistContent(pl_idx, si) => {
                if app.search_query.is_empty() {
                    Some(si)
                } else {
                    // Exclude should index into the filtered list
                    app.playlists.get(pl_idx).and_then(|pl| {
                        pl.songs.get(si).and_then(|current| {
                            let filtered = app.current_playlist_songs(pl_idx);
                            filtered.iter().position(|m| m.absolute_path == current.absolute_path)
                        })
                    })
                }
            }
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
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => {
            if app.search_query.is_empty() {
                app.playlists
                    .get(pl_idx)
                    .map(|pl| pl.songs.len())
                    .unwrap_or(0)
            } else {
                app.current_playlist_songs(pl_idx).len()
            }
        }
        None => 0,
    }
}

fn resolve_song_by_index(app: &App, idx: usize) -> Option<MusicEntry> {
    match app.playing_source {
        Some(PlayingSource::Browse(_)) => app.filtered_music.get(idx).cloned(),
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => {
            if app.search_query.is_empty() {
                app.playlists
                    .get(pl_idx)
                    .and_then(|pl| pl.songs.get(idx).cloned())
            } else {
                app.current_playlist_songs(pl_idx).get(idx).cloned()
            }
        }
        None => None,
    }
}

fn update_playing_source_index(app: &mut App, idx: usize) {
    match app.playing_source {
        Some(PlayingSource::Browse(_)) => {
            app.playing_source = Some(PlayingSource::Browse(idx));
        }
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => {
            if app.search_query.is_empty() {
                app.playing_source =
                    Some(PlayingSource::PlaylistContent(pl_idx, idx));
            } else {
                // idx is a filtered-list index; resolve to real pl.songs index
                let filtered = app.current_playlist_songs(pl_idx);
                if let Some(song) = filtered.get(idx)
                    && let Some(real_idx) = app.playlists.get(pl_idx)
                        .and_then(|pl| pl.songs.iter().position(|s| s.absolute_path == song.absolute_path))
                    {
                        app.playing_source = Some(PlayingSource::PlaylistContent(pl_idx, real_idx));
                    }
            }
        }
        None => {}
    }
}

/// Called after a track starts playing – initialises shuffle if needed.
pub(crate) fn on_play_started(app: &mut App) {
    if app.player.play_mode == PlayMode::Shuffle && app.player.shuffle_state.is_none() {
        let count = match app.playing_source {
            Some(PlayingSource::Browse(_)) => app.filtered_music.len(),
            Some(PlayingSource::PlaylistContent(pl_idx, _)) => {
                if app.search_query.is_empty() {
                    app.playlists
                        .get(pl_idx)
                        .map(|pl| pl.songs.len())
                        .unwrap_or(0)
                } else {
                    app.current_playlist_songs(pl_idx).len()
                }
            }
            None => 0,
        };
        if count > 1 {
            app.player
                .shuffle_state
                .get_or_insert_with(|| ShuffleState::new(count, None));
        }
    }
}

// ── Playback download ──

/// Start playing the song at the current cursor position.
pub(crate) fn start_playback(app: &mut App, tx: mpsc::Sender<MainMessage>) {
    let (music, _src_label) = match app.selected_in_current_view() {
        Some(m) => m,
        None => return,
    };
    // Set playing_source for auto-next tracking.
    // When search is active, pl_content_state.selected() gives the
    // filtered-list index, but auto-next uses the full pl.songs list.
    // We must resolve to the real index in pl.songs.
    match app.view_mode {
        ViewMode::Browse => {
            // list_state.selected() is a display index; resolve to filtered_music index
            app.playing_source = app
                .selected_display_song_index()
                .map(PlayingSource::Browse);
        }
        ViewMode::PlaylistContent(pl_idx) => {
            if app.search_query.is_empty() {
                app.playing_source = app
                    .pl_content_state
                    .selected()
                    .map(|si| PlayingSource::PlaylistContent(pl_idx, si));
            } else {
                // Search active: find the real index in the full playlist
                app.playing_source = app.selected_in_current_view().and_then(|(music, _)| {
                    app.playlists
                        .get(pl_idx)
                        .and_then(|pl| {
                            pl.songs
                                .iter()
                                .position(|s| s.absolute_path == music.absolute_path)
                        })
                        .map(|real_idx| PlayingSource::PlaylistContent(pl_idx, real_idx))
                });
            }
        }
        _ => {}
    }
    start_playback_inner(app, tx, music);
}

/// Spawn a background task that fetches server lyrics (with online fallback)
/// and sends `LyricsReady` back to the main loop.
/// Used so audio playback isn't blocked by lyrics network requests.
/// Takes individual fields rather than a full `MusicEntry` to avoid unnecessary clones.
pub(crate) fn spawn_lyrics_fetch(
    server: Arc<dyn MusicServer>,
    music_name: String,
    music_artist: String,
    music_duration: u64,
    tx: mpsc::Sender<MainMessage>,
) {
    tokio::spawn(async move {
        // Build a minimal MusicEntry for the server trait call (only name/artist needed)
        let entry = MusicEntry {
            name: music_name,
            artist: music_artist,
            duration: music_duration,
            absolute_path: String::new(),
            album: String::new(),
            server_id: String::new(),
        };
        let server_lyrics = server.fetch_lyrics(&entry).await;
        let lyrics = if server_lyrics.is_some() {
            server_lyrics
        } else {
            crate::lyrics_online::search(&entry.name, &entry.artist, entry.duration).await
        };
        let _ = tx.send(MainMessage::LyricsReady(lyrics)).await;
    });
}

/// Start playing an explicit song (used by auto-next — doesn't touch cursor).
pub(crate) fn start_playback_entry(app: &mut App, tx: mpsc::Sender<MainMessage>, music: MusicEntry) {
    start_playback_inner(app, tx, music);
}

pub(crate) fn start_playback_inner(app: &mut App, tx: mpsc::Sender<MainMessage>, music: MusicEntry) {
    if app.downloading {
        return;
    }

    // If it's the same track already playing, just re-seek (single-repeat case)
    if let Some(cur) = app.player.current_track()
        && cur.title == music.name && cur.artist == music.artist
            && app.player.has_audio_data() {
                let _ = app.player.seek_to_ms(0);
                return;
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
            app.status_message = crate::tf!("status.cache_reading", &music.name);
            let music_name = music.name.clone();
            let music_artist = music.artist.clone();
            let music_duration = music.duration;
            let tx_for_lyrics = tx_clone.clone();
            let server_for_lyrics = server.clone();
            let handle = tokio::spawn(async move {
                match tokio::fs::read(cache.path_for(&music.server_id, &music.absolute_path)).await
                {
                    Ok(data) => {
                        // Send audio immediately — don't block on lyrics network calls
                        let _ = tx_clone
                            .send(MainMessage::AudioDownloaded(music, data, None))
                            .await;
                        // Fetch lyrics in background
                        spawn_lyrics_fetch(server_for_lyrics, music_name, music_artist, music_duration, tx_for_lyrics);
                    }
                    Err(e) => {
                        // Corrupted cache entry — delete so next play retries
                        let _ = std::fs::remove_file(cache.path_for(&music.server_id, &music.absolute_path));
                        let _ = tx_clone
                            .send(MainMessage::AudioDownloadFailed(crate::tf!(
                                "status.cache_read_fail", e
                            )))
                            .await;
                    }
                }
            });
            app.track_background_task(handle.abort_handle());
            return;
        }

        // ── Cache miss: progressive streaming with cache-on-completion ──
        app.downloading = true;
            app.status_message = crate::tf!("status.downloading", &music.name);
        let buf = SharedAudioBuf::new();
        let buf_for_download = buf.clone();

        let track = TrackInfo {
            title: music.name.clone(),
            artist: music.artist.clone(),
            absolute_path: music.absolute_path.clone(),
            total_duration_ms: music.duration,
            lyrics: None, // will be set via finalize_streaming()
        };

        let music_name = music.name.clone();
        let music_artist = music.artist.clone();
        let music_duration = music.duration;
        let server_for_lyrics = server.clone();
        let tx_for_lyrics = tx_clone.clone();
        let handle = tokio::spawn(async move {
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
                    spawn_lyrics_fetch(server_for_lyrics, music_name, music_artist, music_duration, tx_for_lyrics);
                }
                Err(e) => {
                    let _ = tx_clone
                        .send(MainMessage::AudioDownloadFailed(crate::tf!("status.download_fail", e)))
                        .await;
                }
            }
        });
        app.track_background_task(handle.abort_handle());
    } else {
        // ── Non-HTTP fallback (local files) ──
        app.downloading = true;
        app.status_message = crate::tf!("status.loading", &music.name);
        let music_name = music.name.clone();
        let music_artist = music.artist.clone();
        let music_duration = music.duration;
        let tx_for_lyrics = tx_clone.clone();
        let server_for_lyrics = server.clone();
        let handle = tokio::spawn(async move {
            match server.fetch_audio(&music).await {
                Ok(data) => {
                    // Send audio immediately; lyrics arrive separately
                    let _ = tx_clone
                        .send(MainMessage::AudioDownloaded(music, data, None))
                        .await;
                    spawn_lyrics_fetch(server_for_lyrics, music_name, music_artist, music_duration, tx_for_lyrics);
                }
                Err(e) => {
                    let _ = tx_clone
                        .send(MainMessage::AudioDownloadFailed(crate::tf!("status.download_fail", e)))
                        .await;
                }
            }
        });
        app.track_background_task(handle.abort_handle());
    }
}

/// Spawn a background task to re-fetch the music list.
pub(crate) fn refresh_music_list(app: &mut App, tx: &mpsc::Sender<MainMessage>) {
    let server = app.server.clone();
    let tx = tx.clone();
    let handle = tokio::spawn(async move {
        match server.fetch_list().await {
            Ok(list) => {
                let _ = tx.send(MainMessage::MusicListLoaded(list)).await;
            }
            Err(e) => {
                    let _ = tx
                        .send(MainMessage::MusicListLoadFailed(crate::tf!(
                            "status.fetch_list_fail", e
                        )))
                        .await;
            }
        }
    });
    app.track_background_task(handle.abort_handle());
}
