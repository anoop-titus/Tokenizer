use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Return the path to ~/.claude. Shared utility to avoid duplication.
pub fn claude_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub frequency_minutes: u32,
    pub auto_optimize_post_session: bool,
    pub categories: HashMap<String, CategoryEntry>,
    pub compression_default: u8,
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
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        let new_dir = base.join("tokenizer");
        let legacy_dir = base.join("claude-optimizer");

        // One-time migration: if the legacy dir exists and the new one doesn't,
        // move it so existing history/config/manifests carry over.
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
}
