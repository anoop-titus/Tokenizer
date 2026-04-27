//! Claude directory discovery + selection.
//!
//! On startup the binary must know *which* `.claude` tree to operate on.
//! Discovery runs before raw mode is enabled (so fzf can use the terminal):
//!
//! 1. CLI flag `--claude-dir <path>` wins unconditionally.
//! 2. Else if config has a saved `claude_dir` and it still validates → use it.
//! 3. Else collect all candidates from canonical locations and the env.
//! 4. If `--reselect-dir`, or if there's >1 candidate, present an fzf picker.
//!    If fzf is missing, fall back to a numeric stdin prompt.
//! 5. Persist the choice into config.toml so subsequent runs skip the picker.
//!
//! All later code reads the choice through `config::claude_dir()`.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};

use crate::config::Config;

/// Result of resolution. The chosen path is also pinned via
/// `config::set_claude_dir`.
pub struct Resolution {
    pub path: PathBuf,
    /// True if the user (or config) made an explicit choice — we should
    /// persist it. False if it was an auto-fallback (e.g. only one candidate
    /// and it matches the default).
    pub persist: bool,
}

/// Top-level entry. `cli_override`/`reselect` come from CLI flags.
pub fn resolve_claude_dir(
    config: &mut Config,
    cli_override: Option<PathBuf>,
    reselect: bool,
) -> Result<PathBuf> {
    // 1. CLI override.
    if let Some(p) = cli_override {
        let p = canonicalize_or(&p);
        if !Config::looks_like_claude_dir(&p) {
            bail!(
                "--claude-dir {} does not look like a Claude directory",
                p.display()
            );
        }
        crate::config::set_claude_dir(p.clone());
        config.claude_dir = Some(p.clone());
        config.save().ok();
        return Ok(p);
    }

    // 2. Config remembered choice.
    if !reselect {
        if let Some(saved) = config.claude_dir.clone() {
            if Config::looks_like_claude_dir(&saved) {
                crate::config::set_claude_dir(saved.clone());
                return Ok(saved);
            }
            // Saved but stale — fall through to rediscovery.
            eprintln!(
                "warn: saved claude_dir {} no longer valid; rediscovering",
                saved.display()
            );
        }
    }

    // 3. Discover candidates.
    let candidates = collect_candidates();
    if candidates.is_empty() {
        bail!(
            "No .claude directory found. Set $CLAUDE_DIR, create ~/.claude, \
             or pass --claude-dir <path>."
        );
    }

    // 4. Pick.
    let chosen = if candidates.len() == 1 && !reselect {
        candidates.into_iter().next().unwrap()
    } else {
        pick(&candidates).context("Claude directory selection failed")?
    };

    let res = Resolution {
        path: chosen.clone(),
        persist: true,
    };
    if res.persist {
        config.claude_dir = Some(res.path.clone());
        config.save().ok();
    }
    crate::config::set_claude_dir(res.path.clone());
    Ok(res.path)
}

fn canonicalize_or(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Gather every plausible Claude directory we can find.
fn collect_candidates() -> Vec<PathBuf> {
    let mut set: BTreeSet<PathBuf> = BTreeSet::new();
    let mut try_add = |p: PathBuf| {
        let p = canonicalize_or(&p);
        if Config::looks_like_claude_dir(&p) {
            set.insert(p);
        }
    };

    // Canonical: ~/.claude
    if let Some(home) = dirs::home_dir() {
        try_add(home.join(".claude"));
    }

    // Env override.
    if let Ok(env_dir) = std::env::var("CLAUDE_DIR") {
        if !env_dir.is_empty() {
            try_add(PathBuf::from(env_dir));
        }
    }
    if let Ok(env_dir) = std::env::var("CLAUDE_HOME") {
        if !env_dir.is_empty() {
            try_add(PathBuf::from(env_dir));
        }
    }

    // XDG.
    if let Some(xdg) = dirs::config_dir() {
        try_add(xdg.join("claude"));
    }

    // Walk up from CWD looking for a `.claude` (project-local).
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors().take(8) {
            try_add(ancestor.join(".claude"));
        }
    }

    set.into_iter().collect()
}

/// Present the picker. Tries `fzf` first; if the binary is missing or fails,
/// falls back to a numbered stdin prompt.
fn pick(candidates: &[PathBuf]) -> Result<PathBuf> {
    match try_fzf(candidates) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => Err(anyhow!("selection cancelled")),
        Err(_) => fallback_prompt(candidates),
    }
}

fn try_fzf(candidates: &[PathBuf]) -> Result<Option<PathBuf>> {
    let mut child = Command::new("fzf")
        .arg("--prompt=Select Claude dir > ")
        .arg("--height=40%")
        .arg("--reverse")
        .arg("--no-multi")
        .arg("--header=Tokenizer needs to know which Claude directory to scan")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("spawning fzf")?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("no fzf stdin"))?;
        for p in candidates {
            writeln!(stdin, "{}", p.display())?;
        }
    }

    let output = child.wait_with_output().context("waiting on fzf")?;
    if !output.status.success() {
        // Exit 130 = ctrl-c, exit 1 = no match. Either way: cancelled.
        return Ok(None);
    }
    let line = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();
    if line.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(line)))
}

fn fallback_prompt(candidates: &[PathBuf]) -> Result<PathBuf> {
    eprintln!("\nfzf not available — choose a Claude directory:\n");
    for (i, p) in candidates.iter().enumerate() {
        eprintln!("  [{}] {}", i + 1, p.display());
    }
    eprint!("\nSelection [1]: ");
    std::io::stderr().flush().ok();

    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("reading selection")?;
    let trimmed = line.trim();
    let idx: usize = if trimmed.is_empty() {
        1
    } else {
        trimmed
            .parse()
            .map_err(|_| anyhow!("invalid selection: {trimmed:?}"))?
    };
    if idx == 0 || idx > candidates.len() {
        bail!("selection {idx} out of range");
    }
    Ok(candidates[idx - 1].clone())
}
