use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use crate::engine::converter;

use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use crate::config::Config;
use crate::db::Db;
use crate::engine::restructure::{self, TreeNode};
use crate::engine::scanner::{self, CategoryStats, FileCategory, ScannedFile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Compression,
    Status,
    Log,
    Efficiency,
    Restructure,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Compression,
        Tab::Status,
        Tab::Log,
        Tab::Efficiency,
        Tab::Restructure,
        Tab::Settings,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Compression => "1 COMPRESS",
            Tab::Status => "2 STATUS",
            Tab::Log => "3 LOG",
            Tab::Efficiency => "4 TOKENS",
            Tab::Restructure => "5 RESTRUCTURE",
            Tab::Settings => "6 SETTINGS",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Compression => 0,
            Tab::Status => 1,
            Tab::Log => 2,
            Tab::Efficiency => 3,
            Tab::Restructure => 4,
            Tab::Settings => 5,
        }
    }
}

/// Clickable region tracker
#[derive(Debug, Default, Clone)]
pub struct ClickAreas {
    pub tab_rects: [Rect; 6],
    pub content_area: Rect,
    pub optimize_button: Rect,
    pub restructure_button: Rect,
    pub popup_yes: Rect,
    pub popup_no: Rect,
}

/// Progress state for long-running operations
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProgressState {
    pub current: usize,
    pub total: usize,
    pub label: String,
    pub done: bool,
}

/// Popup overlay
#[derive(Debug, Clone)]
pub struct Popup {
    pub title: String,
    pub lines: Vec<String>,
    pub kind: PopupKind,
}

#[derive(Debug, Clone)]
pub enum PopupKind {
    /// Dismiss on any key/click.
    Info,
    /// Two-button prompt asking the user to install an available update.
    UpdatePrompt {
        #[allow(dead_code)]
        tag: String,
        asset_url: Option<String>,
    },
}

impl Popup {
    pub fn info(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            lines,
            kind: PopupKind::Info,
        }
    }
}

/// Pending action signalled by a tab for the main loop to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    Optimize,
    Restructure,
}

/// Outcome handed back from the optimize worker thread to the UI.
#[derive(Debug)]
pub struct OptimizeResult {
    pub converted: usize,
    pub total: usize,
    pub tokens_saved: i64,
    pub errors: Vec<String>,
    /// Files whose conversion durably succeeded (manifest written). The UI
    /// merges these into `optimized_paths` even if the SQLite write below
    /// failed, so subsequent runs cannot reconvert them.
    pub converted_paths: Vec<String>,
    pub aborted: bool,
}

/// Worker → UI channel messages.
#[derive(Debug)]
pub enum OptMsg {
    Progress {
        current: usize,
        total: usize,
        label: String,
    },
    Done(OptimizeResult),
}

/// Handle to an in-flight optimize job. Presence == "a worker is running"
/// (used to make OPTIMIZE clicks idempotent).
pub struct OptimizeJob {
    pub rx: Receiver<OptMsg>,
    pub abort: Arc<AtomicBool>,
    pub total: usize,
}

impl std::fmt::Debug for OptimizeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptimizeJob")
            .field("total", &self.total)
            .finish()
    }
}

#[allow(dead_code)]
pub struct App {
    pub current_tab: Tab,
    pub should_quit: bool,
    pub config: Config,
    pub db: Option<Db>,
    pub scan_result: Vec<ScannedFile>,
    pub category_stats: Vec<(FileCategory, CategoryStats)>,
    pub last_scan_time: Option<String>,
    pub total_tokens_saved: i64,
    pub click_areas: ClickAreas,

    // Tab 1: Status — uses files_table_state for the files sub-table
    pub files_table_state: TableState,

    // Tab 2: Log
    pub log_table_state: TableState,

    // Tab 3: Compression
    pub compression_selected: usize,
    pub compression_file_indices: Vec<usize>,
    pub compression_overrides: HashMap<PathBuf, u8>,
    pub compression_list_state: ratatui::widgets::ListState,

    // Tab 5: Settings
    pub settings_selected: usize,
    pub settings_status: Option<String>,
    pub timer_installed: bool,
    pub hook_installed: bool,

    // Tab 6: Restructure
    pub current_tree: Vec<TreeNode>,
    pub proposed_tree: Vec<TreeNode>,
    pub restructure_scroll: u16,

    // Tab 1: Status scroll
    pub status_scroll: u16,

