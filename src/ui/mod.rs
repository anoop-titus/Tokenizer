pub mod button;
pub mod compression;
pub mod efficiency;
pub mod log;
pub mod restructure;
pub mod settings;
pub mod status;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Tab};
use crate::theme;

/// Master render function: tab bar + active tab content + status bar.
pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(1),    // content
            Constraint::Length(1), // status bar
        ])
        .split(size);

    // Store content area for mouse hit testing.
    app.click_areas.content_area = chunks[1];

    // Reset per-tab button rects every frame. Each tab's render fn re-sets
    // its own; anything not re-set this frame won't fire on click.
    app.click_areas.optimize_button = Rect::default();
    app.click_areas.restructure_button = Rect::default();

    render_tab_bar(frame, app, chunks[0]);
    render_active_tab(frame, app, chunks[1]);
    render_status_bar(frame, app, chunks[2]);

    // Popup overlay (rendered last, on top)
    if app.popup.is_some() {
        render_popup(frame, app, size);
    } else {
        // Reset stale click rects so misclicks don't fire.
        app.click_areas.popup_yes = Rect::default();
        app.click_areas.popup_no = Rect::default();
    }
}

fn render_popup(frame: &mut Frame, app: &mut App, area: Rect) {
    let popup = app.popup.as_ref().expect("popup checked above").clone();
    let is_prompt = matches!(popup.kind, crate::app::PopupKind::UpdatePrompt { .. });

    // Reserve room for two button rows when this is a prompt.
    let extra = if is_prompt { 5 } else { 4 };
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height =
        (popup.lines.len() as u16 + extra).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    for l in &popup.lines {
        lines.push(Line::from(Span::styled(
            format!("  {l}"),
            Style::default().fg(theme::GREEN),
        )));
    }
    lines.push(Line::from(""));
    if !is_prompt {
        lines.push(Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(theme::AMBER_DIM),
        )));
    }

    let border_color = if is_prompt {
        theme::AMBER_BRIGHT
    } else {
        theme::CYAN
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(format!("[ {} ]", popup.title))
        .title_style(
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme::BG));

    let inner = block.inner(popup_area);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup_area);

    if is_prompt {
        // Draw two clickable buttons at the bottom of the inner popup area.
        let btn_h: u16 = 3;
        let btn_w: u16 = 14u16.min(inner.width / 2);
        let gap: u16 = 2;
        let total_w = btn_w * 2 + gap;
        let btn_y = inner.y + inner.height.saturating_sub(btn_h);
        let start_x = inner.x + (inner.width.saturating_sub(total_w)) / 2;

        let yes_rect = Rect::new(start_x, btn_y, btn_w, btn_h);
        let no_rect = Rect::new(start_x + btn_w + gap, btn_y, btn_w, btn_h);

        crate::ui::button::render(
            frame,
            yes_rect,
            "YES (y)",
            true,
            crate::ui::button::ButtonIntent::Success,
        );
        crate::ui::button::render(
            frame,
            no_rect,
            "NO (n)",
            false,
            crate::ui::button::ButtonIntent::Neutral,
        );

        app.click_areas.popup_yes = yes_rect;
        app.click_areas.popup_no = no_rect;
    } else {
        app.click_areas.popup_yes = Rect::default();
        app.click_areas.popup_no = Rect::default();
    }
}

