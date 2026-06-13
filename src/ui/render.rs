use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListDirection, ListItem, ListState, Paragraph,
    },
    Frame,
};

use crate::player::PlayerState;
use crate::server::MusicEntry;

use super::{App, CoverArt, DisplayItem, PlayingSource, SortMode, ViewMode};

// ──────────────────────────────────────────────
// Rendering entry point
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
    if app.show_cover {
        render_cover_overlay(frame, area, app);
    }
    if app.show_help {
        render_help_overlay(frame, area, app);
    }
    if app.showing_queue {
        render_queue_overlay(frame, area, app);
    }
    if app.confirm_quit {
        render_quit_overlay(frame, area, app);
    }
}

// ── Header ──

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let view_label = match app.view_mode {
        ViewMode::Browse => {
            let base = crate::t!("view.browse");
            if app.sort_mode != SortMode::Default {
                format!("{} [{}]", base, app.sort_mode.label())
            } else {
                base.to_string()
            }
        }
        ViewMode::PlaylistList => crate::t!("view.playlist_list").to_string(),
        ViewMode::PlaylistContent(_) => crate::t!("view.playlist_content").to_string(),
    };
    let view_label = &view_label;

    let source_tag = match app.playing_source {
        Some(PlayingSource::PlaylistContent(pl_idx, _)) => app
            .playlists
            .get(pl_idx)
            .map(|pl| format!("  🎵{}", pl.name))
            // safe: .get() returns None for invalid pl_idx → empty string fallback
            .unwrap_or_default(),
        _ => String::new(),
    };

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
        format!("[{}]", crate::tf!("misc.servers_count", app.server_configs.len()))
    };

    let title = crate::tf!("header.title", view_label, source_tag, server_tag, app.all_music.len(), app.playlists.len());
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Cyan));
    frame.render_widget(block, area);
}

// ── Browse: music list ──

fn render_music_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let selected = app.list_state.selected();
    let is_grouped = app.sort_mode == SortMode::Album;

    let items: Vec<ListItem> = app
        .display_items
        .iter()
        .enumerate()
        .map(|(i, item)| match item {
            DisplayItem::Song(data_idx) => {
                let music = &app.filtered_music[*data_idx];
                let is_playing = app.player
                    .current_track()
                    .is_some_and(|t| t.absolute_path == music.absolute_path);
                song_to_list_item(i, music, selected, !is_grouped, is_playing)
            }
            DisplayItem::AlbumHeader {
                album,
                artist,
                song_count,
            } => {
                let header_text = format!("  {}  —  {}  ({}首)", album, artist, song_count);
                let content = Line::from(vec![Span::styled(
                    header_text,
                    Style::new()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )]);
                ListItem::new(content).style(Style::new().bg(Color::Black))
            }
        })
        .collect();

    let title = if app.search_query.is_empty() {
        format!(" {} ({}) ", crate::t!("view.browse"), items.len())
    } else {
        format!(
            " {} ",
            crate::tf!("view.search_results", &app.search_query, items.len())
        )
    };

    let title = if let Some(ref artist) = app.artist_filter {
        format!("{} — {}: {} ", title.trim(), crate::t!("misc.filter_artist"), artist)
    } else {
        title
    };

    // Add sort mode and grouping badge
    let title = if is_grouped {
        format!("{} [{}] ", title.trim(), crate::t!("sort.album"))
    } else if app.sort_mode != SortMode::Default {
        format!("{} [{}] ", title.trim(), app.sort_mode.label())
    } else {
        title
    };

    render_song_list(frame, area, items, title.trim().to_string(), &mut app.list_state);
}

// ── Playlist list ──

