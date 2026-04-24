# Tokenizer

Token-optimization TUI for Claude Code's `~/.claude` directory.

Cross-platform Rust application (Linux / macOS / Windows) that scans your Claude
Code install, converts verbose `.md`/`.json` files into compact `.toon` format,
and reorganizes agent/skill files into tidy domain groups — all behind a
6-tab terminal UI with an amber-CRT aesthetic.

## Features

- **Scan & classify** every file under `~/.claude` (agents, rules, skills, commands, memory).
- **4-level compression** (level 0 = lossless reformat, level 3 = aggressive summary).
- **One-way pipeline**: `.md` → `.json/.jsonl` → `.toon`. Originals backed up; rollback via manifest ID.
- **Restructure**: groups loose files into domain subdirectories with idempotency guards.
- **Daemon mode**: hourly background run + post-session Claude Code hook.
- **Safe by default**: whitelist (`settings.json`, `*.sh`, `*.js`, `SKILL.md`, `CLAUDE.md`, `MEMORY.md`), file locking, backups, SQLite registry of already-optimized files.

## Install

```bash
cargo install --path .
# or, after `cargo build --release`:
cp target/release/tokenizer ~/.local/bin/
```

## Usage

```bash
tokenizer                       # launch TUI
tokenizer optimize [--dry-run]  # headless run
tokenizer rollback <manifest-id>
tokenizer install-timer         # hourly background optimization
tokenizer install-hook          # run after each Claude Code session
```

Platform support for `install-timer`:
- **Linux** — systemd user timer (`~/.config/systemd/user/tokenizer.timer`)
- **macOS** — launchd LaunchAgent (`~/Library/LaunchAgents/com.tokenizer.plist`)
- **Windows** — Task Scheduler (`schtasks` hourly task, PowerShell hook)

## Data

- Config: `~/.config/tokenizer/config.toml`
- History: `~/.config/tokenizer/history.db` (SQLite)
- Backups: `~/.config/tokenizer/backups/`
- Manifest: `~/.config/tokenizer/manifest.jsonl`

## Build

```bash
cargo build --release
```

Release binary is ~5 MB stripped with LTO. No runtime deps beyond libc (Unix) or
the Windows system APIs.

## License

MIT
