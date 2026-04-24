use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::config::TargetFormat;
use crate::theme;

/// All frequency options in minutes.
const FREQUENCY_OPTIONS: &[u32] = &[30, 60, 120, 180];

/// Category names in display order.
const CATEGORY_NAMES: &[&str] = &["agents", "rules", "skills", "memory", "commands"];

/// Total rows: frequency + auto-optimize + 5 categories + compression + install-timer + install-hook + apply = 11.
pub const SETTINGS_ROW_COUNT: usize = 11;

/// Render Tab 5: Settings Form.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let inner_constraints: Vec<Constraint> = (0..SETTINGS_ROW_COUNT)
        .map(|_| Constraint::Length(2))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(inner_constraints)
        .split(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::BORDER))
                .title("[ SETTINGS ]")
                .title_style(
                    Style::default()
                        .fg(theme::AMBER)
                        .add_modifier(Modifier::BOLD),
                )
                .inner(area),
        );

    // Draw outer block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .title("[ SETTINGS ]")
        .title_style(
            Style::default()
                .fg(theme::AMBER)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(block, area);

    let sel = app.settings_selected;

    // Row 0: FREQUENCY
    {
        let freq = app.config.frequency_minutes;
        let line = Line::from(vec![
            Span::styled("  FREQUENCY         ", label_style(sel == 0)),
            Span::styled(format!("< {freq} MIN >"), value_style(sel == 0)),
        ]);
        frame.render_widget(Paragraph::new(line).style(row_bg(sel == 0)), rows[0]);
    }

    // Row 1: AUTO-OPTIMIZE POST-SESSION
    {
        let checked = if app.config.auto_optimize_post_session {
            "[x]"
        } else {
            "[ ]"
        };
        let line = Line::from(vec![
            Span::styled("  AUTO-OPTIMIZE     ", label_style(sel == 1)),
            Span::styled(checked, value_style(sel == 1)),
            Span::styled(" Post-Session", Style::default().fg(theme::AMBER_DIM)),
        ]);
        frame.render_widget(Paragraph::new(line).style(row_bg(sel == 1)), rows[1]);
    }

    // Rows 2-6: Category toggles
    for (i, name) in CATEGORY_NAMES.iter().enumerate() {
        let row_idx = 2 + i;
        let entry = app.config.categories.get(*name);
        let enabled = entry.map(|e| e.enabled).unwrap_or(true);
        let fmt = entry.map(|e| e.target_format).unwrap_or(TargetFormat::Toon);

        let checkbox = if enabled { "[x]" } else { "[ ]" };
        let fmt_label = match fmt {
            TargetFormat::Toon => "[TOON]",
            TargetFormat::Json => "[JSON]",
            TargetFormat::Jsonl => "[JSONL]",
        };

        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(checkbox, value_style(sel == row_idx)),
            Span::styled(
                format!(" {:<16}", name.to_uppercase()),
                label_style(sel == row_idx),
            ),
            Span::styled(format!("< {fmt_label} >"), value_style(sel == row_idx)),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(row_bg(sel == row_idx)),
            rows[row_idx],
        );
    }

    // Row 7: COMPRESSION DEFAULT
    {
        let level = app.config.compression_default;
        let level_name = match level {
            1 => "Light",
            2 => "Medium",
            3 => "Heavy",
            _ => "Maximum",
        };
        let line = Line::from(vec![
            Span::styled("  COMPRESSION       ", label_style(sel == 7)),
            Span::styled(format!("< {level} - {level_name} >"), value_style(sel == 7)),
        ]);
        frame.render_widget(Paragraph::new(line).style(row_bg(sel == 7)), rows[7]);
    }

    // Row 8: INSTALL TIMER button
    {
        let timer_status = if app.timer_installed { " ACTIVE" } else { "" };
        let line = Line::from(vec![
            Span::raw("  "),
            super::button::inline_span("INSTALL TIMER", sel == 8, super::button::ButtonIntent::Primary),
            Span::styled(timer_status, Style::default().fg(theme::GREEN)),
        ]);
        frame.render_widget(Paragraph::new(line).style(row_bg(sel == 8)), rows[8]);
    }

    // Row 9: INSTALL HOOK button
    {
        let hook_status = if app.hook_installed { " ACTIVE" } else { "" };
        let line = Line::from(vec![
            Span::raw("  "),
            super::button::inline_span("INSTALL HOOK", sel == 9, super::button::ButtonIntent::Primary),
            Span::styled(hook_status, Style::default().fg(theme::GREEN)),
        ]);
        frame.render_widget(Paragraph::new(line).style(row_bg(sel == 9)), rows[9]);
    }

    // Row 10: APPLY CHANGES button
    {
        let line = Line::from(vec![
            Span::raw("  "),
            super::button::inline_span("APPLY CHANGES", sel == 10, super::button::ButtonIntent::Success),
        ]);
        frame.render_widget(Paragraph::new(line).style(row_bg(sel == 10)), rows[10]);
    }
}

