//! Resolved configuration.
//!
//! All ambient decisions (is-a-tty, terminal width, whether color is on, which
//! input source to use) are resolved exactly once at startup into this struct, so
//! every downstream stage is a pure function of `Config` and is trivially testable.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::model::Level;

#[derive(Debug, Clone)]
pub struct Config {
    /// Target device serial (`adb -s <serial>`), when zlc spawns adb itself.
    pub serial: Option<String>,
    /// Clear the logcat buffer before tailing (`adb logcat -c`).
    pub clear: bool,
    /// Crash view (reserved; wired in a later phase).
    pub crash: bool,
    /// Base directory clickable frame paths are emitted relative to.
    pub root: PathBuf,
    /// Whether ANSI color is enabled for this run.
    pub color: bool,
    /// Terminal width in columns (falls back to 80 when not a tty).
    pub width: usize,
    /// Read from stdin (a pipe was detected) rather than spawning `adb logcat`.
    pub read_stdin: bool,
    /// App packages to follow; non-empty enables app-only filtering + pidtrack.
    pub packages: Vec<String>,
    /// Tag include globs (`-t`); empty means "all tags".
    pub tag_includes: Vec<String>,
    /// Tag exclude globs (`-T`).
    pub tag_excludes: Vec<String>,
    /// Minimum level to show.
    pub min_level: Level,
    /// Collapse consecutive identical lines into a `×N` counter (default on).
    pub dedupe: bool,
    /// PIDs seeded at startup via `adb shell pidof` (empty on the stdin path).
    pub seed_pids: HashSet<u32>,
}
