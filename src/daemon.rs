use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::engine::manifest;

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::env::temp_dir())
}

/// Resolve the tokenizer binary path.
/// Prefers the currently running exe, falls back to PATH lookup by name.
fn optimizer_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| {
        if cfg!(windows) {
            PathBuf::from("tokenizer.exe")
        } else {
            PathBuf::from("tokenizer")
        }
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// Public paths used by app.rs for "installed?" detection
// ──────────────────────────────────────────────────────────────────────────────

pub fn timer_marker_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        home_dir().join(".config/systemd/user/tokenizer.timer")
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library/LaunchAgents/com.tokenizer.plist")
    }
    #[cfg(target_os = "windows")]
    {
        // Task Scheduler has no file; use a sentinel written at install time.
        home_dir().join("AppData/Local/tokenizer/.timer-installed")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        home_dir().join(".config/tokenizer/.timer-installed")
    }
}

pub fn hook_path() -> PathBuf {
    let dir = home_dir().join(".claude/hooks");
    if cfg!(windows) {
        dir.join("tokenizer-post-session.ps1")
    } else {
        dir.join("tokenizer-post-session.sh")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// install_timer — dispatches to the platform-specific backend
// ──────────────────────────────────────────────────────────────────────────────

pub fn install_timer() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        install_timer_linux()
    }
    #[cfg(target_os = "macos")]
    {
        install_timer_macos()
    }
    #[cfg(target_os = "windows")]
    {
        install_timer_windows()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("Periodic timer install is not supported on this OS. Run `tokenizer optimize` from your own scheduler.")
    }
}

#[cfg(target_os = "linux")]
fn install_timer_linux() -> Result<()> {
    let dir = home_dir().join(".config/systemd/user");
    std::fs::create_dir_all(&dir).context("Failed to create systemd user dir")?;

    let exe = optimizer_exe();
    let service_content = format!(
        "[Unit]\n\
         Description=Tokenizer (Claude Code)\n\n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={} optimize --quiet\n\
         Environment=HOME=%h\n\n\
         [Install]\n\
         WantedBy=default.target\n",
        exe.display()
    );

    let timer_content = "[Unit]\n\
         Description=Run Claude Optimizer periodically\n\n\
         [Timer]\n\
         OnUnitActiveSec=3600\n\
         OnBootSec=300\n\
         Persistent=true\n\n\
         [Install]\n\
         WantedBy=timers.target\n";

    let service_path = dir.join("tokenizer.service");
    let timer_path = dir.join("tokenizer.timer");

    std::fs::write(&service_path, service_content)
        .context("Failed to write tokenizer.service")?;
    println!("Wrote {}", service_path.display());

    std::fs::write(&timer_path, timer_content).context("Failed to write tokenizer.timer")?;
    println!("Wrote {}", timer_path.display());

    let status = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("Failed to run systemctl daemon-reload")?;
    if !status.success() {
        anyhow::bail!("systemctl daemon-reload failed");
    }

    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "tokenizer.timer"])
        .status()
        .context("Failed to enable tokenizer.timer")?;
    if !status.success() {
        anyhow::bail!("systemctl enable --now tokenizer.timer failed");
    }

    println!("Timer installed. Check: systemctl --user status tokenizer.timer");
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_timer_macos() -> Result<()> {
    let dir = home_dir().join("Library/LaunchAgents");
    std::fs::create_dir_all(&dir).context("Failed to create LaunchAgents dir")?;

    let exe = optimizer_exe();
    let label = "com.tokenizer";
    let plist_path = dir.join(format!("{label}.plist"));

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>optimize</string>
    <string>--quiet</string>
  </array>
  <key>StartInterval</key><integer>3600</integer>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        exe.display()
    );

    std::fs::write(&plist_path, plist).context("Failed to write launchd plist")?;
    println!("Wrote {}", plist_path.display());

    // Unload first (ignore error) then load. Bootstrap under GUI/<uid> would be more
    // modern but `launchctl load` still works for per-user agents.
    let _ = std::process::Command::new("launchctl")
        .args(["unload", plist_path.to_str().unwrap_or_default()])
        .status();

    let status = std::process::Command::new("launchctl")
        .args(["load", plist_path.to_str().unwrap_or_default()])
        .status()
        .context("Failed to run launchctl load")?;
    if !status.success() {
        anyhow::bail!("launchctl load failed");
    }

    println!("Timer installed. Check: launchctl list | grep {label}");
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_timer_windows() -> Result<()> {
    let exe = optimizer_exe();
    let task_name = "Tokenizer";

    // Delete prior instance (ignore error) so reinstall is idempotent.
    let _ = std::process::Command::new("schtasks")
        .args(["/delete", "/tn", task_name, "/f"])
        .status();

    let status = std::process::Command::new("schtasks")
        .args([
            "/create",
            "/tn",
            task_name,
            "/tr",
            &format!("\"{}\" optimize --quiet", exe.display()),
            "/sc",
            "hourly",
            "/f",
        ])
        .status()
        .context("Failed to run schtasks /create")?;
    if !status.success() {
        anyhow::bail!("schtasks /create failed");
    }

    // Write sentinel so the TUI can show "installed"
    let marker = timer_marker_path();
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&marker, task_name);

    println!("Scheduled task '{task_name}' installed (runs hourly).");
    println!("Check: schtasks /query /tn {task_name}");
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// install_hook — writes a post-session script in ~/.claude/hooks
// ──────────────────────────────────────────────────────────────────────────────

