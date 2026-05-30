//! Follow an app's process set across (re)launches by reading `ActivityManager`
//! lifecycle lines — the established pidcat technique.
//!
//! `Start proc 12345:com.example.app/u0a…` adds a pid; `Killing 12345:…` and
//! `Process … (pid 12345) has died` remove one. The followed value is a *set*, not a
//! single pid, so multi-process apps (`com.example.app:remote`, sandboxed WebView,
//! etc.) are tracked too. `adb shell pidof` seeds the already-running case at startup.
//!
//! The tracker observes the FULL pre-filter record stream (ActivityManager lines come
//! from system_server, a different pid/tag than the app), maintaining the live set as a
//! side effect; `filter` then queries it.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::model::LogRecord;

static START_PROC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Start proc (\d+):(\S+?)/").unwrap());
static KILLING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Killing (\d+):(\S+?)/").unwrap());
static DIED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Process (\S+) \(pid (\d+)\) has died").unwrap());

pub struct Tracker {
    packages: Vec<String>,
    pids: HashSet<u32>,
    enabled: bool,
}

impl Tracker {
    /// `seed` is the startup `pidof` result (empty for the stdin path). The tracker is
    /// only active when at least one package is being followed.
    pub fn new(packages: Vec<String>, seed: HashSet<u32>) -> Self {
        let enabled = !packages.is_empty();
        Self {
            packages,
            pids: seed,
            enabled,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn pids(&self) -> &HashSet<u32> {
        &self.pids
    }

    /// Update the followed set from one record's lifecycle content (no-op when no
    /// package is followed).
    pub fn observe(&mut self, r: &LogRecord) {
        if !self.enabled {
            return;
        }
        if let Some(c) = START_PROC.captures(&r.msg) {
            if self.matches(&c[2]) {
                if let Ok(pid) = c[1].parse() {
                    self.pids.insert(pid);
                }
            }
        }
        if let Some(c) = KILLING.captures(&r.msg) {
            if self.matches(&c[2]) {
                if let Ok(pid) = c[1].parse::<u32>() {
                    self.pids.remove(&pid);
                }
            }
        }
        if let Some(c) = DIED.captures(&r.msg) {
            if self.matches(&c[1]) {
                if let Ok(pid) = c[2].parse::<u32>() {
                    self.pids.remove(&pid);
                }
            }
        }
    }

    /// A process name belongs to us if it is the package itself or one of its
    /// `pkg:process` sub-processes.
    fn matches(&self, proc_name: &str) -> bool {
        self.packages
            .iter()
            .any(|p| proc_name == p || proc_name.starts_with(&format!("{p}:")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_line, ParsedLine};

    fn feed(tracker: &mut Tracker, line: &str) {
        if let ParsedLine::Record(r) = parse_line(line) {
            tracker.observe(&r);
        }
    }

    fn tracker() -> Tracker {
        Tracker::new(vec!["com.example.app".to_string()], HashSet::new())
    }

    #[test]
    fn follows_start_then_death_then_relaunch() {
        let mut t = tracker();
        feed(&mut t, "10-01 10:00:00.000  1000  1000 I ActivityManager: Start proc 4120:com.example.app/u0a123 for top-activity {com.example.app/.MainActivity}");
        assert_eq!(t.pids().iter().copied().collect::<Vec<_>>(), vec![4120]);

        feed(&mut t, "10-01 10:00:05.000  1000  1000 I ActivityManager: Killing 4120:com.example.app/u0a123 (adj 905): stop com.example.app");
        assert!(t.pids().is_empty());

        feed(&mut t, "10-01 10:00:09.000  1000  1000 I ActivityManager: Start proc 4200:com.example.app/u0a123 for next-top-activity {com.example.app/.MainActivity}");
        assert_eq!(t.pids().iter().copied().collect::<Vec<_>>(), vec![4200]);
    }

    #[test]
    fn tracks_multiprocess_sub_processes() {
        let mut t = tracker();
        feed(&mut t, "10-01 10:00:00.000  1000  1000 I ActivityManager: Start proc 4120:com.example.app/u0a123 for top-activity {x}");
        feed(&mut t, "10-01 10:00:01.000  1000  1000 I ActivityManager: Start proc 4200:com.example.app:remote/u0a123 for service {x}");
        let mut pids: Vec<u32> = t.pids().iter().copied().collect();
        pids.sort_unstable();
        assert_eq!(pids, vec![4120, 4200]);
    }

    #[test]
    fn process_died_line_removes_pid() {
        let mut t = tracker();
        feed(&mut t, "10-01 10:00:00.000  1000  1000 I ActivityManager: Start proc 4200:com.example.app:remote/u0a123 for service {x}");
        feed(&mut t, "10-01 10:00:08.000   600   600 I ActivityManager: Process com.example.app:remote (pid 4200) has died: cch  CRE");
        assert!(t.pids().is_empty());
    }

    #[test]
    fn ignores_other_apps() {
        let mut t = tracker();
        feed(&mut t, "10-01 10:00:00.000  1000  1000 I ActivityManager: Start proc 9999:com.other.app/u0a99 for top-activity {y}");
        assert!(t.pids().is_empty());
    }

    #[test]
    fn disabled_tracker_ignores_everything() {
        let mut t = Tracker::new(vec![], HashSet::new());
        feed(&mut t, "10-01 10:00:00.000  1000  1000 I ActivityManager: Start proc 4120:com.example.app/u0a123 for top-activity {x}");
        assert!(!t.enabled());
        assert!(t.pids().is_empty());
    }
}
