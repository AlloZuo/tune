use crossterm::event::{KeyCode, KeyEventKind};

use super::{App, AppEvent, ViewMode};

impl App {
    pub fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Option<AppEvent> {
        if key.kind != KeyEventKind::Press {
            return None;
        }

        // ── Input modes (search / create-playlist) ──
        if self.creating_playlist {
            return match key.code {
                KeyCode::Enter => Some(AppEvent::ConfirmCreatePlaylist),
                KeyCode::Esc => {
                    self.creating_playlist = false;
                    self.new_playlist_name.clear();
                    Some(AppEvent::None)
                }
                KeyCode::Backspace => {
                    self.new_playlist_name.pop();
                    Some(AppEvent::None)
                }
                KeyCode::Char(c) => {
                    self.new_playlist_name.push(c);
                    Some(AppEvent::None)
                }
                _ => None,
            };
        }

        // ── Help overlay (any key dismisses) ──
        if self.show_help {
            self.show_help = false;
            return None;
        }

        // ── Config-server overlay ──
        if self.config_mode {
            return if self.config_phase == 0 {
                match key.code {
                    KeyCode::Up => {
                        if !self.config_servers.is_empty() && self.config_focus > 0 {
                            self.config_focus -= 1;
                        }
                        None
                    }
                    KeyCode::Down => {
                        if !self.config_servers.is_empty()
                            && self.config_focus + 1 < self.config_servers.len()
                        {
                            self.config_focus += 1;
                        }
                        None
                    }
                    KeyCode::Enter => {
                        if self.config_servers.is_empty() {
                            self.config_servers.push(ServerConfig::default());
                        }
                        self.config_edit_idx = self.config_focus.min(self.config_servers.len().saturating_sub(1));
                        let cfg = &self.config_servers[self.config_edit_idx];
                        self.config_inputs = vec![
                            cfg.name.clone(),
                            cfg.server_type.clone(),
                            cfg.server_url.clone(),
                            cfg.username.clone(),
                            cfg.password.clone(),
                        ];
                        self.config_focus = 0;
                        self.config_phase = 1;
                        None
                    }
                    KeyCode::Char('a') => {
                        let idx = self.config_servers.len();
                        self.config_servers.push(ServerConfig::default());
                        self.config_focus = idx;
                        self.config_edit_idx = idx;
                        let cfg = &self.config_servers[idx];
                        self.config_inputs = vec![
                            cfg.name.clone(),
                            cfg.server_type.clone(),
                            cfg.server_url.clone(),
                            cfg.username.clone(),
                            cfg.password.clone(),
                        ];
                        self.config_focus = 0;
                        self.config_phase = 1;
                        None
                    }
                    KeyCode::Char('d') => {
                        if self.config_focus < self.config_servers.len() {
                            self.config_servers.remove(self.config_focus);
                            if self.config_focus >= self.config_servers.len() && !self.config_servers.is_empty() {
                                self.config_focus = self.config_servers.len() - 1;
                            }
                        }
                        None
                    }
                    KeyCode::Char(' ') => {
                        if self.config_focus < self.config_servers.len() {
                            let cfg = &mut self.config_servers[self.config_focus];
                            cfg.disabled = !cfg.disabled;
                        }
                        None
                    }
                    KeyCode::Esc => Some(AppEvent::ConfirmConfig),
                    _ => None,
                }
            } else {
                match key.code {
                    KeyCode::Enter => {
                        if self.config_inputs.len() >= 5 && self.config_edit_idx < self.config_servers.len() {
                            self.config_servers[self.config_edit_idx].name = self.config_inputs[0].clone();
                            self.config_servers[self.config_edit_idx].server_type = self.config_inputs[1].clone();
                            let url = self.config_inputs[2].trim().to_string();
                            self.config_servers[self.config_edit_idx].server_url = url;
                            self.config_servers[self.config_edit_idx].username = self.config_inputs[3].trim().to_string();
                            self.config_servers[self.config_edit_idx].password = self.config_inputs[4].clone();
                        }
                        self.config_inputs.clear();
                        self.config_focus = self.config_edit_idx;
                        self.config_phase = 0;
                        None
                    }
                    KeyCode::Esc => {
                        self.config_inputs.clear();
                        self.config_focus = self.config_edit_idx;
                        self.config_phase = 0;
                        None
                    }
                    KeyCode::Tab | KeyCode::Down => {
                        if !self.config_inputs.is_empty() {
                            self.config_focus = (self.config_focus + 1) % self.config_inputs.len();
                        }
                        None
                    }
                    KeyCode::Up => {
                        if !self.config_inputs.is_empty() {
                            self.config_focus = (self.config_focus + self.config_inputs.len() - 1) % self.config_inputs.len();
                        }
                        None
                    }
                    KeyCode::Left | KeyCode::Right => {
                        if self.config_focus == 1 && self.config_inputs.len() >= 5 {
                            let types = ["file-transfer", "navidrome", "local"];
                            let current = self.config_inputs[1].clone();
                            let idx = types.iter().position(|t| *t == current).unwrap_or(0);
                            let next = match key.code {
                                KeyCode::Right => (idx + 1) % types.len(),
                                _ => (idx + types.len() - 1) % types.len(),
                            };
                            self.config_inputs[1] = types[next].to_string();
                        }
                        None
                    }
                    KeyCode::Backspace => {
                        if self.config_focus != 1 && self.config_focus < self.config_inputs.len() {
                            self.config_inputs[self.config_focus].pop();
                        }
                        None
                    }
                    KeyCode::Char(c) => {
                        if self.config_focus != 1 && self.config_focus < self.config_inputs.len() {
                            self.config_inputs[self.config_focus].push(c);
                        }
                        None
                    }
                    _ => None,
                }
            };
        }

        // ── Pick-playlist overlay ──
        if self.picking_playlist {
            return match key.code {
                KeyCode::Enter => {
                    self.picking_playlist = false;
                    Some(AppEvent::AddToPlaylist)
                }
                KeyCode::Up => {
                    if self.pick_index > 0 {
                        self.pick_index -= 1;
                    }
                    None
                }
                KeyCode::Down => {
                    if self.pick_index + 1 < self.playlists.len() {
                        self.pick_index += 1;
                    }
                    None
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.picking_playlist = false;
                    Some(AppEvent::None)
                }
                _ => None,
            };
        }

        // ── Queue overlay ──
        if self.showing_queue {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('u') => {
                    self.showing_queue = false;
                    None
                }
                KeyCode::Up => {
                    if self.queue_selected > 0 {
                        self.queue_selected -= 1;
                    }
                    None
                }
                KeyCode::Down => {
                    if self.queue_selected + 1 < self.player.queue.len() {
                        self.queue_selected += 1;
                    }
                    None
                }
                KeyCode::Char('d') => {
                    if !self.player.queue.is_empty() {
                        self.player.queue.remove(self.queue_selected);
                        if self.queue_selected >= self.player.queue.len() && self.queue_selected > 0 {
                            self.queue_selected -= 1;
                        }
                    }
                    None
                }
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    self.player.queue.move_up(self.queue_selected);
                    if self.queue_selected > 0 {
                        self.queue_selected -= 1;
                    }
                    None
                }
                KeyCode::Char('-') | KeyCode::Char('_') => {
                    self.player.queue.move_down(self.queue_selected);
                    if self.queue_selected + 1 < self.player.queue.len() {
                        self.queue_selected += 1;
                    }
                    None
                }
                KeyCode::Enter => {
                    if self.player.queue.get(self.queue_selected).is_some() {
                        self.showing_queue = false;
                        Some(AppEvent::QueuePlaySelected)
                    } else {
                        None
                    }
                }
                _ => None,
            };
        }

        // ── Quit confirmation overlay ──
        if self.confirm_quit {
            return match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => Some(AppEvent::Quit),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.confirm_quit = false;
                    None
                }
                _ => None,
            };
        }

        if self.search_mode {
            return match key.code {
                KeyCode::Enter => Some(AppEvent::ConfirmSearch),
                KeyCode::Esc => Some(AppEvent::CancelSearch),
                KeyCode::Backspace => Some(AppEvent::DeleteSearchChar),
                KeyCode::Char(c) => Some(AppEvent::PushSearchChar(c)),
                _ => None,
            };
        }

        // ── Normal mode ──
        match key.code {
            // Global: first press shows confirmation
            KeyCode::Char('q') => {
                self.confirm_quit = true;
                None
            }
            KeyCode::Esc if self.view_mode == ViewMode::Browse => {
                self.confirm_quit = true;
                None
            }
            KeyCode::Esc => match self.view_mode {
                ViewMode::Browse => Some(AppEvent::Quit),
                ViewMode::PlaylistList => {
                    self.view_mode = ViewMode::Browse;
                    Some(AppEvent::None)
                }
                ViewMode::PlaylistContent(_) => {
                    self.view_mode = ViewMode::PlaylistList;
                    Some(AppEvent::None)
                }
            },

            // Navigation
            KeyCode::Up => Some(AppEvent::MoveUp),
            KeyCode::Down => Some(AppEvent::MoveDown),
            KeyCode::PageUp => Some(AppEvent::ScrollUp),
            KeyCode::PageDown => Some(AppEvent::ScrollDown),

            // Playback control
            KeyCode::Enter => match self.view_mode {
                ViewMode::Browse | ViewMode::PlaylistContent(_) => {
                    Some(AppEvent::PlaySelected)
                }
                ViewMode::PlaylistList => {
                    let idx = self.pl_list_state.selected().unwrap_or(0);
                    if idx < self.playlists.len() {
                        self.view_mode = ViewMode::PlaylistContent(idx);
                        self.pl_content_state.select(Some(0));
                    }
                    Some(AppEvent::None)
                }
            },
            KeyCode::Char(' ') => Some(AppEvent::TogglePlayback),
            KeyCode::Char('s') => Some(AppEvent::Stop),
            KeyCode::Right => Some(AppEvent::SeekForward),
            KeyCode::Left => Some(AppEvent::SeekBackward),

            // Play mode
            KeyCode::Char('m') => Some(AppEvent::CyclePlayMode),

            // Volume
            KeyCode::Char('+') | KeyCode::Char('=') => Some(AppEvent::VolumeUp),
            KeyCode::Char('-') | KeyCode::Char('_') => Some(AppEvent::VolumeDown),

            // Search (Browse + PlaylistContent)
            KeyCode::Char('/') => {
                if matches!(self.view_mode, ViewMode::Browse | ViewMode::PlaylistContent(_)) {
                    Some(AppEvent::EnterSearch)
                } else {
                    None
                }
            }

            // Refresh / Config / GoToPlaying / Sort
            KeyCode::Char('r') => Some(AppEvent::Refresh),
            KeyCode::Char('R') => Some(AppEvent::ConfigureServer),
            KeyCode::Char('g') => Some(AppEvent::GoToPlaying),
            KeyCode::Char('S') => Some(AppEvent::CycleSort),
            KeyCode::Char('f') if self.view_mode == ViewMode::Browse => {
                Some(AppEvent::FilterByArtist)
            }
            KeyCode::Char('F') if self.view_mode == ViewMode::Browse => {
                Some(AppEvent::ClearArtistFilter)
            }

            // Playlists
            KeyCode::Char('l') => {
                match self.view_mode {
                    ViewMode::Browse => {
                        self.view_mode = ViewMode::PlaylistList;
                        self.pl_list_state.select(Some(0));
                    }
                    ViewMode::PlaylistList => {
                        self.view_mode = ViewMode::Browse;
                    }
                    ViewMode::PlaylistContent(_) => {
                        self.view_mode = ViewMode::PlaylistList;
                    }
                }
                Some(AppEvent::None)
            }
            KeyCode::Char('c') => {
                if matches!(self.view_mode, ViewMode::PlaylistList) {
                    Some(AppEvent::CreatePlaylist)
                } else {
                    None
                }
            }
            KeyCode::Char('a') => {
                if matches!(self.view_mode, ViewMode::Browse) && self.selected_music().is_some()
                {
                    if self.playlists.is_empty() {
                        Some(AppEvent::AddToPlaylist)
                    } else if self.playlists.len() == 1 {
                        self.pick_index = 0;
                        Some(AppEvent::AddToPlaylist)
                    } else {
                        self.picking_playlist = true;
                        self.pick_index = 0;
                        None
                    }
                } else {
                    None
                }
            }
            KeyCode::Char('d') => {
                match self.view_mode {
                    ViewMode::PlaylistList => Some(AppEvent::DeleteItem),
                    ViewMode::PlaylistContent(_) => Some(AppEvent::DeleteItem),
                    _ => None,
                }
            }

            // Queue
            KeyCode::Char('x') => {
                if matches!(self.view_mode, ViewMode::Browse | ViewMode::PlaylistContent(_))
                    && self.selected_in_current_view().is_some()
                {
                    Some(AppEvent::PlayNext)
                } else {
                    None
                }
            }
            KeyCode::Char('w') => {
                if matches!(self.view_mode, ViewMode::Browse | ViewMode::PlaylistContent(_))
                    && self.selected_in_current_view().is_some()
                {
                    Some(AppEvent::AddToQueue)
                } else {
                    None
                }
            }
            KeyCode::Char('u') => {
                if self.player.queue.is_empty() {
                    self.status_message = crate::tf!("queue.empty");
                    Some(AppEvent::None)
                } else {
                    Some(AppEvent::ToggleQueue)
                }
            }

            // Next track
            KeyCode::Char('n') => Some(AppEvent::NextTrack),

            KeyCode::Char('h' | '?') => Some(AppEvent::ShowHelp),
            KeyCode::Char('L') => Some(AppEvent::ToggleLanguage),
            _ => None,
        }
    }
}

use crate::server::ServerConfig;
