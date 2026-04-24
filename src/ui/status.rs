use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Cell, Gauge, Row, Table};
use ratatui::Frame;

use super::format_bytes;
use crate::app::App;
use crate::engine::scanner::FileCategory;
use crate::theme;

/// Render the Status tab: Before/After split with compression gauge + refresh button.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),    // before/after panels
            Constraint::Length(3), // gauge
            Constraint::Length(3), // refresh button
        ])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    render_before_panel(frame, app, panels[0]);
    render_after_panel(frame, app, panels[1]);
    render_gauge(frame, app, chunks[1]);
    render_refresh_button(frame, chunks[2]);
}

fn render_refresh_button(frame: &mut Frame, area: Rect) {
    // Refresh is always shown as a secondary-intent filled button at rest.
    // (It doesn't receive keyboard focus; it's mouse-clickable.)
    super::button::render(
        frame,
        area,
        "REFRESH SCAN",
        false,
        super::button::ButtonIntent::Neutral,
    );
}

fn render_before_panel(frame: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("CATEGORY").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("FILES").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("BYTES").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("TOKENS").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .height(1);

    let rows: Vec<Row> = app
        .category_stats
        .iter()
        .enumerate()
        .map(|(i, (cat, stats))| {
            let bg = if i % 2 == 0 {
                theme::BG
            } else {
                theme::ROW_ALT
            };
            let fg = match cat {
                FileCategory::Whitelisted => theme::AMBER_DIM,
                _ => theme::AMBER,
            };
            Row::new(vec![
                Cell::from(format!("{cat}")),
                Cell::from(format!("{}", stats.file_count)),
                Cell::from(format_bytes(stats.total_bytes)),
                Cell::from(format!("{}", stats.total_tokens)),
            ])
            .style(Style::default().fg(fg).bg(bg))
        })
        .collect();

    // Totals row
    let total_files: usize = app.category_stats.iter().map(|(_, s)| s.file_count).sum();
    let total_bytes: u64 = app.category_stats.iter().map(|(_, s)| s.total_bytes).sum();
    let total_tokens: u64 = app.category_stats.iter().map(|(_, s)| s.total_tokens).sum();

    let mut all_rows = rows;
    all_rows.push(
        Row::new(vec![
            Cell::from("TOTAL").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from(format!("{total_files}"))
                .style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from(format_bytes(total_bytes))
                .style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from(format!("{total_tokens}"))
                .style(Style::default().add_modifier(Modifier::BOLD)),
        ])
        .style(Style::default().fg(theme::CYAN)),
    );

    let widths = [
        Constraint::Percentage(35),
        Constraint::Percentage(15),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ];

    let table = Table::new(all_rows, widths).header(header).block(
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

    frame.render_widget(table, area);
}

fn render_after_panel(frame: &mut Frame, app: &App, area: Rect) {
    let ratio = app.estimated_savings_ratio();

    let header = Row::new(vec![
        Cell::from("CATEGORY").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("CONV").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("PROJ BYTES").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("PROJ TOKENS").style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .height(1);

    let rows: Vec<Row> = app
        .category_stats
        .iter()
        .enumerate()
        .map(|(i, (cat, stats))| {
            let bg = if i % 2 == 0 {
                theme::BG
            } else {
                theme::ROW_ALT
            };
            let savings_bytes = (stats.convertible_bytes as f64 * ratio) as u64;
            let projected_bytes = stats.total_bytes.saturating_sub(savings_bytes);
            let projected_tokens = projected_bytes / 4;
            let fg = match cat {
                FileCategory::Whitelisted => theme::AMBER_DIM,
                _ => theme::GREEN,
            };
            Row::new(vec![
                Cell::from(format!("{cat}")),
                Cell::from(format!("{}", stats.convertible_count)),
                Cell::from(format_bytes(projected_bytes)),
                Cell::from(format!("{projected_tokens}")),
            ])
            .style(Style::default().fg(fg).bg(bg))
        })
        .collect();

    let total_convertible: usize = app
        .category_stats
        .iter()
        .map(|(_, s)| s.convertible_count)
        .sum();
    let total_bytes: u64 = app.category_stats.iter().map(|(_, s)| s.total_bytes).sum();
    let total_convertible_bytes: u64 = app
        .category_stats
        .iter()
        .map(|(_, s)| s.convertible_bytes)
        .sum();
    let total_savings = (total_convertible_bytes as f64 * ratio) as u64;
    let projected_total = total_bytes.saturating_sub(total_savings);
    let projected_tokens = projected_total / 4;

    let mut all_rows = rows;
    all_rows.push(
        Row::new(vec![
            Cell::from("TOTAL").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from(format!("{total_convertible}"))
                .style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from(format_bytes(projected_total))
                .style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from(format!("{projected_tokens}"))
                .style(Style::default().add_modifier(Modifier::BOLD)),
        ])
        .style(Style::default().fg(theme::CYAN)),
    );

    let widths = [
        Constraint::Percentage(35),
        Constraint::Percentage(15),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ];

    let table = Table::new(all_rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER))
            .title("[ AFTER (PROJECTED) ]")
            .title_style(
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    frame.render_widget(table, area);
}

fn render_gauge(frame: &mut Frame, app: &App, area: Rect) {
    let total_bytes = app.total_original_bytes();
    let convertible_bytes: u64 = app
        .category_stats
        .iter()
        .map(|(_, s)| s.convertible_bytes)
        .sum();
    let savings = (convertible_bytes as f64 * app.estimated_savings_ratio()) as u64;

    let ratio = if total_bytes > 0 {
        (total_bytes.saturating_sub(savings)) as f64 / total_bytes as f64
    } else {
        1.0
    };

    let label = format!(
        "{} convertible files | ~{} bytes saveable | ~{} tokens saveable",
        app.total_convertible_files(),
        format_bytes(savings),
        savings / 4,
    );

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::BORDER))
                .title("[ COMPRESSION RATIO ]")
                .title_style(
                    Style::default()
                        .fg(theme::AMBER)
                        .add_modifier(Modifier::BOLD),
                ),
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
}
