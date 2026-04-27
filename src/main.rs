mod app;
mod config;
mod daemon;
mod db;
mod discover;
mod engine;
mod event;
mod theme;
mod ui;
mod updater;

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

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

use app::{App, OptMsg, OptimizeJob, OptimizeResult, PendingAction, Popup, PopupKind, Tab};
use event::AppEvent;
use ui::compression::{self as compress_ui, CompressAction};
use ui::restructure::{self as restructure_ui, RestructureAction};
use ui::settings::{self, SettingsAction};

#[derive(Parser)]
#[command(name = "tokenizer", version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Token optimization TUI for Claude Code's ~/.claude directory")]
struct Cli {
    /// Use this Claude directory directly (skips discovery + fzf).
    #[arg(long, global = true)]
    claude_dir: Option<PathBuf>,

    /// Force the FZF picker even if a directory is already saved.
    #[arg(long, global = true)]
    reselect_dir: bool,

    /// Shortcut for the `update` subcommand: pull and install the latest
    /// release from GitHub, then exit. Equivalent to `tokenizer update`.
    #[arg(long, conflicts_with = "command")]
    update: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the TUI (default)
    Tui,
    /// Run scan + convert headlessly
    Optimize {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        quiet: bool,
    },
    /// Rollback a conversion by manifest ID
    Rollback { manifest_id: String },
    /// Install systemd timer for periodic optimization
    InstallTimer,
    /// Install Claude Code session hook
    InstallHook,
    /// Check GitHub for the latest release and install it.
    Update {
        /// Reinstall even if already on the latest version.
        #[arg(long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // `update` (subcommand or --update flag) has no need for a Claude
    // directory — short-circuit before discovery so it works on a fresh
    // install.
    if cli.update {
        return updater::run_update_cli(false);
    }
    if let Some(Commands::Update { force }) = cli.command {
        return updater::run_update_cli(force);
    }

    // Discovery runs BEFORE we touch the terminal so fzf can drive it.
    let mut config = config::Config::load().unwrap_or_default();
    let chosen = discover::resolve_claude_dir(&mut config, cli.claude_dir, cli.reselect_dir)?;
    eprintln!("Using Claude directory: {}", chosen.display());

    match cli.command {
        None | Some(Commands::Tui) => run_tui(config),
        Some(Commands::Optimize { dry_run, quiet }) => run_optimize(dry_run, quiet),
        Some(Commands::Rollback { manifest_id }) => daemon::rollback(&manifest_id),
        Some(Commands::InstallTimer) => daemon::install_timer(),
        Some(Commands::InstallHook) => daemon::install_hook(),
        Some(Commands::Update { .. }) => unreachable!("handled above"),
    }
}

fn run_tui(config: config::Config) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::with_config(config);
    let result = tui_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Some(ref new_binary) = app.pending_restart {
        eprintln!("Restarting tokenizer with updated binary...");
        updater::restart(new_binary)?;
    }

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