fn render_playlist_list(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.playlists.is_empty() {
        let msg = Paragraph::new(crate::t!("playlist.empty"))
            .style(Style::new().fg(Color::Gray))
            .block(
                Block::default()
                    .title(format!(" {} ", crate::t!("view.playlist_list")))
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
            let label = crate::tf!("playlist.song_count", &pl.name, pl.songs.len());
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
                .title(format!(" {} ", crate::t!("view.playlist_list")))
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
    let pl_idx = match app.view_mode {
        ViewMode::PlaylistContent(i) => i,
        _ => return,
    };

    let songs = app.current_playlist_songs(pl_idx);

    if songs.is_empty() && !app.search_query.is_empty() {
        // Filtered to empty — show no-results state
        let msg = Paragraph::new(crate::tf!("view.search_results", &app.search_query, 0))
            .style(Style::new().fg(Color::Gray))
            .block(
                Block::default()
                    .title(format!(" {} ", app.playlists.get(pl_idx).map_or(String::new(), |p| p.name.clone())))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(Color::Cyan)),
            );
        frame.render_widget(msg, area);
        return;
    }

    if songs.is_empty() {
        let msg = Paragraph::new(crate::t!("playlist.content_empty"))
            .style(Style::new().fg(Color::Gray))
            .block(
                Block::default()
                    .title(format!(" {} ", app.playlists.get(pl_idx).map_or(String::new(), |p| p.name.clone())))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(Color::Cyan)),
            );
        frame.render_widget(msg, area);
        return;
    }

    // Reset selection if it exceeds filtered list
    let selected = app.pl_content_state.selected();
    if selected.is_some_and(|s| s >= songs.len()) {
        app.pl_content_state.select(Some(songs.len().saturating_sub(1)));
    }

    let items: Vec<ListItem> = songs
        .iter()
        .enumerate()
        .map(|(i, music)| {
            let is_playing = app.player
                .current_track()
                .is_some_and(|t| t.absolute_path == music.absolute_path);
            song_to_list_item(i, music, selected, false, is_playing)
        })
        .collect();

    let title = if app.search_query.is_empty() {
        let pl_name = app.playlists.get(pl_idx).map(|p| p.name.as_str()).unwrap_or("");
        format!(" {} ", pl_name)
    } else {
        format!(" {} ", crate::tf!("view.search_results", &app.search_query, items.len()))
    };

    render_song_list(frame, area, items, title, &mut app.pl_content_state);
}

// ── Shared song rendering helpers ──

fn song_to_list_item(
    i: usize,
    music: &MusicEntry,
    selected: Option<usize>,
    show_album: bool,
    is_playing: bool,
) -> ListItem<'_> {
    let is_selected = Some(i) == selected;
    let name = music.name.clone();
    let artist = if music.artist == "<unknown>" || music.artist.is_empty() {
        crate::tf!("app.unknown_artist")
    } else {
        music.artist.clone()
    };
    let dur = format_duration(music.duration);

    let mut spans: Vec<Span<'_>> = Vec::with_capacity(8);
    if is_playing {
        spans.push(Span::styled("▶ ", Style::new().fg(Color::Green)));
    }
    spans.push(Span::styled(name, Style::new().add_modifier(Modifier::BOLD)));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(artist, Style::new().fg(Color::DarkGray)));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(dur, Style::new().fg(Color::Gray)));

    if show_album && !music.album.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            music.album.clone(),
            Style::new().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ));
    }

    let content = Line::from(spans);

    if is_playing && is_selected {
        ListItem::new(content).style(Style::new().bg(Color::Blue).fg(Color::LightGreen))
    } else if is_playing {
        ListItem::new(content).style(Style::new().fg(Color::Green))
    } else if is_selected {
        ListItem::new(content).style(Style::new().bg(Color::Blue).fg(Color::White))
    } else {
        ListItem::new(content)
    }
}

fn render_song_list(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem>,
    title: String,
    list_state: &mut ListState,
) {
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

    frame.render_stateful_widget(list, area, list_state);
}

// ── Status bar ──

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let msg = if let Some(ref err) = app.error_message {
        format!(" ❌ {}", err)
    } else if app.downloading {
        let progress = app.download_progress.as_deref().unwrap_or("");
        if progress.is_empty() {
            format!(" {}", app.status_message)
        } else {
            format!(" {}  {}", app.status_message, progress)
        }
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
            format!(
                " {} ({})  [{}]",
                icon,
                crate::t!("playback.idle"),
                mode_name
            ),
            false,
        ),
    };

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
    let volume_display = crate::tf!("playback.volume", volume);

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

