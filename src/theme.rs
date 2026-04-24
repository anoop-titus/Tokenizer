#![allow(dead_code)]
use ratatui::style::Color;

// ── Base canvas ───────────────────────────────────────────────────────────────
pub const BG: Color = Color::Rgb(12, 10, 8); // warm near-black
pub const ROW_ALT: Color = Color::Rgb(18, 15, 11);

// ── Surface tiers (for filled buttons / cards at rest vs hover) ───────────────
pub const SURFACE: Color = Color::Rgb(36, 26, 12); // warm dim amber fill — rest state
pub const SURFACE_HOT: Color = Color::Rgb(72, 52, 18); // hover / secondary-focus fill
pub const HIGHLIGHT_BG: Color = Color::Rgb(40, 30, 55); // row/field highlight (cool)

// ── Amber scale (CRT phosphor) ────────────────────────────────────────────────
pub const AMBER: Color = Color::Rgb(255, 180, 60);
pub const AMBER_BRIGHT: Color = Color::Rgb(255, 212, 120);
pub const AMBER_DIM: Color = Color::Rgb(153, 102, 0);

// ── Accents ───────────────────────────────────────────────────────────────────
pub const CYAN: Color = Color::Rgb(80, 240, 210); // primary action / focused
pub const GREEN: Color = Color::Rgb(80, 230, 140); // success
pub const MAGENTA: Color = Color::Rgb(255, 80, 160);
pub const DANGER: Color = Color::Rgb(240, 90, 90);

// ── Borders ───────────────────────────────────────────────────────────────────
pub const BORDER: Color = Color::Rgb(60, 48, 32); // dim warm
pub const BORDER_BRIGHT: Color = Color::Rgb(120, 92, 48); // hover border
