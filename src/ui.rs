//! Terminal UI helpers — Astro‑inspired output style.
//!
//! This module provides colour functions and macros to produce a consistent,
//! visually appealing command‑line interface. The visual language uses symbols:
//!
//! * `◆` – section header / major action
//! * `●` – step in progress (not used by default)
//! * `✔` – success
//! * `✘` – error
//! * `│` – continuation / detail line
//! * `➜` – prompt / next step hint

use colored::*;

// ─── Colours ─────────────────────────────────────────────────────────────────

/// Dim (gray) text – used for details and hints.
pub fn dim(s: &str) -> ColoredString {
    s.truecolor(110, 110, 120)
}

/// Accent colour (purple) – used for symbols and headers.
pub fn accent(s: &str) -> ColoredString {
    s.truecolor(135, 100, 255).bold()
}

/// Green text for success messages.
pub fn ok(s: &str) -> ColoredString {
    s.truecolor(80, 220, 140).bold()
}

/// Red text for error messages.
pub fn err(s: &str) -> ColoredString {
    s.truecolor(255, 80, 80).bold()
}

/// Highlight colour (yellow/orange) for emphasis.
pub fn hi(s: &str) -> ColoredString {
    s.truecolor(255, 200, 80)
}

// ─── Symbols ─────────────────────────────────────────────────────────────────

/// Diamond symbol `◆` used for section headers.
pub const DIAMOND: &str = "◆";
/// Check mark `✔` used for success.
pub const CHECK: &str = "✔";
/// Cross `✘` used for errors.
pub const CROSS: &str = "✘";
/// Vertical bar `│` used for continuation lines.
pub const BAR: &str = "│";
/// Arrow `➜` used for hints / next steps.
pub const ARROW: &str = "➜";

// ─── Macros ──────────────────────────────────────────────────────────────────

/// Print a section header: `◆  <bold title>`.
///
/// # Example
/// ```
/// section!("compiling script.rs");
/// ```
#[macro_export]
macro_rules! section {
    ($($arg:tt)*) => {
        eprintln!("{} {}", $crate::ui::accent($crate::ui::DIAMOND), format!($($arg)*).bold());
    };
}

/// Print a detail line: `│  <dim text>`.
///
/// # Example
/// ```
/// detail!("cache hit · {}", hash);
/// ```
#[macro_export]
macro_rules! detail {
    ($($arg:tt)*) => {
        eprintln!("{}  {}", $crate::ui::dim($crate::ui::BAR), $crate::ui::dim(&format!($($arg)*)));
    };
}

/// Print a step line: `●  <text>` (currently not used in default output).
#[macro_export]
macro_rules! step {
    ($($arg:tt)*) => {
        eprintln!("{}  {}", $crate::ui::accent($crate::ui::DOT), format!($($arg)*));
    };
}

/// Print a success line: `✔  <green text>`.
///
/// # Example
/// ```
/// success!("compiled in {:.2}s", elapsed);
/// ```
#[macro_export]
macro_rules! success {
    ($($arg:tt)*) => {
        eprintln!("{}  {}", $crate::ui::ok($crate::ui::CHECK), format!($($arg)*));
    };
}

/// Print an error line: `✘  <red text>`.
#[macro_export]
macro_rules! fail {
    ($($arg:tt)*) => {
        eprintln!("{}  {}", $crate::ui::err($crate::ui::CROSS), format!($($arg)*));
    };
}

/// Print a hint line: `➜  <dim text>`.
#[macro_export]
macro_rules! hint {
    ($($arg:tt)*) => {
        eprintln!("{}  {}", $crate::ui::dim($crate::ui::ARROW), $crate::ui::dim(&format!($($arg)*)));
    };
}

/// Print an empty line (gap) for visual separation.
#[macro_export]
macro_rules! gap {
    () => {
        eprintln!();
    };
}