// ── Overlays ──

fn render_search_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(60, 20, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(format!(" {} ", crate::t!("search.title")))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let content = if app.search_query.is_empty() {
        crate::t!("search.prompt")
    } else {
        app.search_query.as_str()
    };

    let input = Paragraph::new(Text::from(content))
        .style(if app.search_query.is_empty() {
            Style::new().fg(Color::DarkGray)
        } else {
            Style::new().fg(Color::White)
        })
        .block(block);

    frame.render_widget(input, overlay_area);
}

fn render_create_playlist_overlay(frame: &mut Frame, area: Rect, _app: &App) {
    let overlay_area = centered_rect(60, 20, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(format!(" {} ", crate::t!("playlist.create_title")))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let input = Paragraph::new(Text::from(_app.new_playlist_name.as_str()))
        .style(Style::new().fg(Color::White))
        .block(block);

    frame.render_widget(input, overlay_area);
}

fn build_help_lines() -> Vec<String> {
    vec![
        format!("  {}  ", crate::t!("help.playback_header")),
        format!("  Enter       {}  ", crate::t!("help.play")),
        format!("  Space       {}  ", crate::t!("help.toggle")),
        format!("  s           {}  ", crate::t!("help.stop")),
        format!("  ← / →       {}  ", crate::t!("help.seek")),
        format!("  n           {}  ", crate::t!("help.next_track")),
        format!("  m           {}  ", crate::t!("help.play_mode")),
        format!("  + / =       {}  ", crate::t!("help.volume")),
        format!("  - / _        {}  ", ""),
        format!("  {}  ", crate::t!("help.nav_header")),
        format!("  ↑ / ↓       {}  ", crate::t!("help.nav_up_down")),
        format!("  PgUp/PgDn   {}  ", crate::t!("help.nav_page")),
        format!("  g           {}  ", crate::t!("help.goto_playing")),
        format!("  /           {}  ", crate::t!("help.search")),
        format!("  f / F       {}  ", crate::t!("help.filter_artist")),
        format!("  {}  ", crate::t!("help.queue_header")),
        format!("  x           {}  ", crate::t!("help.queue_play_next")),
        format!("  w           {}  ", crate::t!("help.queue_add")),
        format!("  u           {}  ", crate::t!("help.queue_view")),
        format!("  {}  ", crate::t!("help.playlist_header")),
        format!("  a           {}  ", crate::t!("help.playlist_add")),
        format!("  l           {}  ", crate::t!("help.playlist_manage")),
        format!("  c           {}  ", crate::t!("help.playlist_create")),
        format!("  d           {}  ", crate::t!("help.playlist_delete")),
        format!("  {}  ", crate::t!("help.system_header")),
        format!("  r           {}  ", crate::t!("help.refresh")),
        format!("  R           {}  ", crate::t!("help.config")),
        format!("  C           {}  ", crate::t!("help.cover")),
        format!("  L           {}  ", crate::t!("help.language")),
        format!("  h / ?       {}  ", crate::t!("help.help")),
        format!("  q / Esc     {}  ", crate::t!("help.quit")),
    ]
}

fn render_cover_overlay(frame: &mut Frame, area: Rect, app: &App) {
    match &app.cover_art {
        Some(cover) => render_cover_image(frame, area, cover, &app.cover_status),
        None => render_cover_status(frame, area, &app.cover_status),
    }
}

/// Render a status message when cover is loading or unavailable.
fn render_cover_status(frame: &mut Frame, area: Rect, status: &str) {
    let overlay_area = centered_rect(40, 20, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(format!(" {} ", crate::t!("help.title")))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let text = Paragraph::new(Text::from(Line::from(Span::styled(
        status,
        Style::new().fg(Color::White),
    ))))
    .block(block)
    .alignment(Alignment::Center);

    frame.render_widget(text, overlay_area);
}

/// Center a Rect of exact width × height within `area`.
fn centered_rect_exact(w: u16, h: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(h)) / 2),
            Constraint::Length(h),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(w)) / 2),
            Constraint::Length(w),
        ])
        .split(vert[1])[1]
}

