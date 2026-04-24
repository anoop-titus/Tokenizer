//! Shared button rendering. All buttons are *filled* — never plain outlined
//! rectangles — so they read as interactive at rest, and light up on focus.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::theme;

/// Visual intent — controls the focused accent color.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ButtonIntent {
    /// Primary action (cyan). Use for the dominant action on a tab.
    Primary,
    /// Affirmative (green). Use for destructive-adjacent confirms, installs.
    Success,
    /// Neutral (amber). Use for secondary actions like refresh.
    Neutral,
}

impl ButtonIntent {
    fn focus_color(self) -> ratatui::style::Color {
        match self {
            ButtonIntent::Primary => theme::CYAN,
            ButtonIntent::Success => theme::GREEN,
            ButtonIntent::Neutral => theme::AMBER_BRIGHT,
        }
    }
}

/// Render a bordered, filled button.
/// - Rest: warm `SURFACE` fill, dim border, muted amber label.
/// - Focused: saturated intent color fill, bright border, dark label, chevron affordance.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    focused: bool,
    intent: ButtonIntent,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (fill, fg, border_fg, border_type, text) = if focused {
        let accent = intent.focus_color();
        (
            accent,
            theme::BG,
            accent,
            BorderType::Double,
            format!("\u{25B8} {label} \u{25C2}"),
        )
    } else {
        (
            theme::SURFACE,
            theme::AMBER_BRIGHT,
            theme::BORDER_BRIGHT,
            BorderType::Rounded,
            format!("  {label}  "),
        )
    };

    let paragraph = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default()
            .fg(fg)
            .bg(fill)
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_fg).bg(fill))
            .style(Style::default().bg(fill)),
    )
    .style(Style::default().bg(fill));

    frame.render_widget(paragraph, area);
}

/// Single-line inline button (for use inside a Paragraph with other spans).
/// Produces a styled `Span` suitable for `Line::from(vec![...])`.
pub fn inline_span(label: &str, focused: bool, intent: ButtonIntent) -> Span<'static> {
    let (fg, bg) = if focused {
        (theme::BG, intent.focus_color())
    } else {
        (theme::AMBER_BRIGHT, theme::SURFACE)
    };
    let text = if focused {
        format!(" \u{25B8} {label} \u{25C2} ")
    } else {
        format!("  {label}  ")
    };
    Span::styled(
        text,
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    )
}