                if let Some(popup) = app.popup.clone() {
                    match popup.kind {
                        PopupKind::Info => {
                            app.popup = None;
                        }
                        PopupKind::UpdatePrompt { ref asset_url, .. } => match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                handle_update_accept(app, terminal, asset_url.clone())?;
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                app.popup = None;
                            }
                            _ => {}
                        },
                    }
                    terminal.draw(|f| ui::render(f, app))?;
                    continue;
                }

                // Esc cancels a running optimize job (idempotent — no-op if none).
                if key.code == KeyCode::Esc && app.optimize_job.is_some() {
                    if let Some(job) = &app.optimize_job {
                        job.abort.store(true, Ordering::Relaxed);
                    }
                    terminal.draw(|f| ui::render(f, app))?;
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true;
                    }
                    KeyCode::Tab => app.next_tab(),
                    KeyCode::BackTab => app.prev_tab(),
                    KeyCode::Char('1') => app.set_tab(Tab::Compression),
                    KeyCode::Char('2') => app.set_tab(Tab::Status),
                    KeyCode::Char('3') => app.set_tab(Tab::Log),
                    KeyCode::Char('4') => app.set_tab(Tab::Efficiency),
                    KeyCode::Char('5') => app.set_tab(Tab::Restructure),
                    KeyCode::Char('6') => app.set_tab(Tab::Settings),
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
                    _ => handle_tab_input(app, key.code),
                }

                match app.pending_action.take() {
                    Some(PendingAction::Optimize) => start_optimize(app),
                    Some(PendingAction::Restructure) => run_restructure_inline(app, terminal),
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

                        if let Some(popup) = app.popup.clone() {
                            match popup.kind {
                                PopupKind::Info => {
                                    app.popup = None;
                                }
                                PopupKind::UpdatePrompt { ref asset_url, .. } => {
                                    if hit_test(col, row, app.click_areas.popup_yes) {
                                        handle_update_accept(app, terminal, asset_url.clone())?;
                                    } else if hit_test(col, row, app.click_areas.popup_no) {
                                        app.popup = None;
                                    }
                                }
                            }
                            terminal.draw(|f| ui::render(f, app))?;
                            continue;
                        }

                        for (i, rect) in app.click_areas.tab_rects.iter().enumerate() {
                            if hit_test(col, row, *rect) {
                                app.set_tab(Tab::ALL[i]);
                                needs_redraw = true;
                                break;
                            }
                        }

                        if !needs_redraw
                            && app.current_tab == Tab::Compression
                            && hit_test(col, row, app.click_areas.optimize_button)
                        {
                            start_optimize(app);
                            needs_redraw = true;
                        }

                        if !needs_redraw
                            && app.current_tab == Tab::Restructure
                            && hit_test(col, row, app.click_areas.restructure_button)
                        {
                            run_restructure_inline(app, terminal);
                            needs_redraw = true;
                        }

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
            AppEvent::Tick => {
                let mut redraw = false;
                if drain_optimize_job(app) {
                    redraw = true;
                }
                if drain_update_check(app) {
                    redraw = true;
                }
                if redraw {
                    terminal.draw(|f| ui::render(f, app))?;
                }
            }
        }

        if app.should_quit {
            // Signal abort to any running worker so it stops promptly. We
            // don't block on it — the worker holds its own lock and writes
            // are atomic per file.
            if let Some(job) = &app.optimize_job {
                job.abort.store(true, Ordering::Relaxed);
            }
            break;
        }
    }

    Ok(())
}