pub fn install_hook() -> Result<()> {
    let hooks_dir = home_dir().join(".claude/hooks");
    std::fs::create_dir_all(&hooks_dir).context("Failed to create hooks dir")?;

    let exe = optimizer_exe();
    let hook_path = hook_path();

    #[cfg(windows)]
    let content = format!(
        "# Claude Optimizer post-session hook (Windows)\r\n\
         $input = [Console]::In.ReadToEnd()\r\n\
         Start-Process -FilePath \"{}\" -ArgumentList 'optimize','--quiet' -WindowStyle Hidden\r\n\
         Write-Output $input\r\n",
        exe.display()
    );

    #[cfg(not(windows))]
    let content = format!(
        "#!/bin/bash\n\
         # Claude Optimizer post-session hook\n\
         input=$(cat)\n\
         \"{}\" optimize --quiet 2>/dev/null &\n\
         echo \"$input\"\n",
        exe.display()
    );

    std::fs::write(&hook_path, content).context("Failed to write hook script")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&hook_path, perms).context("Failed to set hook permissions")?;
    }

    println!("Wrote {}", hook_path.display());
    println!("Hook installed. Runs after each Claude Code session.");
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Rollback (platform-agnostic)
// ──────────────────────────────────────────────────────────────────────────────

pub fn rollback(manifest_id: &str) -> Result<()> {
    let entries = manifest::read_manifest().context("Failed to read manifest")?;

    let entry = entries
        .iter()
        .find(|e| e.id == manifest_id)
        .context(format!("Manifest entry '{manifest_id}' not found"))?;

    let backup_path = PathBuf::from(&entry.backup_path);
    let original_path = PathBuf::from(&entry.original_path);
    let converted_path = PathBuf::from(&entry.converted_path);

    if !backup_path.exists() {
        anyhow::bail!(
            "Backup file not found: {}. Cannot rollback.",
            backup_path.display()
        );
    }

    std::fs::copy(&backup_path, &original_path).context(format!(
        "Failed to restore {} from backup",
        original_path.display()
    ))?;
    println!(
        "Restored {} from backup {}",
        original_path.display(),
        backup_path.display()
    );

    if converted_path != original_path && converted_path.exists() {
        std::fs::remove_file(&converted_path).context(format!(
            "Failed to remove converted file {}",
            converted_path.display()
        ))?;
        println!("Removed converted file {}", converted_path.display());
    }

    println!("Rollback of '{manifest_id}' complete.");
    Ok(())
}
