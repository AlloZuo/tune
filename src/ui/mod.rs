pub mod input;
pub mod render;

pub use render::draw;
pub use render::format_duration;

use ratatui::widgets::ListState;

use crate::server::{MusicEntry, MusicServer, ServerConfig};
use crate::player::Player;
use std::sync::Arc;

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
    CancelSearch,
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

    // Play queue
    AddToQueue,
    PlayNext,
    QueuePlaySelected, // play immediately from queue overlay
    ToggleQueue,
    ToggleLanguage,
    CycleSort,
}

// ── Sort mode ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Default,
    Name,
    Artist,
    Duration,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Default => SortMode::Name,
            SortMode::Name => SortMode::Artist,
            SortMode::Artist => SortMode::Duration,
            SortMode::Duration => SortMode::Default,
        }
    }

    pub fn label(&self) -> String {
        match self {
            SortMode::Default => crate::tf!("sort.default"),
            SortMode::Name => crate::tf!("sort.name"),
            SortMode::Artist => crate::tf!("sort.artist"),
            SortMode::Duration => crate::tf!("sort.duration"),
        }
    }
}

// ──────────────────────────────────────────────
// App
// ──────────────────────────────────────────────

pub struct App {
    pub all_music: Vec<MusicEntry>,
    pub filtered_music: Vec<MusicEntry>,
    pub list_state: ListState,
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
    pub pl_list_state: ListState,
    pub pl_content_state: ListState,
    pub creating_playlist: bool,
    pub new_playlist_name: String,
    pub picking_playlist: bool,
    pub pick_index: usize,

    // ── Server config ──
    pub server_configs: Vec<ServerConfig>,
    pub config_servers: Vec<ServerConfig>,
    pub server: Arc<dyn MusicServer>,
    pub config_mode: bool,
    pub config_phase: u8,
    pub config_focus: usize,
    pub config_edit_idx: usize,
    pub config_inputs: Vec<String>,

    // ── UI state ──
    pub show_help: bool,
    pub playing_source: Option<PlayingSource>,

    // ── Play queue overlay ──
    pub showing_queue: bool,
    pub queue_selected: usize,

    // ── Quit confirmation ──
    pub confirm_quit: bool,

    // ── Resume playback ──
    /// Pending seek position (ms) to apply once audio starts playing.
    /// Used to resume from a saved position on startup.
    pub resume_position_ms: Option<u64>,

    // ── Sort ──
    pub sort_mode: SortMode,
}

impl App {
    pub fn new(player: Player, server: Arc<dyn MusicServer>, configs: Vec<ServerConfig>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            all_music: Vec::new(),
            filtered_music: Vec::new(),
            list_state,
            player,
            status_message: crate::tf!("app.ready"),
            error_message: None,
            search_mode: false,
            search_query: String::new(),
            downloading: false,
            download_progress: None,

            playlists: Vec::new(),
            view_mode: ViewMode::Browse,
            pl_list_state: ListState::default(),
            pl_content_state: ListState::default(),
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
            showing_queue: false,
            queue_selected: 0,
            confirm_quit: false,
            resume_position_ms: None,
            sort_mode: SortMode::Default,
        }
    }

    // ── Music list ──

    pub fn set_music_list(&mut self, musics: Vec<MusicEntry>) {
        self.all_music = musics;
        self.apply_filter();
        self.status_message = crate::tf!("status.music_loaded", self.all_music.len(), self.all_music.len());
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
        self.apply_sort();
        if self.filtered_music.is_empty() {
            self.list_state.select(None);
        } else {
            let idx = self.list_state.selected().unwrap_or(0);
            self.list_state
                .select(Some(idx.min(self.filtered_music.len() - 1)));
        }
    }

    pub fn apply_sort(&mut self) {
        match self.sort_mode {
            SortMode::Default => {} // keep server/addition order
            SortMode::Name => {
                self.filtered_music
                    .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            }
            SortMode::Artist => {
                self.filtered_music
                    .sort_by(|a, b| a.artist.to_lowercase().cmp(&b.artist.to_lowercase()));
            }
            SortMode::Duration => {
                self.filtered_music.sort_by_key(|m| m.duration);
            }
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

    pub fn add_to_playlist(&mut self, pl_idx: usize, music: MusicEntry) {
        if pl_idx < self.playlists.len() {
            if !self.playlists[pl_idx]
                .songs
                .iter()
                .any(|s| s.absolute_path == music.absolute_path)
            {
                self.playlists[pl_idx].songs.push(music);
            }
        }
    }

    pub fn create_playlist(&mut self, name: String) {
        let name = if name.trim().is_empty() {
            crate::tf!("playlist.default_name", self.playlists.len() + 1)
        } else {
            name.trim().to_string()
        };
        self.playlists.push(Playlist {
            name,
            songs: Vec::new(),
        });
    }

    pub fn delete_playlist(&mut self, idx: usize) -> bool {
        if idx < self.playlists.len() {
            if let ViewMode::PlaylistContent(ci) = self.view_mode {
                if ci == idx {
                    self.view_mode = ViewMode::PlaylistList;
                } else if ci > idx {
                    self.view_mode = ViewMode::PlaylistContent(ci - 1);
                }
            }
            self.playlists.remove(idx);
            true
        } else {
            false
        }
    }

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

    /// Return the songs to display in the current playlist content view,
    /// filtered by `search_query` when search mode is active.
    pub fn current_playlist_songs(&self, pl_idx: usize) -> Vec<MusicEntry> {
        let Some(pl) = self.playlists.get(pl_idx) else {
            return Vec::new();
        };
        if self.search_query.is_empty() {
            pl.songs.clone()
        } else {
            let q = self.search_query.to_lowercase();
            pl.songs
                .iter()
                .filter(|m| {
                    m.name.to_lowercase().contains(&q)
                        || m.artist.to_lowercase().contains(&q)
                })
                .cloned()
                .collect()
        }
    }

    pub fn selected_in_current_view(&self) -> Option<(MusicEntry, String)> {
        match self.view_mode {
            ViewMode::Browse => {
                self.selected_music()
                    .map(|m| (m.clone(), crate::tf!("view.browse")))
            }
            ViewMode::PlaylistContent(pl_idx) => {
                let pl = self.playlists.get(pl_idx)?;
                let idx = self.pl_content_state.selected()?;
                // When search is active, index into the filtered list
                if self.search_query.is_empty() {
                    pl.songs.get(idx).map(|m| (m.clone(), pl.name.clone()))
                } else {
                    let filtered = self.current_playlist_songs(pl_idx);
                    filtered.get(idx).map(|m| (m.clone(), pl.name.clone()))
                }
            }
            ViewMode::PlaylistList => None,
        }
    }
}
