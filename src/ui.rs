use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListDirection, ListItem, Paragraph,
    },
    Frame,
};

use crate::api::{MusicEntry, MusicServer, ServerConfig};
use std::sync::Arc;
use crate::player::{Player, PlayerState};

// ──────────────────────────────────────────────
// Data types
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Playlist {
    pub name: String,
    pub songs: Vec<MusicEntry>,
}

/// Tracks which list the currently playing song belongs to,
/// and its index in that list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayingSource {
    /// Index in `filtered_music`.
    Browse(usize),
    /// (playlist_index, song_index).
    PlaylistContent(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewMode {
    Browse,
    PlaylistList,
    PlaylistContent(usize),
}

// ──────────────────────────────────────────────
// Events
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppEvent {
    PlaySelected,
    TogglePlayback,
    Stop,
    SeekForward,
    SeekBackward,
    VolumeUp,
    VolumeDown,
    CyclePlayMode,
    EnterSearch,
    ConfirmSearch,
    DeleteSearchChar,
    PushSearchChar(char),
    MoveUp,
    MoveDown,
    ScrollUp,
    ScrollDown,
    CreatePlaylist,
    ConfirmCreatePlaylist,
    AddToPlaylist,
    DeleteItem,
    Refresh,
    GoToPlaying,
    ShowHelp,
    ConfigureServer,
    ConfirmConfig,
    Quit,
    None,
}

// ──────────────────────────────────────────────
// App
// ──────────────────────────────────────────────

pub struct App {
    pub all_music: Vec<MusicEntry>,
    pub filtered_music: Vec<MusicEntry>,
    /// ListState for the browse view (all-music / filtered list).
    pub list_state: ratatui::widgets::ListState,
    pub player: Player,
    pub status_message: String,
    pub error_message: Option<String>,
    pub search_mode: bool,
    pub search_query: String,
    pub downloading: bool,
    pub download_progress: Option<String>,

    // ── Playlists ──
    pub playlists: Vec<Playlist>,
    pub view_mode: ViewMode,
    /// ListState for the playlist-list view.
    pub pl_list_state: ratatui::widgets::ListState,
    /// ListState for the playlist-content view.
    pub pl_content_state: ratatui::widgets::ListState,
    /// True when user is typing a new playlist name.
    pub creating_playlist: bool,
    pub new_playlist_name: String,
    /// True when showing the pick-playlist overlay.
    pub picking_playlist: bool,
    /// Index selected inside the picker.
    pub pick_index: usize,
    /// Server configurations (multiple servers).
    pub server_configs: Vec<ServerConfig>,
    /// Working copy of configs while editing.
    pub config_servers: Vec<ServerConfig>,
    /// Music server adapter instance.
    pub server: Arc<dyn MusicServer>,
    /// True when editing server configuration.
    pub config_mode: bool,
    /// 0 = server list, 1 = editing a single server.
    pub config_phase: u8,
    /// In list mode: selected server index. In edit mode: focused field index.
    pub config_focus: usize,
    /// Which server config we're editing (index into config_servers).
    pub config_edit_idx: usize,
    /// Input buffers for each config field (in edit mode).
    pub config_inputs: Vec<String>,
    /// True when showing the help overlay.
    pub show_help: bool,
    /// Which list the current track came from (for auto-next).
    pub playing_source: Option<PlayingSource>,
}

impl App {
    pub fn new(player: Player, server: Arc<dyn MusicServer>, configs: Vec<ServerConfig>) -> Self {
        let mut list_state = ratatui::widgets::ListState::default();
        list_state.select(Some(0));
        Self {
            all_music: Vec::new(),
            filtered_music: Vec::new(),
            list_state,
            player,
            status_message: "准备就绪 - 按 q 退出".to_string(),
            error_message: None,
            search_mode: false,
            search_query: String::new(),
            downloading: false,
            download_progress: None,

            playlists: Vec::new(),
            view_mode: ViewMode::Browse,
            pl_list_state: ratatui::widgets::ListState::default(),
            pl_content_state: ratatui::widgets::ListState::default(),
            creating_playlist: false,
            new_playlist_name: String::new(),
            picking_playlist: false,
            pick_index: 0,
            server,
            server_configs: configs,
            config_servers: Vec::new(),
            config_mode: false,
            config_phase: 0,
            config_focus: 0,
            config_edit_idx: 0,
            config_inputs: Vec::new(),
            show_help: false,
            playing_source: None,
        }
    }

    // ── Music list ──

    pub fn set_music_list(&mut self, musics: Vec<MusicEntry>) {
        self.all_music = musics;
        self.apply_filter();
        self.status_message = format!(
            "共 {} 首音乐，已加载 {} 首",
            self.all_music.len(),
            self.all_music.len()
        );
    }

    pub fn apply_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_music = self.all_music.clone();
        } else {
            let q = self.search_query.to_lowercase();
            self.filtered_music = self
                .all_music
                .iter()
                .filter(|m| {
                    m.name.to_lowercase().contains(&q)
                        || m.artist.to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
        if self.filtered_music.is_empty() {
            self.list_state.select(None);
        } else {
            let idx = self.list_state.selected().unwrap_or(0);
            self.list_state
                .select(Some(idx.min(self.filtered_music.len() - 1)));
        }
    }

    /// Currently selected music entry (in browse view).
    pub fn selected_music(&self) -> Option<&MusicEntry> {
        let idx = self.list_state.selected()?;
        self.filtered_music.get(idx)
    }

    // ── Playlist helpers ──

    pub fn current_playlist(&self) -> Option<&Playlist> {
        match self.view_mode {
            ViewMode::PlaylistContent(idx) => self.playlists.get(idx),
            _ => None,
        }
    }

    pub fn current_playlist_mut(&mut self) -> Option<&mut Playlist> {
        match self.view_mode {
            ViewMode::PlaylistContent(idx) => self.playlists.get_mut(idx),
            _ => None,
        }
    }

    /// Add a music entry to a playlist by index.
    pub fn add_to_playlist(&mut self, pl_idx: usize, music: MusicEntry) {
        if pl_idx < self.playlists.len() {
            // Avoid duplicates by absolute_path (unique per song across all backends).
            // Note: `id` is unreliable (Navidrome always sets it to 0).
            if !self.playlists[pl_idx]
                .songs
                .iter()
                .any(|s| s.absolute_path == music.absolute_path)
            {
                self.playlists[pl_idx].songs.push(music);
            }
        }
    }

    /// Create a new empty playlist.
    pub fn create_playlist(&mut self, name: String) {
        let name = if name.trim().is_empty() {
            format!("歌单 {}", self.playlists.len() + 1)
        } else {
            name.trim().to_string()
        };
        self.playlists.push(Playlist {
            name,
            songs: Vec::new(),
        });
    }

    /// Delete a playlist by index.
    pub fn delete_playlist(&mut self, idx: usize) -> bool {
        if idx < self.playlists.len() {
            if let ViewMode::PlaylistContent(ci) = self.view_mode {
                if ci == idx {
                    // If we're viewing the deleted playlist, go back to list
                    self.view_mode = ViewMode::PlaylistList;
                } else if ci > idx {
                    // Adjust the viewed index
                    self.view_mode = ViewMode::PlaylistContent(ci - 1);
                }
            }
            self.playlists.remove(idx);
            true
        } else {
            false
        }
    }

    /// Remove a song from the current playlist by its index in the song list.
    pub fn remove_song_from_current_playlist(&mut self, song_idx: usize) -> bool {
        if let Some(pl) = self.current_playlist_mut() {
            if song_idx < pl.songs.len() {
                pl.songs.remove(song_idx);
                return true;
            }
        }
        false
    }

    // ── Navigation ──

    pub fn navigate_up(&mut self) {
        match self.view_mode {
            ViewMode::Browse => {
                let i = self.list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.list_state.select(Some(i - 1));
                }
            }
            ViewMode::PlaylistList => {
                let i = self.pl_list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.pl_list_state.select(Some(i - 1));
                }
            }
            ViewMode::PlaylistContent(_) => {
                let i = self.pl_content_state.selected().unwrap_or(0);
                if i > 0 {
                    self.pl_content_state.select(Some(i - 1));
                }
            }
        }
    }

    pub fn navigate_down(&mut self) {
        match self.view_mode {
            ViewMode::Browse => {
                if self.filtered_music.is_empty() {
                    return;
                }
                let i = self.list_state.selected().unwrap_or(0);
                if i + 1 < self.filtered_music.len() {
                    self.list_state.select(Some(i + 1));
                }
            }
            ViewMode::PlaylistList => {
                let i = self.pl_list_state.selected().unwrap_or(0);
                if i + 1 < self.playlists.len() {
                    self.pl_list_state.select(Some(i + 1));
                }
            }
            ViewMode::PlaylistContent(_) => {
                let len = self.current_playlist().map_or(0, |p| p.songs.len());
                if len == 0 {
                    return;
                }
                let i = self.pl_content_state.selected().unwrap_or(0);
                if i + 1 < len {
                    self.pl_content_state.select(Some(i + 1));
                }
            }
        }
    }

    pub fn scroll_up(&mut self) {
        let page: usize = 15;
        match self.view_mode {
            ViewMode::Browse => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(page)));
            }
            ViewMode::PlaylistList => {
                let i = self.pl_list_state.selected().unwrap_or(0);
                self.pl_list_state.select(Some(i.saturating_sub(page)));
            }
            ViewMode::PlaylistContent(_) => {
                let i = self.pl_content_state.selected().unwrap_or(0);
                self.pl_content_state.select(Some(i.saturating_sub(page)));
            }
        }
    }

    pub fn scroll_down(&mut self) {
        let page: usize = 15;
        match self.view_mode {
            ViewMode::Browse => {
                if self.filtered_music.is_empty() {
                    return;
                }
                let i = self.list_state.selected().unwrap_or(0);
                let new = (i + page).min(self.filtered_music.len() - 1);
                self.list_state.select(Some(new));
            }
            ViewMode::PlaylistList => {
                if self.playlists.is_empty() {
                    return;
                }
                let i = self.pl_list_state.selected().unwrap_or(0);
                let new = (i + page).min(self.playlists.len() - 1);
                self.pl_list_state.select(Some(new));
            }
            ViewMode::PlaylistContent(_) => {
                let len = self.current_playlist().map_or(0, |p| p.songs.len());
                if len == 0 {
                    return;
                }
                let i = self.pl_content_state.selected().unwrap_or(0);
                let new = (i + page).min(len - 1);
                self.pl_content_state.select(Some(new));
            }
        }
    }

    /// Get the selected entry in the current view.
    pub fn selected_in_current_view(&self) -> Option<(MusicEntry, String)> {
        match self.view_mode {
            ViewMode::Browse => {
                self.selected_music()
                    .map(|m| (m.clone(), format!("浏览")))
            }
            ViewMode::PlaylistContent(pl_idx) => {
                let pl = self.playlists.get(pl_idx)?;
                let idx = self.pl_content_state.selected()?;
                pl.songs.get(idx).map(|m| (m.clone(), pl.name.clone()))
            }
            ViewMode::PlaylistList => None,
        }
    }

    // ── Key handler ──

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
                // ── Server list phase ──
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
                        // Enter edit mode for selected server, or add new if empty
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
                        // Toggle disabled state for the selected server
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
                // ── Edit single server phase ──
                match key.code {
                    KeyCode::Enter => {
                        // Save this server and go back to list
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
                        // Discard edits, back to list
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
                    // Left/Right cycle server type when focus is on the type field (index 1)
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
                        // Block backspace on type (index 1) only; name (0) and others are editable
                        if self.config_focus != 1 && self.config_focus < self.config_inputs.len() {
                            self.config_inputs[self.config_focus].pop();
                        }
                        None
                    }
                    KeyCode::Char(c) => {
                        // Block char input on type (index 1) only
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

        if self.search_mode {
            return match key.code {
                KeyCode::Esc | KeyCode::Enter => Some(AppEvent::ConfirmSearch),
                KeyCode::Backspace => Some(AppEvent::DeleteSearchChar),
                KeyCode::Char(c) => Some(AppEvent::PushSearchChar(c)),
                _ => None,
            };
        }

        // ── Normal mode ──
        match key.code {
            // Global
            KeyCode::Char('q') => Some(AppEvent::Quit),
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
                    // Open selected playlist
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

            // Search
            KeyCode::Char('/') => {
                if matches!(self.view_mode, ViewMode::Browse) {
                    Some(AppEvent::EnterSearch)
                } else {
                    None
                }
            }

            // Refresh / Config / GoToPlaying
            KeyCode::Char('r') => Some(AppEvent::Refresh),
            KeyCode::Char('R') => Some(AppEvent::ConfigureServer),
            KeyCode::Char('g') => Some(AppEvent::GoToPlaying),

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
                        // No playlists yet — create default and add directly
                        Some(AppEvent::AddToPlaylist)
                    } else if self.playlists.len() == 1 {
                        // Only one playlist — add directly
                        self.pick_index = 0;
                        Some(AppEvent::AddToPlaylist)
                    } else {
                        // Multiple playlists — show picker
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

            KeyCode::Char('h' | '?') => Some(AppEvent::ShowHelp),
            _ => None,
        }
    }
}

