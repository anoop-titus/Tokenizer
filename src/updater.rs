//! GitHub release auto-update.
//!
//! On startup we spawn a background thread that hits the GitHub releases API.
//! When a newer version exists the TUI surfaces a clickable popup. If the user
//! accepts, we download the matching asset, atomically replace the running
//! binary, and re-exec.

use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const REPO: &str = "anoop-titus/Tokenizer";
const USER_AGENT: &str = concat!("tokenizer-updater/", env!("CARGO_PKG_VERSION"));
const TIMEOUT_SECS: u64 = 8;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Release {
    pub tag: String,
    pub current: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub html_url: String,
}

#[derive(Debug, Default)]
#[allow(dead_code)]
pub enum CheckState {
    #[default]
    Pending,
    UpToDate,
    Available(Release),
    Failed(String),
    Consumed,
}

pub type SharedCheck = Arc<Mutex<CheckState>>;

pub fn new_state() -> SharedCheck {
    Arc::new(Mutex::new(CheckState::Pending))
}

/// Spawn a background thread that fetches the latest release. Never panics.
pub fn spawn_check(state: SharedCheck) {
    thread::spawn(move || {
        let result = fetch_latest();
        let mut guard = state.lock().expect("update check mutex poisoned");
        *guard = match result {
            Ok(Some(r)) => CheckState::Available(r),
            Ok(None) => CheckState::UpToDate,
            Err(e) => CheckState::Failed(e.to_string()),
        };
    });
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// Public for `tokenizer update` — same fetch the background thread uses.
pub fn fetch_latest() -> Result<Option<Release>> {
    fetch_latest_inner()
}

/// One-shot update path used by the `tokenizer update` subcommand. Prints
/// progress to stderr, downloads the matching asset, replaces the running
/// binary, and execs the new one with the user's args minus `update`. With
/// `force=true` the install runs even if remote == local (useful for
/// reinstalling a corrupt binary).
pub fn run_update_cli(force: bool) -> Result<()> {
    eprintln!("Checking GitHub for latest release...");
    let release = match fetch_latest()? {
        Some(r) => r,
        None if force => {
            eprintln!("Forcing reinstall of current version.");
            // We still need a release to know the asset URL — fetch raw.
            match fetch_raw_release()? {
                Some(r) => r,
                None => bail!("no releases found at github.com/{REPO}"),
            }
        }
        None => {
            eprintln!("Already on the latest version (v{}).", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    };

    eprintln!("Latest: {} (you have v{})", release.tag, release.current);

    let url = match release.asset_url.as_deref() {
        Some(u) => u,
        None => {
            eprintln!("No prebuilt binary for this platform ({}/{}).",
                std::env::consts::OS, std::env::consts::ARCH);
            eprintln!("See: {}", release.html_url);
            bail!("no installable asset for this platform");
        }
    };

    eprintln!(
        "Downloading {}...",
        release.asset_name.as_deref().unwrap_or("asset")
    );
    let new_binary = download_and_install(url)?;
    eprintln!("Installed to {}.", new_binary.display());
    eprintln!("Restarting tokenizer...");
    // restart() never returns on success.
    restart_for_cli(&new_binary)
}

/// Like `restart()` but strips the leading `update` arg so we don't loop.
fn restart_for_cli(binary: &PathBuf) -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("update")) {
        args.remove(0);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(binary).args(&args).exec();
        bail!("exec failed: {err}");
    }

    #[cfg(not(unix))]
    {
        std::process::Command::new(binary)
            .args(&args)
            .spawn()
            .context("spawning new binary")?;
        std::process::exit(0);
    }
}

/// Fetch the latest release without comparing versions. Used by `--force`.
fn fetch_raw_release() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(TIMEOUT_SECS))
        .build();
    let resp = agent
        .get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
        .context("github releases request failed")?;
    let release: ApiRelease = resp.into_json().context("invalid github response")?;
    let (asset_name, asset_url) = match pick_asset(&release.assets) {
        Some(a) => (Some(a.name.clone()), Some(a.browser_download_url.clone())),
        None => (None, None),
    };
    Ok(Some(Release {
        tag: release.tag_name,
        current: env!("CARGO_PKG_VERSION").to_string(),
        asset_name,
        asset_url,
        html_url: release.html_url,
    }))
}

