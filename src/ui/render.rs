use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListDirection, ListItem, Paragraph,
    },
    Frame,
};

use crate::player::PlayerState;

use super::{App, PlayingSource, SortMode, ViewMode};

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
    let items: Vec<ListItem> = app
        .filtered_music
        .iter()
        .enumerate()
        .map(|(i, music)| {
            let is_selected = Some(i) == selected;
            let name = music.name.clone();
            let artist = if music.artist == "<unknown>" || music.artist.is_empty() {
                crate::tf!("app.unknown_artist")
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
        format!(" {} ({}) ", crate::t!("view.browse"), items.len())
    } else {
        format!(" {} ", crate::tf!("view.search_results", &app.search_query, items.len()))
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
    if selected.map_or(false, |s| s >= songs.len()) {
        app.pl_content_state.select(Some(songs.len().saturating_sub(1)));
    }

    let items: Vec<ListItem> = songs
        .iter()
        .enumerate()
        .map(|(i, music)| {
            let is_selected = Some(i) == selected;
            let name = music.name.clone();
            let artist = if music.artist == "<unknown>" || music.artist.is_empty() {
                crate::tf!("app.unknown_artist")
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
        let pl_name = app.playlists.get(pl_idx).map(|p| p.name.as_str()).unwrap_or("");
        format!(" {} ", pl_name)
    } else {
        format!(" {} ", crate::tf!("view.search_results", &app.search_query, items.len()))
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

    frame.render_stateful_widget(list, area, &mut app.pl_content_state);
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

fn render_help_overlay(frame: &mut Frame, area: Rect, _app: &App) {
    let overlay_area = centered_rect_lines(27, area);
    frame.render_widget(Clear, overlay_area);

    let box_w = "══════════════════════════════════════";
    let help_text: Vec<String> = if crate::i18n::current() == crate::i18n::Language::En {
        vec![
            format!("╔{}╗", box_w),
            format!("║{:^38}║", crate::t!("help.title")),
            format!("╠{}╣", box_w),
            format!("║  {}  ║", crate::t!("help.playback_header")),
            format!("║  Enter       {}  ║", crate::t!("help.play")),
            format!("║  Space       {}  ║", crate::t!("help.toggle")),
            format!("║  s           {}  ║", crate::t!("help.stop")),
            format!("║  ← / →       {}  ║", crate::t!("help.seek")),
            format!("║  m           {}  ║", crate::t!("help.play_mode")),
            format!("║  + / =       {}  ║", crate::t!("help.volume")),
            format!("║  - / _        {}  ║", ""),
            format!("║  {}  ║", crate::t!("help.nav_header")),
            format!("║  ↑ / ↓       {}  ║", crate::t!("help.nav_up_down")),
            format!("║  PgUp/PgDn   {}  ║", crate::t!("help.nav_page")),
            format!("║  g           {}  ║", crate::t!("help.goto_playing")),
            format!("║  /           {}  ║", crate::t!("help.search")),
            format!("║  {}  ║", crate::t!("help.queue_header")),
            format!("║  x           {}  ║", crate::t!("help.queue_play_next")),
            format!("║  w           {}  ║", crate::t!("help.queue_add")),
            format!("║  u           {}  ║", crate::t!("help.queue_view")),
            format!("║  {}  ║", crate::t!("help.playlist_header")),
            format!("║  a           {}  ║", crate::t!("help.playlist_add")),
            format!("║  l           {}  ║", crate::t!("help.playlist_manage")),
            format!("║  c           {}  ║", crate::t!("help.playlist_create")),
            format!("║  d           {}  ║", crate::t!("help.playlist_delete")),
            format!("║  {}  ║", crate::t!("help.system_header")),
            format!("║  r           {}  ║", crate::t!("help.refresh")),
            format!("║  R           {}  ║", crate::t!("help.config")),
            format!("║  L           {}  ║", crate::t!("help.language")),
            format!("║  h / ?       {}  ║", crate::t!("help.help")),
            format!("║  q / Esc     {}  ║", crate::t!("help.quit")),
            format!("╚{}╝", box_w),
        ]
    } else {
        vec![
            format!("╔{}╗", box_w),
            format!("║{:^38}║", crate::t!("help.title")),
            format!("╠{}╣", box_w),
            format!("║  {}  ║", crate::t!("help.playback_header")),
            format!("║  Enter       {}  ║", crate::t!("help.play")),
            format!("║  Space       {}  ║", crate::t!("help.toggle")),
            format!("║  s           {}  ║", crate::t!("help.stop")),
            format!("║  ← / →       {}  ║", crate::t!("help.seek")),
            format!("║  m           {}  ║", crate::t!("help.play_mode")),
            format!("║  + / =       {}  ║", crate::t!("help.volume")),
            format!("║  - / _        {}  ║", ""),
            format!("║  {}  ║", crate::t!("help.nav_header")),
            format!("║  ↑ / ↓       {}  ║", crate::t!("help.nav_up_down")),
            format!("║  PgUp/PgDn   {}  ║", crate::t!("help.nav_page")),
            format!("║  g           {}  ║", crate::t!("help.goto_playing")),
            format!("║  /           {}  ║", crate::t!("help.search")),
            format!("║  {}  ║", crate::t!("help.queue_header")),
            format!("║  x           {}  ║", crate::t!("help.queue_play_next")),
            format!("║  w           {}  ║", crate::t!("help.queue_add")),
            format!("║  u           {}  ║", crate::t!("help.queue_view")),
            format!("║  {}  ║", crate::t!("help.playlist_header")),
            format!("║  a           {}  ║", crate::t!("help.playlist_add")),
            format!("║  l           {}  ║", crate::t!("help.playlist_manage")),
            format!("║  c           {}  ║", crate::t!("help.playlist_create")),
            format!("║  d           {}  ║", crate::t!("help.playlist_delete")),
            format!("║  {}  ║", crate::t!("help.system_header")),
            format!("║  r           {}  ║", crate::t!("help.refresh")),
            format!("║  R           {}  ║", crate::t!("help.config")),
            format!("║  L           {}  ║", crate::t!("help.language")),
            format!("║  h / ?       {}  ║", crate::t!("help.help")),
            format!("║  q / Esc     {}  ║", crate::t!("help.quit")),
            format!("╚{}╝", box_w),
        ]
    };

    let lines: Vec<Line> = help_text
        .iter()
        .map(|s| {
            let is_header = s.starts_with('╔') || s.starts_with('╚') || s.starts_with('╠');
            let style = if is_header {
                Style::new().fg(Color::Cyan)
            } else {
                Style::new().fg(Color::White)
            };
            Line::from(Span::styled(s.as_str(), style))
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
        ("URL", 2),
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