    // Tab 1: Compression focus (0=FileList, 1=Level, 2=Apply, 3=Optimize)
    pub compress_focus: u8,

    // Tab 5: Restructure focus (0=tree, 1=apply button)
    pub restructure_focus: u8,

    // Optimized files tracking
    pub optimized_paths: HashSet<String>,

    // Progress + popup
    pub optimize_progress: Option<ProgressState>,
    pub restructure_progress: Option<ProgressState>,
    pub popup: Option<Popup>,

    // Pending action signal (replaces magic string "__optimize__"/"__restructure__")
    pub pending_action: Option<PendingAction>,

    // Preview cache for compression tab
    pub preview_cache_path: Option<PathBuf>,
    pub preview_cache_level: u8,
    pub preview_before: Vec<String>,
    pub preview_after: Vec<String>,

    // Cached conversion count for efficiency tab
    pub conversion_count: usize,

    // Update checker — populated by background thread, drained on tick
    pub update_check: crate::updater::SharedCheck,
    /// If set, the main loop will quit and re-exec this binary on shutdown.
    pub pending_restart: Option<PathBuf>,

    /// Active optimize worker, if any. None == idle.
    pub optimize_job: Option<OptimizeJob>,
}

impl App {
    /// Construct using a pre-loaded config (preferred — discovery has already run).
    pub fn with_config(config: Config) -> Self {
        Self::build(config)
    }

    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::build(Config::load().unwrap_or_default())
    }

    fn build(config: Config) -> Self {
        let db = Db::open().ok();
        let total_tokens_saved = db
            .as_ref()
            .and_then(|d| d.total_tokens_saved().ok())
            .unwrap_or(0);

        let timer_installed = crate::daemon::timer_marker_path().exists();
        let hook_installed = crate::daemon::hook_path().exists();

        let optimized_paths = db
            .as_ref()
            .and_then(|d| d.get_optimized_paths().ok())
            .unwrap_or_default();

        let mut app = Self {
            current_tab: Tab::Compression,
            should_quit: false,
            config,
            db,
            scan_result: Vec::new(),
            category_stats: Vec::new(),
            last_scan_time: None,
            total_tokens_saved,
            click_areas: ClickAreas::default(),
            files_table_state: TableState::default(),
            log_table_state: TableState::default(),
            compression_selected: 0,
            compression_file_indices: Vec::new(),
            compression_overrides: HashMap::new(),
            compression_list_state: {
                let mut s = ratatui::widgets::ListState::default();
                s.select(Some(0));
                s
            },
            settings_selected: 0,
            settings_status: None,
            timer_installed,
            hook_installed,
            current_tree: Vec::new(),
            proposed_tree: Vec::new(),
            restructure_scroll: 0,
            status_scroll: 0,
            compress_focus: 0,
            restructure_focus: 0,
            optimized_paths,
            optimize_progress: None,
            restructure_progress: None,
            popup: None,
            pending_action: None,
            preview_cache_path: None,
            preview_cache_level: 0,
            preview_before: Vec::new(),
            preview_after: Vec::new(),
            conversion_count: 0,
            update_check: crate::updater::new_state(),
            pending_restart: None,
            optimize_job: None,
        };
        crate::updater::spawn_check(app.update_check.clone());
        app.refresh_scan();
        app.refresh_trees();
        app
    }

    pub fn refresh_scan(&mut self) {
        self.scan_result = scanner::scan();
        scanner::mark_optimized(&mut self.scan_result, &self.optimized_paths);
        self.category_stats = scanner::aggregate_by_category(&self.scan_result);
        self.last_scan_time = Some(chrono::Local::now().format("%H:%M:%S").to_string());
        if let Some(ref db) = self.db {
            self.total_tokens_saved = db.total_tokens_saved().unwrap_or(0);
            self.conversion_count = db.count_conversions().unwrap_or(0);
        }
        // Invalidate preview cache since scan results changed
        self.preview_cache_path = None;
    }

    pub fn refresh_trees(&mut self) {
        self.current_tree = restructure::build_current_tree();
        self.proposed_tree = restructure::build_proposed_tree(&self.scan_result);
    }

    pub fn next_tab(&mut self) {
        let idx = self.current_tab.index();
        let next = (idx + 1) % Tab::ALL.len();
        self.current_tab = Tab::ALL[next];
    }

    pub fn prev_tab(&mut self) {
        let idx = self.current_tab.index();
        let prev = if idx == 0 {
            Tab::ALL.len() - 1
        } else {
            idx - 1
        };
        self.current_tab = Tab::ALL[prev];
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.current_tab = tab;
    }

    pub fn scroll_log(&mut self, delta: i32) {
        let max_rows = self
            .db
            .as_ref()
            .and_then(|db| db.count_conversions().ok())
            .unwrap_or(0);
        if max_rows == 0 {
            return;
        }
        let current = self.log_table_state.selected().unwrap_or(0);
        let next = if delta > 0 {
            (current.saturating_add(delta as usize)).min(max_rows.saturating_sub(1))
        } else {
            current.saturating_sub(delta.unsigned_abs() as usize)
        };
        self.log_table_state.select(Some(next));
    }

    pub fn scroll_compression(&mut self, delta: i32) {
        let len = self.compression_file_indices.len();
        if len == 0 {
            return;
        }
        let current = self.compression_selected;
        let next = if delta > 0 {
            (current + delta as usize).min(len.saturating_sub(1))
        } else {
            current.saturating_sub(delta.unsigned_abs() as usize)
        };
        self.compression_selected = next;
        self.compression_list_state.select(Some(next));
    }

    pub fn adjust_compression_level(&mut self, delta: i8) {
        if let Some(&scan_idx) = self.compression_file_indices.get(self.compression_selected) {
            if let Some(file) = self.scan_result.get(scan_idx) {
                let current = self
                    .compression_overrides
                    .get(&file.path)
                    .copied()
                    .unwrap_or(self.config.compression_default);
                let new_level = (current as i8 + delta).clamp(1, 4) as u8;
                self.compression_overrides
                    .insert(file.path.clone(), new_level);
            }
        }
    }

    pub fn get_selected_compression_level(&self) -> u8 {
        if let Some(&scan_idx) = self.compression_file_indices.get(self.compression_selected) {
            if let Some(file) = self.scan_result.get(scan_idx) {
                return self
                    .compression_overrides
                    .get(&file.path)
                    .copied()
                    .unwrap_or(self.config.compression_default);
            }
        }
        self.config.compression_default
    }

    pub fn total_original_bytes(&self) -> u64 {
        self.scan_result.iter().map(|f| f.size_bytes).sum()
    }

    pub fn total_convertible_files(&self) -> usize {
        self.scan_result
            .iter()
            .filter(|f| !f.is_whitelisted && !f.is_optimized && f.current_format == "md")
            .count()
    }

    pub fn estimated_savings_ratio(&self) -> f64 {
        0.35
    }

    pub fn scroll_status(&mut self, delta: i32) {
        if delta > 0 {
            self.status_scroll = self.status_scroll.saturating_add(delta as u16);
        } else {
            self.status_scroll = self
                .status_scroll
                .saturating_sub(delta.unsigned_abs() as u16);
        }
    }

    /// Return cached preview lines (before, after) for the selected file + compression level.
    /// Reads from disk only when the cache is stale.
    pub fn get_preview(&mut self) -> (Vec<String>, Vec<String>) {
        let scan_idx = self
            .compression_file_indices
            .get(self.compression_selected)
            .copied();

        let (path, category) = match scan_idx.and_then(|i| self.scan_result.get(i)) {
            Some(file) => (file.path.clone(), file.category),
            None => return (vec!["No file selected".to_string()], vec![]),
        };

        let level = self.get_selected_compression_level();

        // Check cache validity
        if self.preview_cache_path.as_ref() == Some(&path) && self.preview_cache_level == level {
            return (self.preview_before.clone(), self.preview_after.clone());
        }

        // Cache miss: read + convert
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let before: Vec<String> = content.lines().take(10).map(|l| l.to_string()).collect();
        let converted =
            converter::convert_md_to_toon(&content, converter::CompressionLevel(level), category);
        let after: Vec<String> = converted.lines().take(10).map(|l| l.to_string()).collect();

        self.preview_cache_path = Some(path);
        self.preview_cache_level = level;
        self.preview_before = before.clone();
        self.preview_after = after.clone();

        (before, after)
    }

    pub fn scroll_restructure(&mut self, delta: i32) {
        if delta > 0 {
            self.restructure_scroll = self.restructure_scroll.saturating_add(delta as u16);
        } else {
            self.restructure_scroll = self
                .restructure_scroll
                .saturating_sub(delta.unsigned_abs() as u16);
        }
    }
}
