//! Resolved configuration.
//!
//! All ambient decisions (is-a-tty, terminal width, whether color is on, which
//! input source to use) are resolved exactly once at startup into this struct, so
//! every downstream stage is a pure function of `Config` and is trivially testable.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

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
    /// Tag column width.
    pub tag_width: usize,
    /// Wrap long messages with a hanging indent.
    pub wrap: bool,
    /// Explicit source roots (relative to `root`); empty means auto-discover.
    pub source_roots: Vec<String>,
    /// PIDs seeded at startup via `adb shell pidof` (empty on the stdin path).
    pub seed_pids: HashSet<u32>,
}

/// `zlc.toml` schema — a committable, per-project style. All fields optional; CLI
/// flags override. Lives at the project root (the `--root` directory).
#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub app_packages: Vec<String>,
    pub source_roots: Vec<String>,
    pub min_level: Option<String>,
    pub dedupe: Option<bool>,
    pub tag_width: Option<usize>,
    pub wrap: Option<bool>,
    /// Persistent, always-muted tags (merged into the `-T` exclude set).
    pub mute_tags: Vec<String>,
}

impl FileConfig {
    /// Load `<root>/zlc.toml`. A missing file yields defaults; a malformed one errors.
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("zlc.toml");
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
            }
            Err(_) => Ok(Self::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_config() {
        let toml = r#"
            app_packages = ["com.example.app"]
            source_roots = ["app/src/main/kotlin"]
            min_level = "W"
            dedupe = false
            tag_width = 23
            wrap = false
            mute_tags = ["chatty", "OkHttp"]
        "#;
        let c: FileConfig = toml::from_str(toml).unwrap();
        assert_eq!(c.app_packages, ["com.example.app"]);
        assert_eq!(c.source_roots, ["app/src/main/kotlin"]);
        assert_eq!(c.min_level.as_deref(), Some("W"));
        assert_eq!(c.dedupe, Some(false));
        assert_eq!(c.tag_width, Some(23));
        assert_eq!(c.wrap, Some(false));
        assert_eq!(c.mute_tags, ["chatty", "OkHttp"]);
    }

    #[test]
    fn empty_config_is_all_defaults() {
        let c: FileConfig = toml::from_str("").unwrap();
        assert!(c.app_packages.is_empty() && c.mute_tags.is_empty());
        assert!(c.min_level.is_none() && c.dedupe.is_none());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<FileConfig>("bogus_key = 1").is_err());
    }
}