fn fetch_latest_inner() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(TIMEOUT_SECS))
        .build();

    let resp = agent
        .get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
        .context("github releases request failed")?;

    let release: ApiRelease = resp.into_json().context("invalid github response")?;
    let current = env!("CARGO_PKG_VERSION").to_string();

    if !is_newer(&release.tag_name, &current) {
        return Ok(None);
    }

    let (asset_name, asset_url) = match pick_asset(&release.assets) {
        Some(a) => (Some(a.name.clone()), Some(a.browser_download_url.clone())),
        None => (None, None),
    };

    Ok(Some(Release {
        tag: release.tag_name,
        current,
        asset_name,
        asset_url,
        html_url: release.html_url,
    }))
}

/// Strip a leading `v` and parse `MAJOR.MINOR.PATCH` (extra suffixes ignored).
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(|c: char| c == '-' || c == '+').next()?;
    let mut parts = core.split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next().unwrap_or("0").parse().ok()?;
    let c = parts.next().unwrap_or("0").parse().ok()?;
    Some((a, b, c))
}

fn is_newer(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

fn pick_asset(assets: &[ApiAsset]) -> Option<&ApiAsset> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let os_tags: &[&str] = match os {
        "linux" => &["linux"],
        "macos" => &["macos", "darwin", "apple"],
        "windows" => &["windows", "win"],
        _ => &[],
    };
    let arch_tags: &[&str] = match arch {
        "x86_64" => &["x86_64", "amd64", "x64"],
        "aarch64" => &["aarch64", "arm64"],
        _ => &[],
    };

    assets.iter().find(|a| {
        let lower = a.name.to_lowercase();
        os_tags.iter().any(|t| lower.contains(t))
            && arch_tags.iter().any(|t| lower.contains(t))
    })
}

/// Download `asset_url` and atomically replace the current executable.
/// Returns the path of the new binary (which equals the original `current_exe`).
pub fn download_and_install(asset_url: &str) -> Result<PathBuf> {
    let current = std::env::current_exe().context("locating current binary")?;
    let dir = current
        .parent()
        .ok_or_else(|| anyhow!("current_exe has no parent"))?;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let staging = dir.join(format!(".tokenizer.update.{stamp}"));

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(60 * 5))
        .build();

    let resp = agent
        .get(asset_url)
        .set("User-Agent", USER_AGENT)
        .call()
        .context("download request failed")?;

    let mut reader = resp.into_reader();
    let mut bytes = Vec::with_capacity(8 * 1024 * 1024);
    reader
        .read_to_end(&mut bytes)
        .context("download stream failed")?;

    if bytes.len() < 1024 {
        bail!("downloaded asset suspiciously small ({} bytes)", bytes.len());
    }

    std::fs::write(&staging, &bytes).context("writing staged binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&staging)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&staging, perms)?;
    }

    // On Unix, rename over a running binary works (the kernel keeps the open
    // inode alive). On Windows the file is locked; move-aside first.
    #[cfg(windows)]
    {
        let backup = dir.join(format!(".tokenizer.old.{stamp}"));
        let _ = std::fs::rename(&current, &backup);
    }

    std::fs::rename(&staging, &current).context("replacing current binary")?;
    Ok(current)
}

/// Replace the current process image with the (now-updated) binary, preserving args.
/// Only returns on error.
pub fn restart(binary: &PathBuf) -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(binary).args(&args).exec();
        bail!("exec failed: {err}");
    }

    #[cfg(not(unix))]
    {
        std::process::Command::new(binary)
            .args(&args)
            .spawn()
            .context("spawning new binary")?;
        std::process::exit(0);
    }
}
