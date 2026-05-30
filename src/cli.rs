//! Command-line surface and resolution of flags + ambient state into `Config`.

use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::config::Config;
use crate::model::Level;

#[derive(Parser, Debug)]
#[command(
    name = "zlc",
    version,
    about = "Readable, navigable Android logcat for the Zed editor"
)]
pub struct Args {
    /// Target device serial (adb -s). When piping a stream in, this is ignored.
    #[arg(short = 's', long = "serial", visible_alias = "device", value_name = "SERIAL")]
    pub serial: Option<String>,

    /// Clear the logcat buffer before tailing (adb logcat -c).
    #[arg(short = 'c', long = "clear")]
    pub clear: bool,

    /// Crash view (reserved; wired in a later phase).
    #[arg(long = "crash")]
    pub crash: bool,

    /// Base directory for resolving clickable frame paths.
    /// Default: $ZED_WORKTREE_ROOT, else the nearest Gradle root, else cwd.
    #[arg(long = "root", value_name = "DIR")]
    pub root: Option<PathBuf>,

    /// When to colorize output.
    #[arg(long = "color", value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    /// Only show logs from this app package (repeatable). Enables app-only
    /// filtering and PID following; `pkg:process` sub-processes are included.
    #[arg(short = 'p', long = "package", value_name = "PKG")]
    pub package: Vec<String>,

    /// Include only tags matching this glob (repeatable). Crashes bypass this.
    #[arg(short = 't', long = "tag", value_name = "GLOB")]
    pub tag: Vec<String>,

    /// Exclude tags matching this glob (repeatable). Crashes bypass this.
    #[arg(short = 'T', long = "exclude-tag", value_name = "GLOB")]
    pub exclude_tag: Vec<String>,

    /// Minimum level to show: V, D, I, W, E, or F (default: V).
    #[arg(short = 'm', long = "min-level", value_name = "LEVEL")]
    pub min_level: Option<String>,

    /// Do not collapse consecutive identical lines into a ×N counter.
    #[arg(long = "no-dedupe")]
    pub no_dedupe: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// On when stdout is a terminal and NO_COLOR is unset.
    Auto,
    Always,
    Never,
}

impl Args {
    /// Resolve flags + environment into a fully-determined `Config`. Reads ambient
    /// state (tty-ness, width, env) here and nowhere else.
    pub fn resolve(self) -> anyhow::Result<Config> {
        let read_stdin = !std::io::stdin().is_terminal();

        let color = match self.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
            }
        };

        let width = terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(80);

        let min_level = self
            .min_level
            .as_deref()
            .and_then(|s| s.chars().next())
            .map(|c| c.to_ascii_uppercase())
            .and_then(Level::from_char)
            .unwrap_or(Level::Verbose);

        Ok(Config {
            serial: self.serial,
            clear: self.clear,
            crash: self.crash,
            root: resolve_root(self.root),
            color,
            width,
            read_stdin,
            packages: self.package,
            tag_includes: self.tag,
            tag_excludes: self.exclude_tag,
            min_level,
            dedupe: !self.no_dedupe,
            seed_pids: HashSet::new(),
        })
    }
}

/// `--root` precedence: explicit flag > $ZED_WORKTREE_ROOT > nearest Gradle root > cwd.
/// Zed resolves relative terminal links against the worktree root, so that is the
/// base we emit paths relative to whenever it is available.
fn resolve_root(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    if let Some(wt) = std::env::var_os("ZED_WORKTREE_ROOT") {
        return PathBuf::from(wt);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.as_path();
    loop {
        if dir.join("settings.gradle").exists() || dir.join("settings.gradle.kts").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return cwd.clone(),
        }
    }
}
