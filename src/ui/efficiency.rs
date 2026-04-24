use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Gauge, Paragraph, Sparkline};
use ratatui::Frame;

use crate::app::App;
use crate::engine::scanner::FileCategory;
use crate::theme;

/// Render Tab 4: Token Efficiency Dashboard.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // stat boxes
            Constraint::Length(8), // sparkline
            Constraint::Min(1),    // category gauges
        ])
        .split(area);

    render_stat_boxes(frame, app, chunks[0]);
    render_sparkline(frame, app, chunks[1]);
    render_category_gauges(frame, app, chunks[2]);
}

fn render_stat_boxes(frame: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(area);

    // TOTAL SAVED
    let total_saved = app.total_tokens_saved;
    render_stat_box(
        frame,
        "TOTAL SAVED",
        &format_number(total_saved),
        "tokens",
        theme::GREEN,
        cols[0],
    );

    // FILES CONVERTED (cached in App, updated on refresh_scan)
    let converted_count = app.conversion_count;
    render_stat_box(
        frame,
        "FILES CONVERTED",
        &format!("{converted_count}"),
        "files",
        theme::CYAN,
        cols[1],
    );

    // AVG RATIO
    let ratio = app.estimated_savings_ratio();
    render_stat_box(
        frame,
        "AVG RATIO",
        &format!("{:.0}%", ratio * 100.0),
        "compression",
        theme::AMBER,
        cols[2],
    );

    // LAST RUN
    let last_run = app.last_scan_time.as_deref().unwrap_or("never");
    render_stat_box(frame, "LAST RUN", last_run, "", theme::AMBER_DIM, cols[3]);
}

fn render_stat_box(
    frame: &mut Frame,
    title: &str,
    value: &str,
    subtitle: &str,
    color: ratatui::style::Color,
    area: Rect,
) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {value}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("  {subtitle}"),
            Style::default().fg(theme::AMBER_DIM),
        )),
    ];

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER))
            .title(format!("[ {title} ]"))
            .title_style(
                Style::default()
                    .fg(theme::AMBER)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    frame.render_widget(paragraph, area);
}

fn render_sparkline(frame: &mut Frame, app: &App, area: Rect) {
    // Gather daily savings from DB for last 30 days
    let daily = app
        .db
        .as_ref()
        .and_then(|db| db.get_daily_savings(30).ok())
        .unwrap_or_default();

    let mut data: Vec<u64> = daily
        .iter()
        .map(|d| d.tokens_saved.unsigned_abs())
        .collect();

    // Pad to 30 if needed, reverse so oldest is first
    data.reverse();
    while data.len() < 30 {
        data.push(0);
    }
    data.truncate(30);

    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::BORDER))
                .title("[ TOKENS SAVED / DAY (30 DAYS) ]")
                .title_style(
                    Style::default()
                        .fg(theme::AMBER)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .data(&data)
        .style(Style::default().fg(theme::GREEN));

    frame.render_widget(sparkline, area);
}

fn render_category_gauges(frame: &mut Frame, app: &App, area: Rect) {
    let categories = [
        ("AGENTS", FileCategory::Agent),
        ("RULES", FileCategory::Rule),
        ("SKILLS", FileCategory::Skill),
        ("MEMORY", FileCategory::Memory),
        ("COMMANDS", FileCategory::Command),
    ];

    let constraints: Vec<Constraint> = categories.iter().map(|_| Constraint::Length(3)).collect();
    let mut all_constraints = constraints;
    all_constraints.push(Constraint::Min(0)); // absorb extra space

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(all_constraints)
        .split(area);

    for (i, (name, cat)) in categories.iter().enumerate() {
        let stats = app
            .category_stats
            .iter()
            .find(|(c, _)| c == cat)
            .map(|(_, s)| s);

        let (total, convertible) = if let Some(s) = stats {
            (s.file_count, s.convertible_count)
        } else {
            (0, 0)
        };

        let optimized = total.saturating_sub(convertible);
        let ratio = if total > 0 {
            optimized as f64 / total as f64
        } else {
            1.0
        };

        let label = format!(
            "{name}: {optimized}/{total} optimized ({:.0}%)",
            ratio * 100.0
        );

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::NONE))
            .gauge_style(Style::default().fg(theme::GREEN).bg(theme::BG))
            .ratio(ratio.clamp(0.0, 1.0))
            .label(Span::styled(
                label,
                Style::default()
                    .fg(theme::AMBER)
                    .add_modifier(Modifier::BOLD),
            ));

        frame.render_widget(gauge, rows[i]);
    }
}

fn format_number(n: i64) -> String {
    if n.abs() >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n.abs() >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{n}")
    }
}