fn label_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme::CYAN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::AMBER)
    }
}

fn value_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme::GREEN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::AMBER_DIM)
    }
}

fn row_bg(selected: bool) -> Style {
    if selected {
        Style::default().bg(theme::HIGHLIGHT_BG)
    } else {
        Style::default().bg(theme::BG)
    }
}


/// Handle Left/Right/Space/Enter input for the settings tab.
/// Returns true if the event was consumed.
pub fn handle_input(app: &mut App, action: SettingsAction) -> bool {
    let sel = app.settings_selected;
    match action {
        SettingsAction::Up => {
            if sel > 0 {
                app.settings_selected -= 1;
            }
            true
        }
        SettingsAction::Down => {
            if sel < SETTINGS_ROW_COUNT - 1 {
                app.settings_selected += 1;
            }
            true
        }
        SettingsAction::Left => {
            match sel {
                0 => cycle_frequency(&mut app.config.frequency_minutes, false),
                7 => {
                    if app.config.compression_default > 1 {
                        app.config.compression_default -= 1;
                    }
                }
                2..=6 => cycle_format(app, sel - 2, false),
                _ => {}
            }
            true
        }
        SettingsAction::Right => {
            match sel {
                0 => cycle_frequency(&mut app.config.frequency_minutes, true),
                7 => {
                    if app.config.compression_default < 4 {
                        app.config.compression_default += 1;
                    }
                }
                2..=6 => cycle_format(app, sel - 2, true),
                _ => {}
            }
            true
        }
        SettingsAction::Toggle => {
            match sel {
                1 => {
                    app.config.auto_optimize_post_session = !app.config.auto_optimize_post_session;
                }
                2..=6 => {
                    let name = CATEGORY_NAMES[sel - 2];
                    if let Some(entry) = app.config.categories.get_mut(name) {
                        entry.enabled = !entry.enabled;
                    }
                }
                8 => {
                    // Install timer
                    match crate::daemon::install_timer() {
                        Ok(()) => {
                            app.timer_installed = true;
                            app.settings_status = Some("Timer installed".to_string());
                        }
                        Err(e) => {
                            app.settings_status = Some(format!("Timer error: {e}"));
                        }
                    }
                }
                9 => {
                    // Install hook
                    match crate::daemon::install_hook() {
                        Ok(()) => {
                            app.hook_installed = true;
                            app.settings_status = Some("Hook installed".to_string());
                        }
                        Err(e) => {
                            app.settings_status = Some(format!("Hook error: {e}"));
                        }
                    }
                }
                10 => {
                    // Apply: save config
                    let _ = app.config.save();
                    app.settings_status = Some("Settings saved".to_string());
                }
                _ => {}
            }
            true
        }
    }
}

fn cycle_frequency(freq: &mut u32, forward: bool) {
    let idx = FREQUENCY_OPTIONS
        .iter()
        .position(|&f| f == *freq)
        .unwrap_or(1);
    let next = if forward {
        (idx + 1) % FREQUENCY_OPTIONS.len()
    } else if idx == 0 {
        FREQUENCY_OPTIONS.len() - 1
    } else {
        idx - 1
    };
    *freq = FREQUENCY_OPTIONS[next];
}

fn cycle_format(app: &mut App, cat_idx: usize, forward: bool) {
    let name = CATEGORY_NAMES[cat_idx];
    if let Some(entry) = app.config.categories.get_mut(name) {
        entry.target_format = match (entry.target_format, forward) {
            (TargetFormat::Toon, true) => TargetFormat::Json,
            (TargetFormat::Json, true) => TargetFormat::Jsonl,
            (TargetFormat::Jsonl, true) => TargetFormat::Toon,
            (TargetFormat::Toon, false) => TargetFormat::Jsonl,
            (TargetFormat::Json, false) => TargetFormat::Toon,
            (TargetFormat::Jsonl, false) => TargetFormat::Json,
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SettingsAction {
    Up,
    Down,
    Left,
    Right,
    Toggle,
}