fn hit_test(col: u16, row: u16, rect: ratatui::layout::Rect) -> bool {
    rect.width > 0
        && rect.height > 0
        && col >= rect.x
        && col < rect.x + rect.width
        && row >= rect.y
        && row < rect.y + rect.height
}

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
            if (code == KeyCode::Enter || code == KeyCode::Char(' ')) && app.compress_focus == 3 {
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
        Tab::Efficiency => {}
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

fn handle_tab_click(app: &mut App, col: u16, row: u16) -> bool {
    if !hit_test(col, row, app.click_areas.content_area) {
        return false;
    }

    let content = app.click_areas.content_area;
    let relative_row = row.saturating_sub(content.y);

    match app.current_tab {
        Tab::Status => {
            if row >= content.y + content.height.saturating_sub(3) {
                app.refresh_scan();
                app.refresh_trees();
                app.settings_status = Some("Scan refreshed".to_string());
            }
            true
        }
        Tab::Log => {
            if relative_row > 1 {
                let idx = (relative_row as usize).saturating_sub(2);
                app.log_table_state.select(Some(idx));
            }
            true
        }
        Tab::Compression => {
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
            if relative_row > 0 {
                let row_idx = (relative_row.saturating_sub(1) / 2) as usize;
                if row_idx < settings::SETTINGS_ROW_COUNT {
                    app.settings_selected = row_idx;
                    settings::handle_input(app, SettingsAction::Toggle);
                }
            }
            true
        }
        _ => false,
    }
}

fn handle_scroll(app: &mut App, delta: i32) {
    match app.current_tab {
        Tab::Status => app.scroll_status(delta),
        Tab::Log => app.scroll_log(delta),
        Tab::Compression => app.scroll_compression(delta),
        Tab::Efficiency => {}
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

/// Try to acquire an exclusive lock. Returns the open file (drop releases) or
/// the underlying io::Error so callers can distinguish contention from real
/// FS failures.
fn try_acquire_lock() -> std::result::Result<std::fs::File, io::Error> {
    use fs2::FileExt;
    let lock_path = config::Config::config_dir().join("optimizer.lock");
    std::fs::create_dir_all(config::Config::config_dir())?;
    let file = std::fs::File::create(&lock_path)?;
    file.try_lock_exclusive()?;
    Ok(file)
}

// =====================================================================
// Headless CLI optimize — fixed for partial-failure data integrity.
// =====================================================================

fn run_optimize(dry_run: bool, quiet: bool) -> Result<()> {
    use engine::converter::{convert_md_to_toon, CompressionLevel};
    use engine::scanner;
    use engine::tokenizer::estimate_tokens;

    let _lock = match try_acquire_lock() {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            if !quiet {
                println!("Another optimizer instance is running. Skipping.");
            }
            return Ok(());
        }
        Err(e) => {
            anyhow::bail!("could not acquire optimizer lock: {e}");
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

    let mut converted_total = 0usize;
    let mut failed_total = 0usize;

    for f in &convertible {
        let path_str = f.path.display().to_string();
        let content = match std::fs::read_to_string(&f.path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  read {path_str}: {e}");
                failed_total += 1;
                continue;
            }
        };
        let converted = convert_md_to_toon(&content, level, f.category);
        let savings = content.len().saturating_sub(converted.len());
        let pct = if content.is_empty() {
            0
        } else {
            savings * 100 / content.len()
        };
        let toon_path = f.path.with_extension("toon");

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
            continue;
        }

        match commit_conversion(&f.path, &toon_path, &content, &converted, f.category, &db) {
            Ok(()) => {
                converted_total += 1;
                let tokens = estimate_tokens(content.len() as u64)
                    .saturating_sub(estimate_tokens(converted.len() as u64));
                if !quiet {
                    println!(
                        "  {} -> {} ({} -> {} bytes, {}% reduction, ~{} tokens)",
                        f.path.display(),
                        toon_path.display(),
                        content.len(),
                        converted.len(),
                        pct,
                        tokens,
                    );
                }
            }
            Err(e) => {
                eprintln!("  fail {path_str}: {e}");
                failed_total += 1;
            }
        }
    }

    if !quiet {
        if dry_run {
            println!("\nDry run complete. No files were modified.");
        } else {
            println!(
                "\nConversion complete. {converted_total} converted, {failed_total} failed."
            );
        }
    }

    Ok(())
}

/// Commit a single conversion atomically-ish:
/// 1. Backup pre-existing `.toon` (if any) so we never silently clobber.
/// 2. Backup the original `.md`.
/// 3. Write the new `.toon`.
/// 4. Append the manifest entry — *durable* — so rollback is always possible.
/// 5. Then update DB. If DB write fails after manifest, the next run will
///    see the file marked converted (toon exists) and skip it; manifest is
///    authoritative for rollback.
///
/// On any failure between steps, best-effort cleans up the staged toon so a
/// retry produces consistent state.
fn commit_conversion(
    md_path: &std::path::Path,
    toon_path: &std::path::Path,
    original_content: &str,
    converted_content: &str,
    category: engine::scanner::FileCategory,
    db: &db::Db,
) -> Result<()> {
    use engine::manifest;
    use engine::tokenizer::estimate_tokens;
    let _ = category;

    // Idempotency: if a toon already exists at the destination AND its content
    // matches what we'd write, treat as success without touching anything.
    if toon_path.exists() {
        match std::fs::read_to_string(toon_path) {
            Ok(existing) if existing == converted_content => {
                // Still mark optimized in DB so subsequent scans skip it.
                let _ = db.mark_optimized(
                    &md_path.display().to_string(),
                    "md",
                    "toon",
                    original_content.len() as i64,
                    converted_content.len() as i64,
                );
                return Ok(());
            }
            _ => {
                // Pre-existing different .toon — back it up before clobbering.
                manifest::backup_file(toon_path)
                    .map_err(|e| anyhow::anyhow!("backing up existing .toon: {e}"))?;
            }
        }
    }

    let backup = manifest::backup_file(md_path)?;
    let toon_tmp = toon_path.with_extension("toon.tmp");
    std::fs::write(&toon_tmp, converted_content)
        .map_err(|e| anyhow::anyhow!("writing staged .toon: {e}"))?;
    std::fs::rename(&toon_tmp, toon_path)
        .map_err(|e| anyhow::anyhow!("promoting .toon: {e}"))?;

    let entry = manifest::ManifestEntry {
        id: manifest::generate_id(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        original_path: md_path.display().to_string(),
        backup_path: backup.display().to_string(),
        converted_path: toon_path.display().to_string(),
        original_bytes: original_content.len() as u64,
        converted_bytes: converted_content.len() as u64,
    };

    if let Err(e) = manifest::write_manifest_entry(&entry) {
        // Roll back the .toon write so the file appears unconverted next run.
        let _ = std::fs::remove_file(toon_path);
        anyhow::bail!("manifest write failed (rolled back): {e}");
    }

    let path_str = md_path.display().to_string();
    let savings_bytes = original_content.len().saturating_sub(converted_content.len());
    let tokens_saved = estimate_tokens(original_content.len() as u64)
        .saturating_sub(estimate_tokens(converted_content.len() as u64))
        as i64;
    let _ = savings_bytes;

    db.insert_conversion(
        &path_str,
        "md_to_toon",
        original_content.len() as i64,
        converted_content.len() as i64,
        tokens_saved,
    )?;
    db.mark_optimized(
        &path_str,
        "md",
        "toon",
        original_content.len() as i64,
        converted_content.len() as i64,
    )?;
    Ok(())
}

// =====================================================================
// TUI optimize: spawn a worker thread so the UI stays responsive.
// =====================================================================

/// Kick off the optimize job. Idempotent — if a job is already running, this
/// is a no-op (prevents duplicate workers from re-entrant clicks).
fn start_optimize(app: &mut App) {
    if app.optimize_job.is_some() {
        return;
    }

    let lock = match try_acquire_lock() {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            app.popup = Some(Popup::info(
                "LOCK CONFLICT",
                vec!["Another optimizer instance is running.".to_string()],
            ));
            return;
        }
        Err(e) => {
            app.popup = Some(Popup::info(
                "LOCK ERROR",
                vec![format!("Could not acquire lock: {e}")],
            ));
            return;
        }
    };

    use engine::converter::CompressionLevel;
    let level = CompressionLevel::clamp(app.config.compression_default);

    // Snapshot the convertible-file list — the worker mustn't touch app state.
    let files: Vec<FileToConvert> = app
        .scan_result
        .iter()
        .filter(|f| !f.is_whitelisted && !f.is_optimized && f.current_format == "md")
        .map(|f| FileToConvert {
            path: f.path.clone(),
            path_str: f.path.display().to_string(),
            file_name: f
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string(),
            category: f.category,
            scanned_size: f.size_bytes,
        })
        .collect();

    let total = files.len();
    if total == 0 {
        app.popup = Some(Popup::info(
            "OPTIMIZATION COMPLETE",
            vec!["No files to convert.".to_string()],
        ));
        return;
    }

    let abort = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<OptMsg>();

    app.optimize_progress = Some(app::ProgressState {
        current: 0,
        total,
        label: "Starting...".to_string(),
        done: false,
    });
    app.optimize_job = Some(OptimizeJob {
        rx,
        abort: abort.clone(),
        total,
    });

    let abort_for_thread = abort.clone();
    std::thread::spawn(move || {
        // Hold the lock for the duration of the worker.
        let _lock_guard = lock;
        run_optimize_worker(files, level, abort_for_thread, tx);
    });
}

#[derive(Debug, Clone)]
struct FileToConvert {
    path: PathBuf,
    path_str: String,
    file_name: String,
    category: engine::scanner::FileCategory,
    #[allow(dead_code)]
    scanned_size: u64,
}

fn run_optimize_worker(
    files: Vec<FileToConvert>,
    level: engine::converter::CompressionLevel,
    abort: Arc<AtomicBool>,
    tx: mpsc::Sender<OptMsg>,
) {
    use engine::converter::convert_md_to_toon;
    use engine::tokenizer::estimate_tokens;

    let total = files.len();
    let db = db::Db::open().ok();
    let db_available = db.is_some();
    let mut converted_paths = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut converted_count = 0usize;
    let mut tokens_saved = 0i64;
    let mut aborted = false;

    if !db_available {
        errors.push("warn: SQLite history DB unavailable — stats not persisted".to_string());
    }

    for (i, f) in files.iter().enumerate() {
        if abort.load(Ordering::Relaxed) {
            aborted = true;
            break;
        }

        let _ = tx.send(OptMsg::Progress {
            current: i + 1,
            total,
            label: f.file_name.clone(),
        });

        let content = match std::fs::read_to_string(&f.path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("read {}: {e}", f.path_str));
                continue;
            }
        };
        let converted = convert_md_to_toon(&content, level, f.category);
        let toon_path = f.path.with_extension("toon");

        // Idempotency check: identical .toon already there → mark + skip.
        if toon_path.exists() {
            if let Ok(existing) = std::fs::read_to_string(&toon_path) {
                if existing == converted {
                    converted_paths.push(f.path_str.clone());
                    if let Some(ref db) = db {
                        let _ = db.mark_optimized(
                            &f.path_str,
                            "md",
                            "toon",
                            content.len() as i64,
                            converted.len() as i64,
                        );
                    }
                    continue;
                }
            }
            // Different content — back up the existing .toon first.
            if let Err(e) = engine::manifest::backup_file(&toon_path) {
                errors.push(format!("backup-existing {}: {e}", toon_path.display()));
                continue;
            }
        }

        let backup = match engine::manifest::backup_file(&f.path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("backup {}: {e}", f.path_str));
                continue;
            }
        };

        let toon_tmp = toon_path.with_extension("toon.tmp");
        if let Err(e) = std::fs::write(&toon_tmp, &converted) {
            errors.push(format!("write {}: {e}", toon_tmp.display()));
            continue;
        }
        if let Err(e) = std::fs::rename(&toon_tmp, &toon_path) {
            let _ = std::fs::remove_file(&toon_tmp);
            errors.push(format!("rename {}: {e}", toon_path.display()));
            continue;
        }

        let entry = engine::manifest::ManifestEntry {
            id: engine::manifest::generate_id(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            original_path: f.path_str.clone(),
            backup_path: backup.display().to_string(),
            converted_path: toon_path.display().to_string(),
            original_bytes: content.len() as u64,
            converted_bytes: converted.len() as u64,
        };
        if let Err(e) = engine::manifest::write_manifest_entry(&entry) {
            // Roll back the .toon write — file should look unconverted.
            let _ = std::fs::remove_file(&toon_path);
            errors.push(format!("manifest {}: {e} (rolled back)", f.path_str));
            continue;
        }

        // From this point on, the conversion is durable. Track it in our
        // in-memory list so the App can mark it optimized even if the DB
        // write below fails.
        converted_paths.push(f.path_str.clone());
        converted_count += 1;
        let saved = estimate_tokens(content.len() as u64)
            .saturating_sub(estimate_tokens(converted.len() as u64));
        tokens_saved += saved as i64;

        if let Some(ref db) = db {
            if let Err(e) = db.insert_conversion(
                &f.path_str,
                "md_to_toon",
                content.len() as i64,
                converted.len() as i64,
                saved as i64,
            ) {
                errors.push(format!("db insert_conversion {}: {e}", f.path_str));
            }
            if let Err(e) = db.mark_optimized(
                &f.path_str,
                "md",
                "toon",
                content.len() as i64,
                converted.len() as i64,
            ) {
                errors.push(format!("db mark_optimized {}: {e}", f.path_str));
            }
        }
    }

    let _ = tx.send(OptMsg::Done(OptimizeResult {
        converted: converted_count,
        total,
        tokens_saved,
        errors,
        converted_paths,
        aborted,
    }));
}