// ──────────────────────────────────────────────
// Rendering
// ──────────────────────────────────────────────

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(1),    // Main content
            Constraint::Length(3), // Status bar
            Constraint::Length(3), // Playback bar
        ])
        .split(area);

    render_header(frame, chunks[0], app);

    match app.view_mode {
        ViewMode::Browse => render_music_list(frame, chunks[1], app),
        ViewMode::PlaylistList => render_playlist_list(frame, chunks[1], app),
        ViewMode::PlaylistContent(_) => render_playlist_content(frame, chunks[1], app),
    }

    render_status_bar(frame, chunks[2], app);
    render_playback_bar(frame, chunks[3], app);

    // Overlays
    if app.search_mode {
        render_search_overlay(frame, area, app);
    }
    if app.creating_playlist {
        render_create_playlist_overlay(frame, area, app);
    }
    if app.picking_playlist {
        render_pick_playlist_overlay(frame, area, app);
    }
    if app.config_mode {
        render_config_overlay(frame, area, app);
    }
    if app.show_help {
        render_help_overlay(frame, area, app);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let view_label = match app.view_mode {
        ViewMode::Browse => "音乐列表",
        ViewMode::PlaylistList => "歌单管理",
        ViewMode::PlaylistContent(_) => "歌单内容",
    };

    // Show playlist name when playing from a playlist
    let source_tag = match app.playing_source {
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => app
            .playlists
            .get(pl_idx)
            .map(|pl| format!("  🎵{}", pl.name))
            .unwrap_or_default(),
        _ => String::new(),
    };

    // Show server count or first server info
    let server_tag = if app.server_configs.is_empty() {
        String::new()
    } else if app.server_configs.len() == 1 {
        let cfg = &app.server_configs[0];
        let url = cfg
            .server_url
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        format!("[{}] {}", cfg.server_type, url)
    } else {
        format!("[{} 个服务器]", app.server_configs.len())
    };

    let title = format!(
        " ♪ 音源播放器 [{}]{}  {}  |  {} 首  {} 歌单",
        view_label,
        source_tag,
        server_tag,
        app.all_music.len(),
        app.playlists.len(),
    );
    let block = Block::default()
        .title(title)
        .title_alignment(ratatui::layout::Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Cyan));
    frame.render_widget(block, area);
}