/// Render the cover image using half-block characters (▀).
///
/// Each terminal cell displays two vertical pixels: the top pixel is the
/// foreground color, the bottom pixel is the background color — achieved by
/// rendering the ▀ (upper-half block) character with the appropriate colors.
fn render_cover_image(frame: &mut Frame, area: Rect, cover: &CoverArt, status: &str) {
    const MAX_COLS: usize = 30;

    // Compute display size in terminal cells (each cell = 2 vertical pixels).
    // Because each row renders 2 pixel rows, we need half as many terminal rows
    // as pixel rows to maintain the correct aspect ratio.
    let cw = cover.width as usize;
    let ch = cover.height as usize;
    let aspect = cw as f64 / ch as f64;
    let cols = MAX_COLS.min(cw);
    let rows = ((cols as f64) / (2.0 * aspect)).round() as usize;
    let pixel_rows = rows * 2;
    let cols = cols.max(1);
    let rows = rows.max(1);

    // Nearest-neighbour downscale to [pixel_rows × cols] pixels
    let mut downscaled = vec![[0u8; 3]; pixel_rows * cols];
    for py in 0..pixel_rows {
        for px in 0..cols {
            let src_x = px * cw / cols;
            let src_y = py * ch / pixel_rows;
            downscaled[py * cols + px] = cover.pixels[src_y * cw + src_x];
        }
    }

    // Overlay tightly sized to the image content + border, centered
    let overlay_w = ((cols + 2) as u16).min(area.width);   // +2 for left/right border
    let overlay_h = ((rows + 2) as u16).min(area.height);
    let overlay_area = centered_rect_exact(overlay_w, overlay_h, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(format!(" {} ", status))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let inner_area = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    // Build half-block lines
    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let r0 = row * 2;
        let r1 = (row * 2 + 1).min(pixel_rows - 1);
        let mut spans = Vec::with_capacity(cols);
        for col in 0..cols {
            let top = downscaled[r0 * cols + col];
            let bot = downscaled[r1 * cols + col];
            spans.push(Span::styled(
                "▀",
                Style::new()
                    .fg(Color::Rgb(top[0], top[1], top[2]))
                    .bg(Color::Rgb(bot[0], bot[1], bot[2])),
            ));
        }
        lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(Text::from(lines)).style(Style::new().bg(Color::Black));
    frame.render_widget(paragraph, inner_area);
}

fn render_help_overlay(frame: &mut Frame, area: Rect, _app: &App) {
    let overlay_area = centered_rect_lines(35, area);
    frame.render_widget(Clear, overlay_area);

    // Rebuild on every call: the function is only called when user presses `h`,
    // so caching adds complexity (language-switch invalidation) for negligible gain.
    let help_text = build_help_lines();

    let lines: Vec<Line> = help_text
        .iter()
        .map(|s| Line::from(Span::styled(s.as_str(), Style::new().fg(Color::White))))
        .collect();

    let paragraph = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(format!(" {} ", crate::t!("help.title")))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::new().fg(Color::Cyan))
                .style(Style::new().bg(Color::Black)),
        )
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, overlay_area);
}

fn render_config_overlay(frame: &mut Frame, area: Rect, app: &App) {
    if app.config_phase == 0 {
        render_config_list(frame, area, app);
    } else {
        render_config_edit(frame, area, app);
    }
}

