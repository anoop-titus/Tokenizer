use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Gauge, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::theme;

// ---------------------------------------------------------------------------
// Focus system (local to this module — not in app.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CompressFocus {
    FileList,
    Level,
    Apply,
    Optimize,
}

impl CompressFocus {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::FileList,
            1 => Self::Level,
            2 => Self::Apply,
            _ => Self::Optimize,
        }
    }

    #[allow(dead_code)]
    fn to_u8(self) -> u8 {
        match self {
            Self::FileList => 0,
            Self::Level => 1,
            Self::Apply => 2,
            Self::Optimize => 3,
        }
    }

    #[allow(dead_code)]
    fn next(self) -> Self {
        Self::from_u8((self.to_u8() + 1) % 4)
    }
}

// ---------------------------------------------------------------------------
// Public action enum + input handler
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum CompressAction {
    Up,
    Down,
    Left,
    Right,
    Toggle,
    CycleFocus,
}

/// Handle input for the Compress tab. Returns true if consumed.
#[allow(dead_code)]
pub fn handle_input(app: &mut App, action: CompressAction) -> bool {
    let focus = CompressFocus::from_u8(app.compress_focus);

    match action {
        CompressAction::CycleFocus => {
            app.compress_focus = focus.next().to_u8();
            true
        }
        CompressAction::Up => match focus {
            CompressFocus::FileList => {
                app.scroll_compression(-1);
                true
            }
            CompressFocus::Apply => {
                // Move focus to Level
                app.compress_focus = CompressFocus::Level.to_u8();
                true
            }
            CompressFocus::Optimize => {
                app.compress_focus = CompressFocus::Apply.to_u8();
                true
            }
            _ => true,
        },
        CompressAction::Down => match focus {
            CompressFocus::FileList => {
                app.scroll_compression(1);
                true
            }
            CompressFocus::Level => {
                app.compress_focus = CompressFocus::Apply.to_u8();
                true
            }
            CompressFocus::Apply => {
                app.compress_focus = CompressFocus::Optimize.to_u8();
                true
            }
            _ => true,
        },
        CompressAction::Left => {
            if focus == CompressFocus::Level {
                app.adjust_compression_level(-1);
            }
            true
        }
        CompressAction::Right => {
            if focus == CompressFocus::Level {
                app.adjust_compression_level(1);
            }
            true
        }
        CompressAction::Toggle => match focus {
            CompressFocus::Apply => {
                // Save compression override for selected file
                if let Some(&scan_idx) = app.compression_file_indices.get(app.compression_selected)
                {
                    if let Some(file) = app.scan_result.get(scan_idx) {
                        let level = app.get_selected_compression_level();
                        app.compression_overrides.insert(file.path.clone(), level);
                        app.settings_status = Some(format!("Level {} saved for file", level));
                    }
                }
                true
            }
            CompressFocus::Optimize => {
                app.settings_status = Some("Optimization started...".to_string());
                true
            }
            _ => true,
        },
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render Tab 3: Compression Controls — file list + detail panel + optimize button.
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(outer[0]);

    render_file_list(frame, app, panels[0]);
    render_detail_panel(frame, app, panels[1]);
    render_optimize_button(frame, app, outer[1]);
}

// ---------------------------------------------------------------------------
// File list (left 40%)
// ---------------------------------------------------------------------------

fn render_file_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let focus = CompressFocus::from_u8(app.compress_focus);

    // Collect ALL non-whitelisted files (not just md)
    let files: Vec<(usize, String, String, &str)> = app
        .scan_result
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_whitelisted)
        .map(|(i, f)| {
            let name = f
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            let cat = format!("{}", f.category);
            let fmt = f.current_format.as_str();
            let suffix = if f.is_optimized {
                "done"
            } else if fmt == "toon" {
                "toon"
            } else if fmt == "json" || fmt == "jsonl" {
                "to_toon"
            } else {
                "md"
            };
            (i, name, cat, suffix)
        })
        .collect();

    let items: Vec<ListItem> = files
        .iter()
        .enumerate()
        .map(|(list_idx, (_, name, cat, suffix))| {
            let selected = app.compression_selected == list_idx && focus == CompressFocus::FileList;
            let (name_style, tag) = match *suffix {
                "done" => (
                    Style::default()
                        .fg(theme::AMBER_DIM)
                        .add_modifier(Modifier::DIM),
                    Span::styled(
                        " [DONE]",
                        Style::default()
                            .fg(theme::AMBER_DIM)
                            .add_modifier(Modifier::DIM),
                    ),
                ),
                "toon" => (
                    Style::default()
                        .fg(theme::AMBER_DIM)
                        .add_modifier(Modifier::DIM),
                    Span::styled(
                        " [TOON]",
                        Style::default()
                            .fg(theme::AMBER_DIM)
                            .add_modifier(Modifier::DIM),
                    ),
                ),
                "to_toon" => (
                    Style::default().fg(theme::AMBER),
                    Span::styled(
                        " [->TOON]",
                        Style::default()
                            .fg(theme::CYAN)
                            .add_modifier(Modifier::BOLD),
                    ),
                ),
                _ => (Style::default().fg(theme::AMBER), Span::raw("")),
            };

            let row_style = if selected {
                Style::default()
                    .fg(theme::CYAN)
                    .bg(theme::HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                name_style
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{name:<30}"), row_style),
                Span::styled(format!(" [{cat}]"), Style::default().fg(theme::AMBER_DIM)),
                tag,
            ]))
        })
        .collect();

    // Store index mapping
    app.compression_file_indices = files.iter().map(|(i, _, _, _)| *i).collect();

    let border_color = if focus == CompressFocus::FileList {
        theme::CYAN
    } else {
        theme::BORDER
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .title(format!("[ FILES ({}) ]", files.len()))
                .title_style(
                    Style::default()
                        .fg(theme::AMBER)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .fg(theme::CYAN)
                .bg(theme::HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(list, area, &mut app.compression_list_state);
}

// ---------------------------------------------------------------------------
// Detail panel (right 60%)
// ---------------------------------------------------------------------------

fn render_detail_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // compression gauge + apply button
            Constraint::Min(1),    // preview
        ])
        .split(area);

    render_compression_gauge(frame, app, chunks[0]);
    render_preview(frame, &mut *app, chunks[1]);
}

