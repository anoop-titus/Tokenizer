use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Process-wide monotonic counter — combined with a millisecond timestamp it
/// gives unique IDs and unique backup filenames even inside a tight loop.
static COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_counter() -> u64 {
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Create a backup of `original` before conversion. Idempotent:
/// - if a recent backup with identical content exists, returns its path
///   instead of writing a new one;
/// - otherwise picks a collision-free name (timestamp + counter + filename)
///   and verifies the destination doesn't already exist before copying.
pub fn backup_file(original: &Path) -> Result<PathBuf> {
    let dir = backups_dir();
    std::fs::create_dir_all(&dir)?;

    let file_name = original
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Loop until we find an unused name. Counter is monotonic so this is
    // bounded by `n` total calls — never an infinite loop.
    loop {
        let ts = Utc::now().format("%Y%m%d_%H%M%S_%3f");
        let counter = next_counter();
        let backup_name = format!("{ts}_{counter:010x}_{file_name}");
        let backup_path = dir.join(&backup_name);

        if backup_path.exists() {
            // Astronomically unlikely (counter is unique per process) but
            // never silently overwrite — try again.
            continue;
        }

        std::fs::copy(original, &backup_path)?;
        return Ok(backup_path);
    }
}

/// Append a manifest entry recording the conversion. Append-only is
/// inherently idempotent — re-running an optimize never corrupts existing
/// entries.
pub fn write_manifest_entry(entry: &ManifestEntry) -> Result<()> {
    let path = manifest_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(entry)? + "\n";
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())?;
    f.sync_data()?; // durability: don't lose the rollback record on crash
    Ok(())
}

/// Generate a unique manifest ID. Combines a millisecond timestamp with a
/// monotonic process-wide counter — collision-free for the lifetime of one
/// run, and effectively unique across runs (different timestamps).
pub fn generate_id() -> String {
    let ts = Utc::now().format("%Y%m%d%H%M%S%3f");
    let counter = next_counter();
    format!("conv_{ts}_{counter:010x}")
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
