mod app;
mod config;
mod daemon;
mod db;
mod engine;
mod event;
mod theme;
mod ui;

use std::io;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEventKind, KeyModifiers,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, PendingAction, Tab};
use event::AppEvent;
use ui::compression::{self as compress_ui, CompressAction};
use ui::restructure::{self as restructure_ui, RestructureAction};
use ui::settings::{self, SettingsAction};

#[derive(Parser)]
#[command(name = "tokenizer", version = "0.2.0")]
#[command(about = "Token optimization TUI for Claude Code's ~/.claude directory")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the TUI (default)
    Tui,
    /// Run scan + convert headlessly
    Optimize {
        /// Dry run: show what would be converted without writing
        #[arg(long)]
        dry_run: bool,
        /// Suppress progress output
        #[arg(long)]
        quiet: bool,
    },
    /// Rollback a conversion by manifest ID
    Rollback {
        /// Manifest ID to rollback
        manifest_id: String,
    },
    /// Install systemd timer for periodic optimization
    InstallTimer,
    /// Install Claude Code session hook
    InstallHook,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Tui) => run_tui(),
        Some(Commands::Optimize { dry_run, quiet }) => run_optimize(dry_run, quiet),
        Some(Commands::Rollback { manifest_id }) => daemon::rollback(&manifest_id),
        Some(Commands::InstallTimer) => daemon::install_timer(),
        Some(Commands::InstallHook) => daemon::install_hook(),
    }
}

fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let result = tui_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn tui_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    terminal.draw(|f| ui::render(f, app))?;

    loop {
        match event::poll_event()? {
            AppEvent::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Dismiss popup on any key
                if app.popup.is_some() {
                    app.popup = None;
                    terminal.draw(|f| ui::render(f, app))?;
                    continue;
                }

                // Global keys first
                match key.code {
                    KeyCode::Char('q') => {
                        app.should_quit = true;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true;
                    }
                    // Tab / Shift-Tab cycle through tabs (universal across all OS)
                    KeyCode::Tab => app.next_tab(),
                    KeyCode::BackTab => app.prev_tab(),
                    // Number keys for direct tab access
                    KeyCode::Char('1') => app.set_tab(Tab::Compression),
                    KeyCode::Char('2') => app.set_tab(Tab::Status),
                    KeyCode::Char('3') => app.set_tab(Tab::Log),
                    KeyCode::Char('4') => app.set_tab(Tab::Efficiency),
                    KeyCode::Char('5') => app.set_tab(Tab::Restructure),
                    KeyCode::Char('6') => app.set_tab(Tab::Settings),
                    // F-keys still work as fallback
                    KeyCode::F(1) => app.set_tab(Tab::Compression),
                    KeyCode::F(2) => app.set_tab(Tab::Status),
                    KeyCode::F(3) => app.set_tab(Tab::Log),
                    KeyCode::F(4) => app.set_tab(Tab::Efficiency),
                    KeyCode::F(5) => app.set_tab(Tab::Restructure),
                    KeyCode::F(6) => app.set_tab(Tab::Settings),
                    KeyCode::Char('r') => {
                        app.refresh_scan();
                        app.refresh_trees();
                    }
                    _ => {
                        // Per-tab input handling
                        handle_tab_input(app, key.code);
                    }
                }

                // Check for pending action signal from compress/restructure tab
                match app.pending_action.take() {
                    Some(PendingAction::Optimize) => {
                        run_optimize_inline(app, terminal);
                    }
                    Some(PendingAction::Restructure) => {
                        run_restructure_inline(app, terminal);
                    }
                    None => {}
                }

                terminal.draw(|f| ui::render(f, app))?;
            }
            AppEvent::Mouse(mouse) => {
                let mut needs_redraw = false;

                match mouse.kind {
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        let col = mouse.column;
                        let row = mouse.row;

                        // Check tab clicks
                        for (i, rect) in app.click_areas.tab_rects.iter().enumerate() {
                            if hit_test(col, row, *rect) {
                                app.set_tab(Tab::ALL[i]);
                                needs_redraw = true;
                                break;
                            }
                        }

                        // Dismiss popup on click
                        if app.popup.is_some() {
                            app.popup = None;
                            needs_redraw = true;
                        }

                        // Check OPTIMIZE button click (only on Compression tab)
                        if !needs_redraw
                            && app.current_tab == Tab::Compression
                            && hit_test(col, row, app.click_areas.optimize_button)
                        {
                            run_optimize_inline(app, terminal);
                            needs_redraw = true;
                        }

                        // Check RESTRUCTURE button click (only on Restructure tab)
                        if !needs_redraw
                            && app.current_tab == Tab::Restructure
                            && hit_test(col, row, app.click_areas.restructure_button)
                        {
                            run_restructure_inline(app, terminal);
                            needs_redraw = true;
                        }

                        // Per-tab click handling
                        if !needs_redraw {
                            needs_redraw = handle_tab_click(app, col, row);
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        handle_scroll(app, -3);
                        needs_redraw = true;
                    }
                    MouseEventKind::ScrollDown => {
                        handle_scroll(app, 3);
                        needs_redraw = true;
                    }
                    _ => {}
                }

                if needs_redraw {
                    terminal.draw(|f| ui::render(f, app))?;
                }
            }
            AppEvent::Resize(_, _) => {
                terminal.draw(|f| ui::render(f, app))?;
            }
            AppEvent::Tick => {}
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Hit test: is (col, row) inside rect?
fn hit_test(col: u16, row: u16, rect: ratatui::layout::Rect) -> bool {
    rect.width > 0
        && rect.height > 0
        && col >= rect.x
        && col < rect.x + rect.width
        && row >= rect.y
        && row < rect.y + rect.height
}

/// Handle per-tab keyboard input (non-global keys).
fn handle_tab_input(app: &mut App, code: KeyCode) {
    match app.current_tab {
        Tab::Status => match code {
            KeyCode::Down | KeyCode::Char('j') => app.scroll_status(1),
            KeyCode::Up | KeyCode::Char('k') => app.scroll_status(-1),
            _ => {}
        },
        Tab::Log => match code {
            KeyCode::Down | KeyCode::Char('j') => app.scroll_log(1),
            KeyCode::Up | KeyCode::Char('k') => app.scroll_log(-1),
            _ => {}
        },
        Tab::Compression => {
            // Intercept Enter/Space on Optimize focus to run inline
            if (code == KeyCode::Enter || code == KeyCode::Char(' ')) && app.compress_focus == 3 {
                // Signal: will be handled by tui_loop after redraw
                app.pending_action = Some(PendingAction::Optimize);
                return;
            }
            let action = match code {
                KeyCode::Down | KeyCode::Char('j') => Some(CompressAction::Down),
                KeyCode::Up | KeyCode::Char('k') => Some(CompressAction::Up),
                KeyCode::Left | KeyCode::Char('h') => Some(CompressAction::Left),
                KeyCode::Right | KeyCode::Char('l') => Some(CompressAction::Right),
                KeyCode::Char(' ') | KeyCode::Enter => Some(CompressAction::Toggle),
                KeyCode::Char('\t') => Some(CompressAction::CycleFocus),
                _ => None,
            };
            if let Some(action) = action {
                compress_ui::handle_input(app, action);
            }
        }
        Tab::Efficiency => {
            // Dashboard is read-only
        }
        Tab::Settings => {
            let action = match code {
                KeyCode::Down | KeyCode::Char('j') => Some(SettingsAction::Down),
                KeyCode::Up | KeyCode::Char('k') => Some(SettingsAction::Up),
                KeyCode::Left | KeyCode::Char('h') => Some(SettingsAction::Left),
                KeyCode::Right | KeyCode::Char('l') => Some(SettingsAction::Right),
                KeyCode::Char(' ') | KeyCode::Enter => Some(SettingsAction::Toggle),
                _ => None,
            };
            if let Some(action) = action {
                settings::handle_input(app, action);
            }
        }
        Tab::Restructure => {
            let action = match code {
                KeyCode::Down | KeyCode::Char('j') => Some(RestructureAction::Down),
                KeyCode::Up | KeyCode::Char('k') => Some(RestructureAction::Up),
                KeyCode::Char(' ') | KeyCode::Enter => Some(RestructureAction::Toggle),
                KeyCode::Char('\t') => Some(RestructureAction::CycleFocus),
                _ => None,
            };
            if let Some(action) = action {
                restructure_ui::handle_input(app, action);
            }
        }
    }
}

/// Handle mouse click per tab — returns true if handled.
fn handle_tab_click(app: &mut App, col: u16, row: u16) -> bool {
    // Ensure click is in content area
    if !hit_test(col, row, app.click_areas.content_area) {
        return false;
    }

    let content = app.click_areas.content_area;
    let relative_row = row.saturating_sub(content.y);

    match app.current_tab {
        Tab::Status => {
            // Bottom 3 rows = refresh button
            if row >= content.y + content.height.saturating_sub(3) {
                app.refresh_scan();
                app.refresh_trees();
                app.settings_status = Some("Scan refreshed".to_string());
            }
            true
        }
        Tab::Log => {
            // Click on a log row selects it
            if relative_row > 1 {
                // skip header
                let idx = (relative_row as usize).saturating_sub(2);
                app.log_table_state.select(Some(idx));
            }
            true
        }
        Tab::Compression => {
            // Left 40% = file list
            let list_width = content.width * 40 / 100;
            if col < content.x + list_width && relative_row > 0 {
                let clicked = relative_row.saturating_sub(1) as usize;
                if clicked < app.compression_file_indices.len() {
                    app.compression_selected = clicked;
                }
            }
            true
        }
        Tab::Settings => {
            // Each settings row is 2 lines tall, offset by border
            if relative_row > 0 {
                let row_idx = (relative_row.saturating_sub(1) / 2) as usize;
                if row_idx < settings::SETTINGS_ROW_COUNT {
                    app.settings_selected = row_idx;
                    // Also toggle if clicking on a checkbox/button row
                    settings::handle_input(app, SettingsAction::Toggle);
                }
            }
            true
        }
        _ => false,
    }
}

/// Handle mouse scroll per tab — always bounded.
fn handle_scroll(app: &mut App, delta: i32) {
    match app.current_tab {
        Tab::Status => app.scroll_status(delta),
        Tab::Log => app.scroll_log(delta),
        Tab::Compression => app.scroll_compression(delta),
        Tab::Efficiency => {} // static dashboard
        Tab::Settings => {
            if delta > 0 {
                settings::handle_input(app, SettingsAction::Down);
            } else {
                settings::handle_input(app, SettingsAction::Up);
            }
        }
        Tab::Restructure => app.scroll_restructure(delta),
    }
}

/// Try to acquire an exclusive lock on a lockfile to prevent concurrent runs.
/// Returns the file handle (drop releases the lock), or None if already locked.
fn try_acquire_lock() -> Option<std::fs::File> {
    use fs2::FileExt;
    let lock_path = config::Config::config_dir().join("optimizer.lock");
    let _ = std::fs::create_dir_all(config::Config::config_dir());
    let file = std::fs::File::create(&lock_path).ok()?;
    if file.try_lock_exclusive().is_err() {
        return None;
    }
    Some(file)
}

fn run_optimize(dry_run: bool, quiet: bool) -> Result<()> {
    use engine::converter::{convert_md_to_toon, CompressionLevel};
    use engine::manifest;
    use engine::scanner;

    let _lock = match try_acquire_lock() {
        Some(f) => f,
        None => {
            if !quiet {
                println!("Another optimizer instance is running. Skipping.");
            }
            return Ok(());
        }
    };

    let config = config::Config::load()?;
    let level = CompressionLevel::clamp(config.compression_default);
    let files = scanner::scan();
    let db = db::Db::open()?;
    let optimized_paths = db.get_optimized_paths().unwrap_or_default();

    let convertible: Vec<_> = files
        .iter()
        .filter(|f| {
            !f.is_whitelisted
                && f.current_format == "md"
                && !optimized_paths.contains(&f.path.display().to_string())
        })
        .collect();

    if convertible.is_empty() {
        if !quiet {
            println!("No files to convert.");
        }
        return Ok(());
    }

    if !quiet {
        println!("Found {} convertible file(s):", convertible.len());
    }

    for f in &convertible {
        let content = std::fs::read_to_string(&f.path)?;
        let converted = convert_md_to_toon(&content, level, f.category);
        let savings = content.len().saturating_sub(converted.len());
        let pct = if content.is_empty() {
            0
        } else {
            savings * 100 / content.len()
        };

        if dry_run {
            if !quiet {
                println!(
                    "  [DRY] {} ({} -> {} bytes, {}% reduction)",
                    f.path.display(),
                    content.len(),
                    converted.len(),
                    pct,
                );
            }
        } else {
            let backup = manifest::backup_file(&f.path)?;
            let toon_path = f.path.with_extension("toon");
            std::fs::write(&toon_path, &converted)?;

            let entry = manifest::ManifestEntry {
                id: manifest::generate_id(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                original_path: f.path.display().to_string(),
                backup_path: backup.display().to_string(),
                converted_path: toon_path.display().to_string(),
                original_bytes: content.len() as u64,
                converted_bytes: converted.len() as u64,
            };
            manifest::write_manifest_entry(&entry)?;

            db.insert_conversion(
                &f.path.display().to_string(),
                "md_to_toon",
                content.len() as i64,
                converted.len() as i64,
                (savings / 4) as i64,
            )?;

            db.mark_optimized(
                &f.path.display().to_string(),
                "md",
                "toon",
                content.len() as i64,
                converted.len() as i64,
            )?;

            if !quiet {
                println!(
                    "  {} -> {} ({} -> {} bytes, {}% reduction)",
                    f.path.display(),
                    toon_path.display(),
                    content.len(),
                    converted.len(),
                    pct,
                );
            }
        }
    }

    if !quiet {
        if dry_run {
            println!("\nDry run complete. No files were modified.");
        } else {
            println!("\nConversion complete.");
        }
    }

    Ok(())
}

/// Run optimization inline within the TUI, showing progress bar and popup on completion.
fn run_optimize_inline(app: &mut App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    use engine::converter::{convert_md_to_toon, CompressionLevel};
    use engine::manifest;

    let _lock = match try_acquire_lock() {
        Some(f) => f,
        None => {
            app.popup = Some(app::Popup {
                title: "LOCK CONFLICT".to_string(),
                lines: vec!["Another optimizer instance is running.".to_string()],
            });
            return;
        }
    };

    let level = CompressionLevel::clamp(app.config.compression_default);

    let convertible: Vec<usize> = app
        .scan_result
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_whitelisted && !f.is_optimized && f.current_format == "md")
        .map(|(i, _)| i)
        .collect();

    let total = convertible.len();
    if total == 0 {
        app.popup = Some(app::Popup {
            title: "OPTIMIZATION COMPLETE".to_string(),
            lines: vec!["No files to convert.".to_string()],
        });
        return;
    }

    app.optimize_progress = Some(app::ProgressState {
        current: 0,
        total,
        label: "Starting...".to_string(),
        done: false,
    });
    let _ = terminal.draw(|f| ui::render(f, app));

    let mut converted_count = 0usize;
    let mut total_saved = 0i64;
    let mut errors = Vec::new();
    let db = db::Db::open().ok();

    // Pre-collect file info to avoid borrow conflicts
    let file_info: Vec<(
        std::path::PathBuf,
        String,
        String,
        engine::scanner::FileCategory,
    )> = convertible
        .iter()
        .map(|&idx| {
            let f = &app.scan_result[idx];
            let path = f.path.clone();
            let path_str = f.path.display().to_string();
            let fname = f
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            (path, path_str, fname, f.category)
        })
        .collect();

    for (i, (path, path_str, fname, category)) in file_info.iter().enumerate() {
        app.optimize_progress = Some(app::ProgressState {
            current: i + 1,
            total,
            label: fname.clone(),
            done: false,
        });
        let _ = terminal.draw(|f| ui::render(f, app));

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{path_str}: {e}"));
                continue;
            }
        };

        let converted = convert_md_to_toon(&content, level, *category);
        let savings = content.len().saturating_sub(converted.len());

        if let Ok(backup) = manifest::backup_file(path) {
            let toon_path = path.with_extension("toon");
            if std::fs::write(&toon_path, &converted).is_ok() {
                let entry = manifest::ManifestEntry {
                    id: manifest::generate_id(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    original_path: path_str.clone(),
                    backup_path: backup.display().to_string(),
                    converted_path: toon_path.display().to_string(),
                    original_bytes: content.len() as u64,
                    converted_bytes: converted.len() as u64,
                };
                if let Err(e) = manifest::write_manifest_entry(&entry) {
                    errors.push(format!("manifest: {e}"));
                }

                if let Some(ref db) = db {
                    let _ = db.insert_conversion(
                        path_str,
                        "md_to_toon",
                        content.len() as i64,
                        converted.len() as i64,
                        (savings / 4) as i64,
                    );
                    let _ = db.mark_optimized(
                        path_str,
                        "md",
                        "toon",
                        content.len() as i64,
                        converted.len() as i64,
                    );
                }
                converted_count += 1;
                total_saved += (savings / 4) as i64;
            }
        }
    }

    app.optimize_progress = None;
    app.popup = Some(app::Popup {
        title: "OPTIMIZATION COMPLETE".to_string(),
        lines: vec![
            format!("{converted_count}/{total} files converted"),
            format!("~{total_saved} tokens saved"),
            if errors.is_empty() {
                "No errors".to_string()
            } else {
                format!("{} errors", errors.len())
            },
        ],
    });

    if let Some(ref db) = db {
        app.optimized_paths = db.get_optimized_paths().unwrap_or_default();
    }
    app.refresh_scan();
}