fn render_tab_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    // Outer chrome: rounded border + title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .title(format!("[ TOKENIZER v{} ]", env!("CARGO_PKG_VERSION")))
        .title_style(
            Style::default()
                .fg(theme::AMBER)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Lay out each tab with its natural label width + 2 cells padding.
    // Clicks map 1:1 to the rendered cells — no offset bug.
    const PAD: u16 = 2; // one space on each side of the label
    const DIVIDER: u16 = 1; // single-cell divider between tabs

    let mut cursor_x: u16 = inner.x;
    let row_y = inner.y;
    let row_h = inner.height;

    for (i, tab) in Tab::ALL.iter().enumerate() {
        let label = tab.label();
        let label_w = label.chars().count() as u16;
        let tab_w = label_w + PAD;

        // Stop drawing if we'd overflow the inner area.
        if cursor_x.saturating_add(tab_w) > inner.x + inner.width {
            // Still record a zero-width rect so click_test skips cleanly.
            app.click_areas.tab_rects[tab.index()] =
                Rect::new(cursor_x.min(inner.x + inner.width), row_y, 0, row_h);
            continue;
        }

        let tab_rect = Rect::new(cursor_x, row_y, tab_w, row_h);
        app.click_areas.tab_rects[tab.index()] = tab_rect;

        let is_active = *tab == app.current_tab;
        let (fg, bg, modifier) = if is_active {
            (
                theme::BG,
                theme::CYAN,
                Modifier::BOLD | Modifier::UNDERLINED,
            )
        } else {
            (theme::AMBER_DIM, theme::BG, Modifier::empty())
        };

        let text = format!(" {label} ");
        let para = Paragraph::new(Line::from(Span::styled(
            text,
            Style::default().fg(fg).bg(bg).add_modifier(modifier),
        )));
        frame.render_widget(para, tab_rect);

        cursor_x += tab_w;

        // Draw divider between tabs (not after last)
        if i < Tab::ALL.len() - 1 && cursor_x < inner.x + inner.width {
            let div_rect = Rect::new(cursor_x, row_y, DIVIDER, row_h);
            let divider = Paragraph::new(Span::styled(
                "\u{2502}",
                Style::default().fg(theme::BORDER),
            ));
            frame.render_widget(divider, div_rect);
            cursor_x += DIVIDER;
        }
    }
}

fn render_active_tab(frame: &mut Frame, app: &mut App, area: Rect) {
    match app.current_tab {
        Tab::Status => status::render(frame, app, area),
        Tab::Log => log::render(frame, app, area),
        Tab::Compression => compression::render(frame, app, area),
        Tab::Efficiency => efficiency::render(frame, app, area),
        Tab::Settings => settings::render(frame, app, area),
        Tab::Restructure => restructure::render(frame, app, area),
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let scan_time = app.last_scan_time.as_deref().unwrap_or("never");

    let mut spans = vec![
        Span::styled(
            " Tab/Shift-Tab: switch ",
            Style::default().fg(theme::AMBER_DIM),
        ),
        Span::styled("\u{2502}", Style::default().fg(theme::BORDER)),
        Span::styled(
            format!(" Scan: {scan_time} "),
            Style::default().fg(theme::AMBER_DIM),
        ),
        Span::styled("\u{2502}", Style::default().fg(theme::BORDER)),
        Span::styled(
            format!(" Files: {} ", app.scan_result.len()),
            Style::default().fg(theme::AMBER_DIM),
        ),
        Span::styled("\u{2502}", Style::default().fg(theme::BORDER)),
        Span::styled(
            format!(" Saved: {} tokens ", app.total_tokens_saved),
            Style::default().fg(theme::GREEN),
        ),
        Span::styled("\u{2502}", Style::default().fg(theme::BORDER)),
        Span::styled(" r:refresh q:quit ", Style::default().fg(theme::AMBER_DIM)),
    ];

    // While an optimize worker is running, expose Esc to abort.
    if app.optimize_job.is_some() {
        spans.push(Span::styled("\u{2502}", Style::default().fg(theme::BORDER)));
        spans.push(Span::styled(
            " RUNNING — Esc to abort ",
            Style::default()
                .fg(theme::BG)
                .bg(theme::AMBER_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Show settings status if present
    if let Some(ref status) = app.settings_status {
        spans.push(Span::styled("\u{2502}", Style::default().fg(theme::BORDER)));
        spans.push(Span::styled(
            format!(" {status} "),
            Style::default().fg(theme::GREEN),
        ));
    }

    let status = Line::from(spans);
    let paragraph = Paragraph::new(status).style(Style::default().bg(theme::BG));
    frame.render_widget(paragraph, area);
}

/// Format bytes for display. Used by status tab and others.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
