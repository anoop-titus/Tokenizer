use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Process-wide pinned Claude directory. Set once at startup by
/// `discover::resolve_claude_dir`; all later calls to `claude_dir()` read it.
static CLAUDE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Default fallback when no config + no discovery run yet.
fn default_claude_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
}

/// Pin the active Claude directory for the rest of the process. Idempotent —
/// later calls are silently ignored (OnceLock semantics).
pub fn set_claude_dir(p: PathBuf) {
    let _ = CLAUDE_DIR.set(p);
}

/// Return the active Claude directory. Falls back to `~/.claude` if discovery
/// has not run (e.g. unit tests, headless paths that bypass startup).
pub fn claude_dir() -> PathBuf {
    CLAUDE_DIR.get().cloned().unwrap_or_else(default_claude_dir)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub frequency_minutes: u32,
    pub auto_optimize_post_session: bool,
    pub categories: HashMap<String, CategoryEntry>,
    pub compression_default: u8,
    /// Persisted Claude directory selection. None on first run; populated by
    /// the discovery flow once the user picks (or the single candidate is
    /// auto-accepted).
    #[serde(default)]
    pub claude_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryEntry {
    pub enabled: bool,
    pub target_format: TargetFormat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TargetFormat {
    Toon,
    Json,
    Jsonl,
}

impl std::fmt::Display for TargetFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toon => write!(f, "toon"),
            Self::Json => write!(f, "json"),
            Self::Jsonl => write!(f, "jsonl"),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut categories = HashMap::new();
        for name in ["agents", "rules", "skills", "memory", "commands"] {
            categories.insert(
                name.to_string(),
                CategoryEntry {
                    enabled: true,
                    target_format: TargetFormat::Toon,
                },
            );
        }
        Self {
            frequency_minutes: 60,
            auto_optimize_post_session: true,
            categories,
            compression_default: 2,
            claude_dir: None,
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        let new_dir = base.join("tokenizer");
        let legacy_dir = base.join("claude-optimizer");

        if legacy_dir.exists() && !new_dir.exists() {
            let _ = std::fs::rename(&legacy_dir, &new_dir);
        }

        new_dir
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(toml::from_str(&content)?)
        } else {
            let cfg = Self::default();
            cfg.save()?;
            Ok(cfg)
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        std::fs::write(Self::config_path(), toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Validate that a candidate path looks like a Claude directory:
    /// must exist, be a dir, and contain at least one of the canonical
    /// subdirs (agents/skills/rules/commands) or top-level CLAUDE.md.
    pub fn looks_like_claude_dir(p: &Path) -> bool {
        if !p.is_dir() {
            return false;
        }
        let markers = ["agents", "skills", "rules", "commands"];
        if markers.iter().any(|m| p.join(m).is_dir()) {
            return true;
        }
        p.join("CLAUDE.md").is_file()
    }
}
