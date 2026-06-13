// ── Event dispatch ──
//
// Maps AppEvent to concrete actions: playback, navigation, playlist ops,
// search, config, queue, and system commands.

use crate::player::PlayerState;
use crate::server::create_server_pool;
use crate::store::{save_playlists, save_servers};
use crate::ui::{App, AppEvent, PlayingSource, ViewMode};

use crate::message::MainMessage;
use crate::playback::{start_playback, start_playback_inner, start_playback_entry, refresh_music_list, handle_auto_next, advance_sequential, advance_shuffle};

use tokio::sync::mpsc;

/// Returns `true` if the action may have started a background download
/// (so the caller should yield to let tokio poll it).
pub(crate) fn handle_app_action(action: AppEvent, app: &mut App, tx: &mpsc::Sender<MainMessage>) -> bool {
    match action {
        // ── Playback ──
        AppEvent::PlaySelected => {
            start_playback(app, tx.clone());
            true
        }
        AppEvent::TogglePlayback => {
            app.player.toggle_playback();
            match app.player.state() {
                PlayerState::Playing => app.status_message = crate::tf!("status.resumed"),
                PlayerState::Paused => app.status_message = crate::tf!("status.paused"),
                _ => {}
            }
            app.error_message = None;
            false
        }
        AppEvent::Stop => {
            app.player.stop();
            app.playing_source = None;
            app.status_message = crate::tf!("status.stopped");
            false
        }
        AppEvent::SeekForward => {
            // Spawn background decode so the UI stays responsive
            if let Some((data, pos_ms)) = app.player.extract_seek_data(5) {
                let tx = tx.clone();
                let handle = tokio::spawn(async move {
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
                app.track_background_task(handle.abort_handle());
            } else {
                app.status_message = crate::tf!("status.seek_unavailable");
            }
            false
        }
        AppEvent::SeekBackward => {
            if let Some((data, pos_ms)) = app.player.extract_seek_data(-5) {
                let tx = tx.clone();
                let handle = tokio::spawn(async move {
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
                app.track_background_task(handle.abort_handle());
            } else {
                app.status_message = crate::tf!("status.seek_unavailable");
            }
            false
        }
        AppEvent::CyclePlayMode => {
            app.player.cycle_play_mode();
            app.status_message = crate::tf!("status.play_mode", app.player.play_mode.label());
            false
        }

        // ── Volume ──
        AppEvent::VolumeUp => {
            app.player.adjust_volume(0.05);
            app.status_message = crate::tf!("status.volume", (app.player.volume() * 100.0).round() as u8);
            false
        }
        AppEvent::VolumeDown => {
            app.player.adjust_volume(-0.05);
            app.status_message = crate::tf!("status.volume", (app.player.volume() * 100.0).round() as u8);
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
            match app.playing_source {
                Some(PlayingSource::Browse(idx)) => {
                    app.view_mode = ViewMode::Browse;
                    if idx < app.filtered_music.len() {
                        // Map filtered_music index to display index
                        let di = app.find_display_index_for_song(idx).unwrap_or(idx);
                        app.list_state.select(Some(di));
                        app.status_message =
                            crate::tf!("status.jumped_to", &app.filtered_music[idx].name);
                    }
                }
                Some(PlayingSource::PlaylistContent(pl_idx, song_idx)) => {
                    app.view_mode = ViewMode::PlaylistContent(pl_idx);
                    if let Some(pl) = app.playlists.get(pl_idx)
                        && song_idx < pl.songs.len() {
                            app.pl_content_state.select(Some(song_idx));
                            app.status_message = crate::tf!("status.jumped_to_playlist", &pl.name, &pl.songs[song_idx].name);
                        }
                }
                None => {
                    app.status_message = crate::tf!("app.no_playing");
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
            app.status_message = crate::tf!("status.search_done", count);
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
            app.status_message = crate::tf!("playlist.created", name);
            let _ = save_playlists(&app.playlists);
            false
        }
        AppEvent::AddToPlaylist => {
            if let Some((music, _src)) = app.selected_in_current_view() {
                if app.playlists.is_empty() {
                    app.create_playlist(crate::tf!("playlist.default_name", 1));
                }
                let idx = app.pick_index.min(app.playlists.len().saturating_sub(1));
                let name = app.playlists[idx].name.clone();
                app.add_to_playlist(idx, music);
                app.status_message = crate::tf!("playlist.added", &name);
                let _ = save_playlists(&app.playlists);
            }
            false
        }
        AppEvent::DeleteItem => {
            match app.view_mode {
                ViewMode::PlaylistList => {
                    let idx = app.pl_list_state.selected().unwrap_or(0);
                    if app.delete_playlist(idx) {
                        app.status_message = crate::tf!("playlist.deleted");
                        let _ = save_playlists(&app.playlists);
                    }
                }
                ViewMode::PlaylistContent(_) => {
                    let idx = app.pl_content_state.selected().unwrap_or(0);
                    if app.remove_song_from_current_playlist(idx) {
                        app.status_message = crate::tf!("playlist.removed");
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
                app.status_message = crate::tf!("app.no_server");
                false
            } else {
                refresh_music_list(app, tx);
                app.status_message = crate::tf!("status.refreshing");
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
            app.status_message = crate::tf!("status.servers_saved", count);
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
            app.status_message = crate::tf!("queue.added_front", app.player.queue.len());
            false
        }
        AppEvent::AddToQueue => {
            let (music, _src_label) = match app.selected_in_current_view() {
                Some(m) => m,
                None => return false,
            };
            app.player.queue.push_back(music);
            app.status_message = crate::tf!("queue.added_back", app.player.queue.len());
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
                        if let Some(pl) = app.playlists.get(pl_idx)
                            && let Some(si) = pl.songs.iter().position(|s| s.absolute_path == *path) {
                                app.playing_source = Some(PlayingSource::PlaylistContent(pl_idx, si));
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
            app.rebuild_display();
            app.status_message = crate::tf!("status.sort_changed", app.sort_mode.label());
            false
        }
        AppEvent::FilterByArtist => {
            let artist = app.selected_music().map(|m| m.artist.clone());
            if let Some(a) = artist {
                if app.artist_filter.as_deref() == Some(a.as_str()) {
                    // Toggle off if same artist
                    app.artist_filter = None;
                } else {
                    app.artist_filter = Some(a);
                }
                app.apply_filter();
                app.status_message = match &app.artist_filter {
                    Some(a) => crate::tf!("status.filter_artist", a),
                    None => crate::tf!("status.filter_cleared"),
                };
            }
            false
        }
        AppEvent::ClearArtistFilter => {
            if app.artist_filter.is_some() {
                app.artist_filter = None;
                app.apply_filter();
                app.status_message = crate::tf!("status.filter_cleared");
            }
            false
        }
        AppEvent::ToggleLanguage => {
            let new = crate::i18n::current().toggle();
            crate::i18n::init(new.as_str());
            let _ = crate::store::save_language(new.as_str());
            app.status_message = crate::tf!("status.lang_switched");
            false
        }
        AppEvent::NextTrack => {
            // Manual skip: stop current playback, then advance to next track.
            // Don't clear playing_source — handle_auto_next needs it.
            app.player.stop();
            if !app.player.queue.is_empty() {
                handle_auto_next(app, tx);
            } else {
                match app.player.play_mode {
                    crate::player::PlayMode::SingleRepeat => {
                        // Skip the repeat — advance as sequential
                        if let Some(music) = advance_sequential(app) {
                            start_playback_entry(app, tx.clone(), music);
                        } else {
                            app.playing_source = None;
                            app.status_message = crate::tf!("status.playlist_end");
                        }
                    }
                    crate::player::PlayMode::Sequential => {
                        if let Some(music) = advance_sequential(app) {
                            start_playback_entry(app, tx.clone(), music);
                        } else {
                            app.playing_source = None;
                            app.status_message = crate::tf!("status.playlist_end");
                        }
                    }
                    crate::player::PlayMode::Shuffle => {
                        if let Some(music) = advance_shuffle(app) {
                            start_playback_entry(app, tx.clone(), music);
                        } else {
                            app.playing_source = None;
                            app.status_message = crate::tf!("status.shuffle_end");
                        }
                    }
                }
            }
            true
        }
        AppEvent::None => false,
    }
}