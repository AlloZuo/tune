use crossterm::event::{KeyCode, KeyEventKind};

use super::{App, AppEvent, ViewMode};
use crate::server::ServerConfig;
use crate::tf;

// ── Key dispatch ──

impl App {
    /// Route a key press to the handler for the current overlay or normal mode.
    pub fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Option<AppEvent> {
        if key.kind != KeyEventKind::Press {
            return None;
        }

        if self.creating_playlist {
            return self.handle_create_playlist_key(key.code);
        }
        if self.config_mode {
            return self.handle_config_key(key.code);
        }
        if self.showing_queue {
            return self.handle_queue_key(key.code);
        }
        if self.picking_playlist {
            return self.handle_pick_playlist_key(key.code);
        }

        self.handle_overlay_or_normal(key.code)
    }
}

// ── Overlay handlers ──

impl App {
    fn handle_create_playlist_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
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
        }
    }

    fn handle_config_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        if self.config_phase == 0 {
            self.handle_config_list_key(key)
        } else {
            self.handle_config_edit_key(key)
        }
    }

    fn handle_config_list_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
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
                self.config_edit_idx = self
                    .config_focus
                    .min(self.config_servers.len().saturating_sub(1));
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
    }

    fn handle_config_edit_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
            KeyCode::Enter => {
                if self.config_inputs.len() >= 5 && self.config_edit_idx < self.config_servers.len() {
                    self.config_servers[self.config_edit_idx].name = self.config_inputs[0].clone();
                    self.config_servers[self.config_edit_idx].server_type = self.config_inputs[1].clone();
                    self.config_servers[self.config_edit_idx].server_url =
                        self.config_inputs[2].trim().to_string();
                    self.config_servers[self.config_edit_idx].username =
                        self.config_inputs[3].trim().to_string();
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
                    self.config_focus =
                        (self.config_focus + self.config_inputs.len() - 1) % self.config_inputs.len();
                }
                None
            }
            KeyCode::Left | KeyCode::Right => {
                if self.config_focus == 1 && self.config_inputs.len() >= 5 {
                    let types = ["file-transfer", "navidrome", "local"];
                    let current = self.config_inputs[1].clone();
                    let idx = types.iter().position(|t| *t == current).unwrap_or(0);
                    let next = match key {
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
    }

    fn handle_queue_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
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
        }
    }

    fn handle_pick_playlist_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
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
        }
    }

    fn handle_overlay_or_normal(&mut self, key: KeyCode) -> Option<AppEvent> {
        // Each overlay has its own handler; normal mode falls through.
        if self.show_cover {
            return self.handle_cover_key(key);
        }
        if self.show_help {
            return self.handle_help_key(key);
        }
        if self.confirm_quit {
            return self.handle_quit_confirm_key(key);
        }
        if self.search_mode {
            return self.handle_search_key(key);
        }
        self.handle_normal_key(key)
    }

    // ── Overlay sub-handlers ──

    fn handle_cover_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
            // C again or Esc dismisses
            KeyCode::Char('C') | KeyCode::Esc => {
                self.show_cover = false;
                None
            }
            _ => None,
        }
    }

    fn handle_help_key(&mut self, _key: KeyCode) -> Option<AppEvent> {
        self.show_help = false;
        None
    }

    fn handle_quit_confirm_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
            KeyCode::Char('y' | 'Y') => Some(AppEvent::Quit),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                self.confirm_quit = false;
                None
            }
            _ => None,
        }
    }

    fn handle_search_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
            KeyCode::Enter => Some(AppEvent::ConfirmSearch),
            KeyCode::Esc => Some(AppEvent::CancelSearch),
            KeyCode::Backspace => Some(AppEvent::DeleteSearchChar),
            KeyCode::Char(c) => Some(AppEvent::PushSearchChar(c)),
            _ => None,
        }
    }

    // ── Normal mode ──

    fn handle_normal_key(&mut self, key: KeyCode) -> Option<AppEvent> {
        match key {
            // ── Global / system ──
            KeyCode::Char('q') | KeyCode::Esc if self.view_mode == ViewMode::Browse => {
                return self.handle_quit_request(key);
            }
            KeyCode::Esc => return self.handle_esc(key),

            // ── Navigation ──
            KeyCode::Up => Some(AppEvent::MoveUp),
            KeyCode::Down => Some(AppEvent::MoveDown),
            KeyCode::PageUp => Some(AppEvent::ScrollUp),
            KeyCode::PageDown => Some(AppEvent::ScrollDown),
            KeyCode::Char('g') => Some(AppEvent::GoToPlaying),

            // ── Search trigger ──
            KeyCode::Char('/')
                if matches!(self.view_mode, ViewMode::Browse | ViewMode::PlaylistContent(_)) =>
            {
                Some(AppEvent::EnterSearch)
            }

            // ── Playback ──
            KeyCode::Enter => Some(self.handle_enter_key()),
            KeyCode::Char(' ') => Some(AppEvent::TogglePlayback),
            KeyCode::Char('s') => Some(AppEvent::Stop),
            KeyCode::Right => Some(AppEvent::SeekForward),
            KeyCode::Left => Some(AppEvent::SeekBackward),
            KeyCode::Char('m') => Some(AppEvent::CyclePlayMode),
            KeyCode::Char('n') => Some(AppEvent::NextTrack),
            KeyCode::Char('+') | KeyCode::Char('=') => Some(AppEvent::VolumeUp),
            KeyCode::Char('-') | KeyCode::Char('_') => Some(AppEvent::VolumeDown),

            // ── Artist filter ──
            KeyCode::Char('f') if self.view_mode == ViewMode::Browse => {
                Some(AppEvent::FilterByArtist)
            }
            KeyCode::Char('F') if self.view_mode == ViewMode::Browse => {
                Some(AppEvent::ClearArtistFilter)
            }

            // ── View / sort ──
            KeyCode::Char('r') => Some(AppEvent::Refresh),
            KeyCode::Char('R') => Some(AppEvent::ConfigureServer),
            KeyCode::Char('S') => Some(AppEvent::CycleSort),
            KeyCode::Char('h' | '?') => Some(AppEvent::ShowHelp),
            KeyCode::Char('L') => Some(AppEvent::ToggleLanguage),
            KeyCode::Char('C') => Some(AppEvent::ToggleCover),

            // ── Playlist management ──
            KeyCode::Char('l') => Some(self.handle_playlist_toggle_key()),
            KeyCode::Char('c') if self.view_mode == ViewMode::PlaylistList => {
                Some(AppEvent::CreatePlaylist)
            }
            KeyCode::Char('a') if self.view_mode == ViewMode::Browse && self.selected_music().is_some() => {
                self.handle_add_to_playlist_key()
            }
            KeyCode::Char('d') => match self.view_mode {
                ViewMode::PlaylistList | ViewMode::PlaylistContent(_) => Some(AppEvent::DeleteItem),
                _ => None,
            },

            // ── Play queue (from normal mode) ──
            KeyCode::Char('x')
                if matches!(self.view_mode, ViewMode::Browse | ViewMode::PlaylistContent(_))
                    && self.selected_in_current_view().is_some() =>
            {
                Some(AppEvent::PlayNext)
            }
            KeyCode::Char('w')
                if matches!(self.view_mode, ViewMode::Browse | ViewMode::PlaylistContent(_))
                    && self.selected_in_current_view().is_some() =>
            {
                Some(AppEvent::AddToQueue)
            }
            KeyCode::Char('u') => {
                if self.player.queue.is_empty() {
                    self.status_message = tf!("queue.empty");
                    Some(AppEvent::None)
                } else {
                    Some(AppEvent::ToggleQueue)
                }
            }

            _ => None,
        }
    }

    // ── Normal-mode sub-handlers ──

    fn handle_quit_request(&mut self, _key: KeyCode) -> Option<AppEvent> {
        self.confirm_quit = true;
        None
    }

    fn handle_esc(&mut self, _key: KeyCode) -> Option<AppEvent> {
        match self.view_mode {
            ViewMode::Browse => Some(AppEvent::Quit),
            ViewMode::PlaylistList => {
                self.view_mode = ViewMode::Browse;
                Some(AppEvent::None)
            }
            ViewMode::PlaylistContent(_) => {
                self.view_mode = ViewMode::PlaylistList;
                Some(AppEvent::None)
            }
        }
    }

    fn handle_enter_key(&mut self) -> AppEvent {
        match self.view_mode {
            ViewMode::Browse | ViewMode::PlaylistContent(_) => AppEvent::PlaySelected,
            ViewMode::PlaylistList => {
                let idx = self.pl_list_state.selected().unwrap_or(0);
                if idx < self.playlists.len() {
                    self.view_mode = ViewMode::PlaylistContent(idx);
                    self.pl_content_state.select(Some(0));
                }
                AppEvent::None
            }
        }
    }

    fn handle_playlist_toggle_key(&mut self) -> AppEvent {
        match self.view_mode {
            ViewMode::Browse => {
                self.view_mode = ViewMode::PlaylistList;
                self.pl_list_state.select(Some(0));
            }
            ViewMode::PlaylistList => self.view_mode = ViewMode::Browse,
            ViewMode::PlaylistContent(_) => self.view_mode = ViewMode::PlaylistList,
        }
        AppEvent::None
    }

    fn handle_add_to_playlist_key(&mut self) -> Option<AppEvent> {
        if self.playlists.len() <= 1 {
            self.pick_index = 0;
            Some(AppEvent::AddToPlaylist)
        } else {
            self.picking_playlist = true;
            self.pick_index = 0;
            None
        }
    }
}