// ── Browse: music list ──

fn render_music_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let selected = app.list_state.selected();
    let items: Vec<ListItem> = app
        .filtered_music
        .iter()
        .enumerate()
        .map(|(i, music)| {
            let is_selected = Some(i) == selected;
            let name = music.name.clone();
            let artist = if music.artist == "<unknown>" || music.artist.is_empty() {
                "未知艺术家".to_string()
            } else {
                music.artist.clone()
            };
            let dur = format_duration(music.duration);

            let content = Line::from(vec![
                Span::styled(name, Style::new().add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(artist, Style::new().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(dur, Style::new().fg(Color::Gray)),
            ]);

            if is_selected {
                ListItem::new(content).style(Style::new().bg(Color::Blue).fg(Color::White))
            } else {
                ListItem::new(content)
            }
        })
        .collect();

    let title = if app.search_query.is_empty() {
        format!(" 音乐列表 ({}) ", items.len())
    } else {
        format!(" 搜索: {} ({} 结果) ", app.search_query, items.len())
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(Color::Cyan)),
        )
        .direction(ListDirection::TopToBottom)
        .highlight_symbol("▸ ")
        .highlight_style(Style::new().bg(Color::Blue).fg(Color::White));

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

// ── Playlist list ──

fn render_playlist_list(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.playlists.is_empty() {
        let msg = Paragraph::new("还没有歌单 — 按 c 创建新歌单")
            .style(Style::new().fg(Color::Gray))
            .block(
                Block::default()
                    .title(" 歌单管理 ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(Color::Cyan)),
            );
        frame.render_widget(msg, area);
        return;
    }

    let selected = app.pl_list_state.selected();
    let items: Vec<ListItem> = app
        .playlists
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let is_selected = Some(i) == selected;
            let label = format!(" {} ({} 首)", pl.name, pl.songs.len());
            let content = Line::from(Span::raw(label));
            if is_selected {
                ListItem::new(content).style(Style::new().bg(Color::Blue).fg(Color::White))
            } else {
                ListItem::new(content)
            }
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" 歌单管理 ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(Color::Cyan)),
        )
        .direction(ListDirection::TopToBottom)
        .highlight_symbol("▸ ")
        .highlight_style(Style::new().bg(Color::Blue).fg(Color::White));

    frame.render_stateful_widget(list, area, &mut app.pl_list_state);
}

// ── Playlist content ──

fn render_playlist_content(frame: &mut Frame, area: Rect, app: &mut App) {
    let pl = match app.current_playlist() {
        Some(p) => p.clone(),
        None => {
            let msg = Paragraph::new("歌单不存在")
                .block(
                    Block::default()
                        .title(" 错误 ")
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                );
            frame.render_widget(msg, area);
            return;
        }
    };

    if pl.songs.is_empty() {
        let msg = Paragraph::new("歌单为空 — 在音乐列表中按 a 添加歌曲")
            .style(Style::new().fg(Color::Gray))
            .block(
                Block::default()
                    .title(format!(" {} ", pl.name))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(Color::Cyan)),
            );
        frame.render_widget(msg, area);
        return;
    }

    let selected = app.pl_content_state.selected();
    let items: Vec<ListItem> = pl
        .songs
        .iter()
        .enumerate()
        .map(|(i, music)| {
            let is_selected = Some(i) == selected;
            let name = music.name.clone();
            let artist = if music.artist == "<unknown>" || music.artist.is_empty() {
                "未知艺术家".to_string()
            } else {
                music.artist.clone()
            };
            let dur = format_duration(music.duration);

            let content = Line::from(vec![
                Span::styled(name, Style::new().add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(artist, Style::new().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(dur, Style::new().fg(Color::Gray)),
            ]);

            if is_selected {
                ListItem::new(content).style(Style::new().bg(Color::Blue).fg(Color::White))
            } else {
                ListItem::new(content)
            }
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(format!(" {} ", pl.name))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(Color::Cyan)),
        )
        .direction(ListDirection::TopToBottom)
        .highlight_symbol("▸ ")
        .highlight_style(Style::new().bg(Color::Blue).fg(Color::White));

    frame.render_stateful_widget(list, area, &mut app.pl_content_state);
}

// ── Status bar ──

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let msg = if let Some(ref err) = app.error_message {
        format!(" ❌ {}", err)
    } else if app.downloading {
        format!(
            " ⏳ 下载中... {}",
            app.download_progress.as_deref().unwrap_or("")
        )
    } else {
        format!(" {}", app.status_message)
    };

    let style = if app.error_message.is_some() {
        Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Green)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::DarkGray));
    let paragraph = Paragraph::new(Text::from(msg))
        .style(style)
        .block(block);
    frame.render_widget(paragraph, area);
}