/// Run restructure inline within the TUI, showing progress and popup on completion.
fn run_restructure_inline(app: &mut App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    let base_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".claude");

    app.restructure_progress = Some(app::ProgressState {
        current: 0,
        total: 1,
        label: "Analyzing...".to_string(),
        done: false,
    });
    let _ = terminal.draw(|f| ui::render(f, app));

    match engine::restructure::apply_restructure(&app.proposed_tree, &base_dir) {
        Ok(actions) => {
            let total = actions.len();
            for (i, action) in actions.iter().enumerate() {
                app.restructure_progress = Some(app::ProgressState {
                    current: i + 1,
                    total,
                    label: action.clone(),
                    done: false,
                });
                let _ = terminal.draw(|f| ui::render(f, app));
            }

            app.restructure_progress = None;
            app.popup = Some(app::Popup {
                title: "RESTRUCTURE COMPLETE".to_string(),
                lines: if actions.is_empty() {
                    vec!["No actions needed.".to_string()]
                } else {
                    let mut lines: Vec<String> = actions.iter().take(8).cloned().collect();
                    if actions.len() > 8 {
                        lines.push(format!("... and {} more", actions.len() - 8));
                    }
                    lines.push(format!("{total} total actions"));
                    lines
                },
            });
            app.refresh_trees();
        }
        Err(e) => {
            app.restructure_progress = None;
            app.popup = Some(app::Popup {
                title: "RESTRUCTURE ERROR".to_string(),
                lines: vec![format!("{e}")],
            });
        }
    }
}