fn render_compression_gauge(frame: &mut Frame, app: &App, area: Rect) {
    let focus = CompressFocus::from_u8(app.compress_focus);
    let level = app.get_selected_compression_level();
    let label = match level {
        1 => "Light",
        2 => "Medium",
        3 => "Heavy",
        _ => "Maximum",
    };

    let filled = level as usize;
    let empty = 4 - filled;
    let bar = format!(
        "[{}{}]",
        "\u{2588}".repeat(filled * 2),
        "\u{2591}".repeat(empty * 2)
    );

    let level_border = if focus == CompressFocus::Level {
        theme::CYAN
    } else {
        theme::BORDER
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  LEVEL: ", Style::default().fg(theme::CYAN)),
            Span::styled(
                format!("{level} - {label}"),
                Style::default()
                    .fg(theme::AMBER)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                bar,
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Left/Right to adjust",
                Style::default().fg(theme::AMBER_DIM),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            super::button::inline_span(
                "APPLY LEVEL",
                focus == CompressFocus::Apply,
                super::button::ButtonIntent::Success,
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(level_border))
            .title("[ COMPRESSION LEVEL ]")
            .title_style(
                Style::default()
                    .fg(theme::AMBER)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    frame.render_widget(paragraph, area);
}

fn render_preview(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let (before_lines, after_lines) = app.get_preview();

    let before_text: Vec<Line> = before_lines
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                truncate_line(l, chunks[0].width.saturating_sub(3) as usize),
                Style::default().fg(theme::MAGENTA),
            ))
        })
        .collect();

    let after_text: Vec<Line> = after_lines
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                truncate_line(l, chunks[1].width.saturating_sub(3) as usize),
                Style::default().fg(theme::GREEN),
            ))
        })
        .collect();

    let before_para = Paragraph::new(before_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER))
            .title("[ BEFORE ]")
            .title_style(
                Style::default()
                    .fg(theme::MAGENTA)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    let after_para = Paragraph::new(after_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER))
            .title("[ AFTER ]")
            .title_style(
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    frame.render_widget(before_para, chunks[0]);
    frame.render_widget(after_para, chunks[1]);
}

// ---------------------------------------------------------------------------
// Optimize button (full width, bottom)
// ---------------------------------------------------------------------------

fn render_optimize_button(frame: &mut Frame, app: &mut App, area: Rect) {
    // Store rect for mouse click detection
    app.click_areas.optimize_button = area;

    // Show progress bar if optimization is running
    if let Some(ref progress) = app.optimize_progress {
        let ratio = if progress.total > 0 {
            progress.current as f64 / progress.total as f64
        } else {
            0.0
        };
        let label = format!(
            "OPTIMIZING... {}/{} files",
            progress.current, progress.total
        );
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme::CYAN)),
            )
            .gauge_style(Style::default().fg(theme::GREEN).bg(theme::BG))
            .ratio(ratio.clamp(0.0, 1.0))
            .label(Span::styled(
                label,
                Style::default()
                    .fg(theme::AMBER)
                    .add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(gauge, area);
        return;
    }

    let focus = CompressFocus::from_u8(app.compress_focus);
    let focused = focus == CompressFocus::Optimize;
    super::button::render(
        frame,
        area,
        "OPTIMIZE NOW",
        focused,
        super::button::ButtonIntent::Primary,
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate_line(line: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut display_width = 0;
    let mut byte_end = 0;
    for ch in line.chars() {
        let ch_width = if ch.is_ascii() { 1 } else { 2 };
        if display_width + ch_width > max_width {
            break;
        }
        display_width += ch_width;
        byte_end += ch.len_utf8();
    }
    if byte_end >= line.len() {
        line.to_string()
    } else if max_width > 3 {
        let mut trunc_width = 0;
        let mut trunc_end = 0;
        for ch in line.chars() {
            let ch_width = if ch.is_ascii() { 1 } else { 2 };
            if trunc_width + ch_width > max_width.saturating_sub(3) {
                break;
            }
            trunc_width += ch_width;
            trunc_end += ch.len_utf8();
        }
        format!("{}...", &line[..trunc_end])
    } else {
        line[..byte_end].to_string()
    }
}