// ── Playback bar ──

fn render_playback_bar(frame: &mut Frame, area: Rect, app: &App) {
    let icon = match app.player.state() {
        PlayerState::Playing => "▶",
        PlayerState::Paused => "⏸",
        PlayerState::Stopped => "⏹",
    };

    let mode_name = app.player.play_mode.short_label();
    let (label_text, has_track) = match app.player.current_track() {
        Some(track) => {
            let pos = format_duration(app.player.position_ms());
            let total = format_duration(track.total_duration_ms);
            (
                format!(
                    " {} {} - {}  [{}/{}]  [{}]",
                    icon, track.artist, track.title, pos, total, mode_name
                ),
                true,
            )
        }
        None => (
            format!(" {} (空闲)  [{}]", icon, mode_name),
            false,
        ),
    };

    // ── Current lyrics: original on top border, translation on bottom border ──
    let pos = app.player.position_ms();
    let (orig_text, trans_text) = app
        .player
        .current_track()
        .and_then(|t| t.lyrics.as_ref())
        .map(|l| l.lines_at(pos))
        .unwrap_or((None, None));

    let truncate = |s: &str| -> String {
        let max_len = area.width.saturating_sub(10) as usize;
        if s.len() > max_len {
            let limit = max_len.saturating_sub(2);
            let byte_end = s
                .char_indices()
                .nth(limit)
                .map(|(i, _)| i)
                .unwrap_or(s.len());
            format!(" {}… ", &s[..byte_end])
        } else {
            format!(" {} ", s)
        }
    };

    let lyric_title = orig_text
        .filter(|s| !s.is_empty())
        .map(truncate)
        .unwrap_or_default();
    let lyric_bottom = trans_text
        .filter(|s| !s.is_empty())
        .map(truncate)
        .unwrap_or_default();

    let volume = (app.player.volume() * 100.0).round() as u8;
    let volume_display = format!("音量: {}%", volume);

    let percent = if has_track {
        let total = app
            .player
            .current_track()
            .map_or(1, |t| t.total_duration_ms.max(1));
        ((app.player.position_ms() as f64 / total as f64 * 100.0).round() as u16).clamp(0, 100)
    } else {
        0
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(lyric_title)
                .title_alignment(Alignment::Left)
                .title_bottom(lyric_bottom)
                .title_alignment(Alignment::Left)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .gauge_style(Style::new().fg(Color::LightCyan).bg(Color::DarkGray))
        .percent(percent)
        .label(Span::styled(
            label_text,
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
        ));

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(12)])
        .split(area);

    frame.render_widget(gauge, layout[0]);

    let vol_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::DarkGray));
    let vol_para = Paragraph::new(volume_display)
        .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(vol_block);
    frame.render_widget(vol_para, layout[1]);
}