/// Drain progress / completion messages from the worker. Returns true if a
/// redraw is needed.
fn drain_optimize_job(app: &mut App) -> bool {
    let job = match app.optimize_job.as_ref() {
        Some(j) => j,
        None => return false,
    };
    let mut redraw = false;
    let mut completion: Option<OptimizeResult> = None;

    loop {
        match job.rx.try_recv() {
            Ok(OptMsg::Progress {
                current,
                total,
                label,
            }) => {
                app.optimize_progress = Some(app::ProgressState {
                    current,
                    total,
                    label,
                    done: false,
                });
                redraw = true;
            }
            Ok(OptMsg::Done(res)) => {
                completion = Some(res);
                redraw = true;
                break;
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                // Worker died without sending Done — surface as error.
                completion = Some(OptimizeResult {
                    converted: 0,
                    total: job.total,
                    tokens_saved: 0,
                    errors: vec!["worker disconnected unexpectedly".to_string()],
                    converted_paths: Vec::new(),
                    aborted: false,
                });
                redraw = true;
                break;
            }
        }
    }

    if let Some(res) = completion {
        app.optimize_job = None;
        app.optimize_progress = None;

        // Idempotency: merge converted paths into the optimized set even if
        // the DB write failed. Subsequent OPTIMIZE clicks won't reconvert.
        for p in &res.converted_paths {
            app.optimized_paths.insert(p.clone());
        }

        let title = if res.aborted {
            "OPTIMIZATION CANCELLED"
        } else {
            "OPTIMIZATION COMPLETE"
        };
        let mut lines = vec![
            format!("{}/{} files converted", res.converted, res.total),
            format!("~{} tokens saved", res.tokens_saved),
        ];
        if res.errors.is_empty() {
            lines.push("No errors".to_string());
        } else {
            lines.push(format!("{} error(s):", res.errors.len()));
            for e in res.errors.iter().take(5) {
                lines.push(format!("  {e}"));
            }
            if res.errors.len() > 5 {
                lines.push(format!("  ... and {} more", res.errors.len() - 5));
            }
        }
        app.popup = Some(Popup::info(title, lines));
        app.refresh_scan();
    }

    redraw
}

