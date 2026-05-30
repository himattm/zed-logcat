//! Resolved configuration.
//!
//! All ambient decisions (is-a-tty, terminal width, whether color is on, which
//! input source to use) are resolved exactly once at startup into this struct, so
//! every downstream stage is a pure function of `Config` and is trivially testable.

use std::path::PathBuf;

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
}