// ── Help bar ──

// ── Overlays ──

fn render_search_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(60, 20, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" 搜索音乐 ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let input = Paragraph::new(Text::from(app.search_query.as_str()))
        .style(Style::new().fg(Color::White))
        .block(block);

    frame.render_widget(input, overlay_area);
}

fn render_create_playlist_overlay(frame: &mut Frame, area: Rect, _app: &App) {
    let overlay_area = centered_rect(60, 20, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" 新建歌单 ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let input = Paragraph::new(Text::from(_app.new_playlist_name.as_str()))
        .style(Style::new().fg(Color::White))
        .block(block);

    frame.render_widget(input, overlay_area);
}

fn render_help_overlay(frame: &mut Frame, area: Rect, _app: &App) {
    let overlay_area = centered_rect_lines(27, area);
    frame.render_widget(Clear, overlay_area);

    let help_text = vec![
        "╔══════════════════════════════════════╗",
        "║             键盘快捷键                ║",
        "╠══════════════════════════════════════╣",
        "║  ── 播放控制 ──                      ║",
        "║  Enter       播放选中                ║",
        "║  Space       暂停 / 继续             ║",
        "║  s           停止                    ║",
        "║  ← / →       快退 / 快进 5秒         ║",
        "║  m           切换播放模式            ║",
        "║  + / =       音量增加                ║",
        "║  - / _       音量减少                ║",
        "║  ── 列表导航 ──                      ║",
        "║  ↑ / ↓       上下选择                ║",
        "║  PgUp / PgDn 翻页                    ║",
        "║  g           跳转到正在播放          ║",
        "║  /           搜索                    ║",
        "║  ── 歌单操作 ──                      ║",
        "║  a           加入歌单                ║",
        "║  l           歌单管理 / 返回         ║",
        "║  c           创建歌单                ║",
        "║  d           删除歌单 / 移出         ║",
        "║  ── 系统 ──                          ║",
        "║  r           刷新音乐列表            ║",
        "║  R           配置服务器地址          ║",
        "║  h / ?       本帮助                  ║",
        "║  q / Esc     退出 / 返回             ║",
        "╚══════════════════════════════════════╝",
    ];

    let lines: Vec<Line> = help_text
        .iter()
        .map(|s| {
            let is_header = s.starts_with('╔') || s.starts_with('╚') || s.starts_with('╠');
            let style = if is_header {
                Style::new().fg(Color::Cyan)
            } else {
                Style::new().fg(Color::White)
            };
            Line::from(Span::styled(*s, style))
        })
        .collect();

    let paragraph = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::NONE)
                .style(Style::new().bg(Color::Black)),
        )
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, overlay_area);
}