fn run_restructure_inline(app: &mut App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    let base_dir = config::claude_dir();

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
            app.popup = Some(Popup::info(
                "RESTRUCTURE COMPLETE",
                if actions.is_empty() {
                    vec!["No actions needed.".to_string()]
                } else {
                    let mut lines: Vec<String> = actions.iter().take(8).cloned().collect();
                    if actions.len() > 8 {
                        lines.push(format!("... and {} more", actions.len() - 8));
                    }
                    lines.push(format!("{total} total actions"));
                    lines
                },
            ));
            app.refresh_trees();
        }
        Err(e) => {
            app.restructure_progress = None;
            app.popup = Some(Popup::info(
                "RESTRUCTURE ERROR",
                vec![format!("{e}")],
            ));
        }
    }
}

// =====================================================================
// Updater wiring (unchanged behavior; gated on no-progress/no-popup).
// =====================================================================

fn drain_update_check(app: &mut App) -> bool {
    use updater::CheckState;
    if app.popup.is_some()
        || app.optimize_progress.is_some()
        || app.restructure_progress.is_some()
        || app.optimize_job.is_some()
    {
        return false;
    }
    let mut guard = match app.update_check.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let state = std::mem::replace(&mut *guard, CheckState::Consumed);
    match state {
        CheckState::Available(release) => {
            let mut lines = vec![
                format!("New version available: {}", release.tag),
                format!("You're running: v{}", release.current),
                String::new(),
            ];
            if release.asset_url.is_some() {
                lines.push("Install update now?".to_string());
            } else {
                lines.push("No prebuilt binary for this platform.".to_string());
                lines.push(format!("Open: {}", release.html_url));
            }
            app.popup = Some(Popup {
                title: "UPDATE AVAILABLE".to_string(),
                lines,
                kind: PopupKind::UpdatePrompt {
                    tag: release.tag,
                    asset_url: release.asset_url,
                },
            });
            true
        }
        CheckState::Pending => {
            *guard = CheckState::Pending;
            false
        }
        _ => false,
    }
}

fn handle_update_accept(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    asset_url: Option<String>,
) -> Result<()> {
    let url = match asset_url {
        Some(u) => u,
        None => {
            app.popup = Some(Popup::info(
                "UPDATE",
                vec!["No installable asset for this platform.".to_string()],
            ));
            return Ok(());
        }
    };

    app.popup = Some(Popup::info(
        "UPDATING",
        vec!["Downloading new version...".to_string()],
    ));
    let _ = terminal.draw(|f| ui::render(f, app));

    match updater::download_and_install(&url) {
        Ok(path) => {
            app.pending_restart = Some(path);
            app.should_quit = true;
        }
        Err(e) => {
            app.popup = Some(Popup::info(
                "UPDATE FAILED",
                vec![format!("{e}"), "Press any key to continue.".to_string()],
            ));
        }
    }
    Ok(())
}