fn render_config_list(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(60, 30, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(format!(" {} ", crate::t!("config.title_list")))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let mut lines: Vec<Line> = Vec::new();
    if app.config_servers.is_empty() {
        lines.push(Line::from(Span::styled(
            crate::tf!("config.empty"),
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
                format!(" [{}]", crate::t!("config.disabled"))
            } else {
                String::new()
            };
            let style = if cfg.disabled {
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

fn render_config_edit(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(70, 35, area);
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(format!(" {} ", crate::t!("config.title_edit")))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let fields: &[(&str, usize)] = &[
        (crate::t!("config.label_name"), 0),
        (crate::t!("config.label_type"), 1),
        (crate::t!("config.label_url"), 2),
        (crate::t!("config.label_user"), 3),
        (crate::t!("config.label_password"), 4),
    ];

    let mut lines = Vec::new();
    for &(label, idx) in fields {
        let is_focused = idx == app.config_focus;
        let value = if idx == 1 {
            let type_val = app.config_inputs.get(idx).map(|s| s.as_str()).unwrap_or("file-transfer");
            if is_focused {
                format!(" ◄ {} ► ", type_val)
            } else {
                format!(" {} ", type_val)
            }
        } else if idx == 4 && !is_focused {
            // safe: .get() returns None for missing/out-of-range idx → 0-length mask
            let len = app.config_inputs.get(idx).map(|s| s.len()).unwrap_or(0);
            "*".repeat(len.min(20))
        } else {
            app.config_inputs.get(idx).map(|s| s.as_str()).unwrap_or("").to_string()
        };
        let prefix = if is_focused { " ▸ " } else { "   " };
        let value_style = if is_focused {
            Style::new().fg(Color::White).bg(Color::Blue)
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
            format!("{}{}", prefix, crate::tf!("playlist.song_count", &pl.name, pl.songs.len())),
            style,
        )));
    }

    let block = Block::default()
        .title(format!(" {} ", crate::t!("playlist.add_title")))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let para = Paragraph::new(Text::from(lines))
        .block(block);
    frame.render_widget(para, overlay_area);
}

// ── Queue overlay ──

fn render_queue_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let overlay_area = centered_rect(70, 50, area);
    frame.render_widget(Clear, overlay_area);

    let mut lines: Vec<Line> = Vec::new();
    let current_label = app
        .player
        .current_track()
        .map(|t| crate::tf!("queue.current_label", &t.title))
        .unwrap_or_default();

    let queue_len = app.player.queue.len();
    lines.push(Line::from(Span::styled(
        format!(
            "  {}  —— {} ",
            current_label,
            if queue_len == 1 {
                crate::tf!("queue.count_single")
            } else {
                crate::tf!("queue.count_multi", queue_len)
            }
        ),
        Style::new().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::raw("")));

    if queue_len == 0 {
        lines.push(Line::from(Span::styled(
            crate::tf!("queue.empty_hint"),
            Style::new().fg(Color::Gray),
        )));
    } else {
        for (i, song) in app.player.queue.iter().enumerate() {
            let is_selected = i == app.queue_selected;
            let prefix = if is_selected { "▸ " } else { "  " };
            let artist = if song.artist == "<unknown>" || song.artist.is_empty() {
                crate::tf!("app.unknown_artist")
            } else {
                song.artist.clone()
            };
            let dur = format_duration(song.duration);
            let text = format!("{}{}  {}  {}", prefix, song.name, artist, dur);
            let style = if is_selected {
                Style::new().fg(Color::White).bg(Color::Blue)
            } else {
                Style::new().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(text, style)));
        }
    }

    lines.push(Line::from(Span::raw("")));
    lines.push(Line::from(Span::styled(
        crate::tf!("queue.hint"),
        Style::new().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(format!(" {} ", crate::t!("queue.title")))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let para = Paragraph::new(Text::from(lines))
        .block(block)
        .alignment(Alignment::Left);
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

// ── Quit confirmation overlay ──

fn render_quit_overlay(frame: &mut Frame, area: Rect, _app: &App) {
    let overlay_area = centered_rect(40, 20, area);
    frame.render_widget(Clear, overlay_area);

    let text = if crate::i18n::current() == crate::i18n::Language::En {
        " Quit? (y/N) "
    } else {
        " 确认退出？(y/N) "
    };

    let block = Block::default()
        .title(text)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::Cyan))
        .style(Style::new().bg(Color::Black));

    let paragraph = Paragraph::new(Text::from(""))
        .block(block);
    frame.render_widget(paragraph, overlay_area);
}