fn render_config_overlay(frame: &mut Frame, area: Rect, app: &App) {
    if app.config_phase == 0 {
        render_config_list(frame, area, app);
    } else {
        render_config_edit(frame, area, app);
    }
}

/// Config phase 0: server list (select, add, delete).
fn render_config_list(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(60, 30, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" 服务器管理 (Enter编辑 a添加 d删除 Space停用 Esc保存) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let mut lines: Vec<Line> = Vec::new();
    if app.config_servers.is_empty() {
        lines.push(Line::from(Span::styled(
            "  暂无服务器，按 a 添加",
            Style::new().fg(Color::DarkGray),
        )));
    } else {
        for (i, cfg) in app.config_servers.iter().enumerate() {
            let is_selected = i == app.config_focus;
            let prefix = if is_selected { " ▸ " } else { "   " };
            let url_short = cfg
                .server_url
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            let label = if cfg.name.is_empty() {
                format!("{} ({})", cfg.server_type, url_short)
            } else {
                format!("{} [{}] ({})", cfg.name, cfg.server_type, url_short)
            };
            let status = if cfg.disabled {
                " [停用]"
            } else {
                ""
            };
            let style = if cfg.disabled {
                // Dimmed style for disabled servers
                if is_selected {
                    Style::new().fg(Color::DarkGray).bg(Color::Blue)
                } else {
                    Style::new().fg(Color::DarkGray)
                }
            } else {
                if is_selected {
                    Style::new().fg(Color::White).bg(Color::Blue)
                } else {
                    Style::new().fg(Color::White)
                }
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}{}", prefix, label, status),
                style,
            )));
        }
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::new().bg(Color::Black))
        .block(block);
    frame.render_widget(paragraph, overlay_area);
}

