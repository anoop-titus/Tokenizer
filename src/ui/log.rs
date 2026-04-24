use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::theme;

/// Render Tab 2: Change Log — conversion history from SQLite.
pub fn render(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let records = app
        .db
        .as_ref()
        .and_then(|db| db.get_conversions(200).ok())
        .unwrap_or_default();

    let header = Row::new(vec![
        Cell::from("TIMESTAMP").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("FILE PATH").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("ACTION").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("SIZE DELTA").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("TOKENS SAVED").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .height(1);

    let rows: Vec<Row> = records
        .iter()
        .enumerate()
        .map(|(i, rec)| {
            let bg = if i % 2 == 0 {
                theme::BG
            } else {
                theme::ROW_ALT
            };

            let delta = rec.original_bytes - rec.converted_bytes;
            let (delta_str, delta_color) = if delta >= 0 {
                (format!("-{} B", delta), theme::GREEN)
            } else {
                (format!("+{} B", delta.abs()), theme::MAGENTA)
            };

            let (tok_str, tok_color) = if rec.tokens_saved >= 0 {
                (format!("+{}", rec.tokens_saved), theme::GREEN)
            } else {
                (format!("{}", rec.tokens_saved), theme::MAGENTA)
            };

            // Truncate timestamp to 19 chars
            let ts = if rec.timestamp.len() > 19 {
                &rec.timestamp[..19]
            } else {
                &rec.timestamp
            };

            // Truncate file path for display (char-safe)
            let path_display = if rec.file_path.chars().count() > 50 {
                let tail: String = rec
                    .file_path
                    .chars()
                    .rev()
                    .take(47)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                format!("...{tail}")
            } else {
                rec.file_path.clone()
            };

            Row::new(vec![
                Cell::from(ts.to_string()),
                Cell::from(path_display),
                Cell::from(rec.action.clone()),
                Cell::from(delta_str).style(Style::default().fg(delta_color)),
                Cell::from(tok_str).style(Style::default().fg(tok_color)),
            ])
            .style(Style::default().fg(theme::AMBER).bg(bg))
        })
        .collect();

    let row_count = rows.len();

    let widths = [
        Constraint::Length(19),
        Constraint::Percentage(40),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::BORDER))
                .title(format!("[ CHANGE LOG ({row_count}) ]"))
                .title_style(
                    Style::default()
                        .fg(theme::AMBER)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .row_highlight_style(
            Style::default()
                .bg(theme::HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(table, area, &mut app.log_table_state);
}
