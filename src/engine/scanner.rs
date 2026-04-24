use std::collections::HashSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use super::tokenizer::estimate_tokens;
use crate::config::claude_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileCategory {
    Agent,
    Rule,
    Skill,
    Memory,
    Command,
    TopLevel,
    Whitelisted,
}

impl std::fmt::Display for FileCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Agent => write!(f, "Agent"),
            Self::Rule => write!(f, "Rule"),
            Self::Skill => write!(f, "Skill"),
            Self::Memory => write!(f, "Memory"),
            Self::Command => write!(f, "Command"),
            Self::TopLevel => write!(f, "TopLevel"),
            Self::Whitelisted => write!(f, "Whitelisted"),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub category: FileCategory,
    pub size_bytes: u64,
    pub line_count: usize,
    pub token_estimate: u64,
    pub current_format: String,
    pub is_whitelisted: bool,
    pub is_optimized: bool,
}

/// Names that must never be converted.
const WHITELIST_NAMES: &[&str] = &[
    "settings.json",
    "settings.local.json",
    ".claude.json",
    "SKILL.md",
    "mcp-config.json",
    "CLAUDE.md",
    "MEMORY.md",
];

/// Extensions that must never be converted.
const WHITELIST_EXTS: &[&str] = &["sh", "js", "json"];

fn is_whitelisted(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if WHITELIST_NAMES.contains(&name) {
            return true;
        }
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if WHITELIST_EXTS.contains(&ext) {
            return true;
        }
    }
    // Whitelist entire skills directory for SKILL.md files
    let skills_dir = claude_dir().join("skills");
    if path.starts_with(&skills_dir) {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name == "SKILL.md" {
                return true;
            }
        }
    }
    false
}

fn classify(path: &Path) -> FileCategory {
    if is_whitelisted(path) {
        return FileCategory::Whitelisted;
    }

    let cd = claude_dir();
    let rules_dir = cd.join("rules");
    let memory_dir = cd.join("projects").join("-home-typhoon").join("memory");
    let agents_dir = cd.join("agents");
    let commands_dir = cd.join("commands");
    let skills_dir = cd.join("skills");

    if path.starts_with(&rules_dir) {
        FileCategory::Rule
    } else if path.starts_with(&memory_dir) {
        FileCategory::Memory
    } else if path.starts_with(&agents_dir) {
        FileCategory::Agent
    } else if path.starts_with(&commands_dir) {
        FileCategory::Command
    } else if path.starts_with(&skills_dir) {
        FileCategory::Skill
    } else {
        FileCategory::TopLevel
    }
}

/// Scan all relevant dirs under ~/.claude and return file metadata.
pub fn scan() -> Vec<ScannedFile> {
    let cd = claude_dir();
    let scan_paths = vec![
        cd.join("rules"),
        cd.join("projects").join("-home-typhoon").join("memory"),
        cd.join("agents"),
        cd.join("commands"),
        cd.join("skills"),
    ];

    let mut results = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for base in &scan_paths {
        if !base.exists() {
            continue;
        }
        for entry in WalkDir::new(base).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path().to_path_buf();
            if !path.is_file() {
                continue;
            }
            if seen.contains(&path) {
                continue;
            }
            seen.insert(path.clone());

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();

            // Only scan text-ish files
            if !matches!(
                ext.as_str(),
                "md" | "toon" | "json" | "jsonl" | "toml" | "yaml" | "yml" | "txt" | ""
            ) {
                continue;
            }

            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size = meta.len();
            let line_count = (size / 40) as usize;

            let category = classify(&path);
            let whitelisted = category == FileCategory::Whitelisted;

            results.push(ScannedFile {
                path,
                category,
                size_bytes: size,
                line_count,
                token_estimate: estimate_tokens(size),
                current_format: if ext.is_empty() {
                    "unknown".to_string()
                } else {
                    ext
                },
                is_whitelisted: whitelisted,
                is_optimized: false,
            });
        }
    }

    // Also scan top-level .claude/ files (non-recursive)
    if let Ok(entries) = std::fs::read_dir(&cd) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if seen.contains(&path) {
                continue;
            }
            seen.insert(path.clone());

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();

            if !matches!(ext.as_str(), "md" | "toon" | "json" | "toml" | "txt") {
                continue;
            }

            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size = meta.len();
            let line_count = (size / 40) as usize;

            let category = classify(&path);
            let whitelisted = category == FileCategory::Whitelisted;

            results.push(ScannedFile {
                path,
                category,
                size_bytes: size,
                line_count,
                token_estimate: estimate_tokens(size),
                current_format: ext,
                is_whitelisted: whitelisted,
                is_optimized: false,
            });
        }
    }

    results.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    results
}

/// Mark files as optimized if their path exists in the optimized set.
pub fn mark_optimized(files: &mut [ScannedFile], optimized: &HashSet<String>) {
    for f in files.iter_mut() {
        if optimized.contains(&f.path.display().to_string()) {
            f.is_optimized = true;
        }
    }
}

/// Aggregate stats per category.
#[derive(Debug, Clone, Default)]
pub struct CategoryStats {
    pub file_count: usize,
    pub total_bytes: u64,
    pub total_tokens: u64,
    pub convertible_count: usize,
    pub convertible_bytes: u64,
    pub convertible_tokens: u64,
}

pub fn aggregate_by_category(files: &[ScannedFile]) -> Vec<(FileCategory, CategoryStats)> {
    use std::collections::BTreeMap;

    let order = [
        FileCategory::Rule,
        FileCategory::Memory,
        FileCategory::Agent,
        FileCategory::Skill,
        FileCategory::Command,
        FileCategory::TopLevel,
        FileCategory::Whitelisted,
    ];

    let mut map: BTreeMap<u8, (FileCategory, CategoryStats)> = BTreeMap::new();
    for (i, cat) in order.iter().enumerate() {
        map.insert(i as u8, (*cat, CategoryStats::default()));
    }

    for f in files {
        let idx = order.iter().position(|c| *c == f.category).unwrap_or(5) as u8;
        let entry = map
            .entry(idx)
            .or_insert((f.category, CategoryStats::default()));
        entry.1.file_count += 1;
        entry.1.total_bytes += f.size_bytes;
        entry.1.total_tokens += f.token_estimate;
        if !f.is_whitelisted && !f.is_optimized && f.current_format == "md" {
            entry.1.convertible_count += 1;
            entry.1.convertible_bytes += f.size_bytes;
            entry.1.convertible_tokens += f.token_estimate;
        }
    }

    map.into_values().collect()
}