/// Config phase 1: edit a single server's fields.
fn render_config_edit(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(70, 35, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" 编辑服务器 (Tab切换 Enter保存) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    // Field labels and their input indices
    let fields: &[(&str, usize)] = &[
        ("名称", 0),   // editable text
        ("类型", 1),   // ←/→ cycling
        ("URL", 2),    // editable text
        ("用户名", 3), // editable text
        ("密码", 4),   // editable text, masked
    ];

    let mut lines = Vec::new();
    for &(label, idx) in fields {
        let is_focused = idx == app.config_focus;
        let value = if idx == 1 {
            // Type selector
            let type_val = app.config_inputs.get(idx).map(|s| s.as_str()).unwrap_or("file-transfer");
            if is_focused {
                format!(" ◄ {} ► ", type_val)
            } else {
                format!(" {} ", type_val)
            }
        } else if idx == 4 && !is_focused {
            // Password: show asterisks when not editing
            let len = app.config_inputs.get(idx).map(|s| s.len()).unwrap_or(0);
            "*".repeat(len.min(20))
        } else {
            app.config_inputs.get(idx).map(|s| s.as_str()).unwrap_or("").to_string()
        };
        let prefix = if is_focused { " ▸ " } else { "   " };
        let value_style = if is_focused {
            if idx == 1 {
                Style::new().fg(Color::White).bg(Color::Blue)
            } else {
                Style::new().fg(Color::White).bg(Color::Blue)
            }
        } else if idx == 1 {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{}: ", prefix, label), Style::new().fg(Color::DarkGray)),
            Span::styled(value, value_style),
        ]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::new().bg(Color::Black))
        .block(block);
    frame.render_widget(paragraph, overlay_area);
}

fn render_pick_playlist_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(50, 40, area);
    frame.render_widget(Clear, overlay_area);

    let mut lines = Vec::new();
    for (i, pl) in app.playlists.iter().enumerate() {
        let prefix = if i == app.pick_index { "▸ " } else { "  " };
        let style = if i == app.pick_index {
            Style::new().fg(Color::White).bg(Color::Blue)
        } else {
            Style::new().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!("{}{} ({} 首)", prefix, pl.name, pl.songs.len()),
            style,
        )));
    }

    let block = Block::default()
        .title(" 添加到歌单 ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let para = Paragraph::new(Text::from(lines))
        .block(block);
    frame.render_widget(para, overlay_area);
}

// ── Helpers ──

/// Center a rectangle with a fixed number of lines (height in rows).
fn centered_rect_lines(height_lines: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height_lines),
            Constraint::Fill(1),
        ])
        .split(area);
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Percentage(70),
            Constraint::Fill(1),
        ])
        .split(vert[1]);
    horiz[1]
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}
