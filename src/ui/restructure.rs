use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Gauge, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::engine::restructure::{TreeNode, TreeNodeKind};
use crate::theme;

// ---------------------------------------------------------------------------
// Public action enum + input handler
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum RestructureAction {
    Up,
    Down,
    Toggle,
    CycleFocus,
}

/// Handle input for the Restructure tab. Returns true if consumed.
#[allow(dead_code)]
pub fn handle_input(app: &mut App, action: RestructureAction) -> bool {
    match action {
        RestructureAction::CycleFocus => {
            app.restructure_focus = if app.restructure_focus == 0 { 1 } else { 0 };
            true
        }
        RestructureAction::Up => {
            if app.restructure_focus == 0 {
                app.scroll_restructure(-1);
            } else {
                // Move focus to tree
                app.restructure_focus = 0;
            }
            true
        }
        RestructureAction::Down => {
            if app.restructure_focus == 0 {
                app.scroll_restructure(1);
            } else {
                // Already on button, nowhere to go down
            }
            true
        }
        RestructureAction::Toggle => {
            if app.restructure_focus == 1 {
                // Signal main loop to run restructure with progress bar
                app.pending_action = Some(crate::app::PendingAction::Restructure);
            }
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render Tab 6: Directory Restructure -- current vs proposed tree.
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let tree_focused = app.restructure_focus == 0;
    let scroll = app.restructure_scroll;
    render_tree_panel(
        frame,
        "CURRENT",
        &app.current_tree,
        theme::AMBER,
        scroll,
        panels[0],
        tree_focused,
    );
    render_tree_panel(
        frame,
        "PROPOSED",
        &app.proposed_tree,
        theme::GREEN,
        scroll,
        panels[1],
        tree_focused,
    );
    render_apply_button(frame, &mut *app, chunks[1]);
}

fn render_tree_panel(
    frame: &mut Frame,
    title: &str,
    tree: &[TreeNode],
    title_color: ratatui::style::Color,
    scroll: u16,
    area: Rect,
    focused: bool,
) {
    let lines = tree_to_lines(tree, 0);

    let border_color = if focused { theme::CYAN } else { theme::BORDER };

    let paragraph = Paragraph::new(lines).scroll((scroll, 0)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(format!("[ {title} ]"))
            .title_style(
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    frame.render_widget(paragraph, area);
}

fn tree_to_lines(nodes: &[TreeNode], depth: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let count = nodes.len();

    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == count - 1;
        let prefix = if depth == 0 {
            String::new()
        } else {
            let mut p = String::new();
            for _ in 0..depth.saturating_sub(1) {
                p.push_str("\u{2502}   ");
            }
            if is_last {
                p.push_str("\u{2514}\u{2500}\u{2500} ");
            } else {
                p.push_str("\u{251c}\u{2500}\u{2500} ");
            }
            p
        };

        let (name_color, suffix) = match node.kind {
            TreeNodeKind::Existing => (theme::AMBER, ""),
            TreeNodeKind::New => (theme::GREEN, " [NEW]"),
            TreeNodeKind::Moved => (theme::CYAN, " [MOVED]"),
            TreeNodeKind::Removed => (theme::MAGENTA, " [DEL]"),
        };

        let dir_marker = if node.is_dir { "/" } else { "" };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(theme::BORDER)),
            Span::styled(
                format!("{}{dir_marker}", node.name),
                Style::default().fg(name_color),
            ),
            Span::styled(
                suffix.to_string(),
                Style::default().fg(name_color).add_modifier(Modifier::BOLD),
            ),
        ]));

        if !node.children.is_empty() {
            let child_lines = tree_to_lines(&node.children, depth + 1);
            lines.extend(child_lines);
        }
    }

    lines
}

fn render_apply_button(frame: &mut Frame, app: &mut App, area: Rect) {
    // Store rect for mouse click detection
    app.click_areas.restructure_button = area;

    // Show progress bar if restructure is running
    if let Some(ref progress) = app.restructure_progress {
        let ratio = if progress.total > 0 {
            progress.current as f64 / progress.total as f64
        } else {
            0.0
        };
        let label = format!(
            "RESTRUCTURING... {}/{} actions",
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

    let focused = app.restructure_focus == 1;
    super::button::render(
        frame,
        area,
        "APPLY RESTRUCTURE",
        focused,
        super::button::ButtonIntent::Primary,
    );
}
