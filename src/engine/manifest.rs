use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub timestamp: String,
    pub original_path: String,
    pub backup_path: String,
    pub converted_path: String,
    pub original_bytes: u64,
    pub converted_bytes: u64,
}

fn backups_dir() -> PathBuf {
    Config::config_dir().join("backups")
}

fn manifest_path() -> PathBuf {
    Config::config_dir().join("manifest.jsonl")
}

/// Create backup of original file before conversion.
/// Returns the backup path.
pub fn backup_file(original: &Path) -> Result<PathBuf> {
    let dir = backups_dir();
    std::fs::create_dir_all(&dir)?;

    let ts = Utc::now().format("%Y%m%d_%H%M%S_%3f");
    let nanos: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let file_name = original
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let backup_name = format!("{ts}_{:04x}_{file_name}", nanos & 0xFFFF);
    let backup_path = dir.join(backup_name);

    std::fs::copy(original, &backup_path)?;
    Ok(backup_path)
}

/// Append a manifest entry recording the conversion.
pub fn write_manifest_entry(entry: &ManifestEntry) -> Result<()> {
    let path = manifest_path();
    let line = serde_json::to_string(entry)? + "\n";
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Generate a unique manifest ID with timestamp + random suffix.
pub fn generate_id() -> String {
    let ts = Utc::now().format("%Y%m%d%H%M%S%3f");
    let rand: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("conv_{ts}_{:04x}", rand & 0xFFFF)
}

/// Read all manifest entries.
#[allow(dead_code)]
pub fn read_manifest() -> Result<Vec<ManifestEntry>> {
    let path = manifest_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ManifestEntry>(line) {
            Ok(e) => entries.push(e),
            Err(_) => continue,
        }
    }
    Ok(entries)
}
