//! Decide which emitted records/traces to show.
//!
//! Composition: app-only (PID in the followed set) AND tag include/exclude globs AND
//! `min_level`. Crash bypass: a detected `FATAL EXCEPTION` block always shows, so
//! focusing on your own tag (or app) can never swallow a fatal crash.

use std::collections::HashSet;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::model::{Level, LogRecord};
use crate::trace::Emit;

pub struct Filter {
    app_only: bool,
    includes: Option<GlobSet>,
    excludes: Option<GlobSet>,
    min_level: Level,
}

impl Filter {
    pub fn new(
        app_only: bool,
        includes: &[String],
        excludes: &[String],
        min_level: Level,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            app_only,
            includes: build_set(includes)?,
            excludes: build_set(excludes)?,
            min_level,
        })
    }

    /// Whether to render this emit. Fatal traces bypass every filter.
    pub fn keep(&self, emit: &Emit, pids: &HashSet<u32>) -> bool {
        match emit {
            Emit::Record(r) => self.keep_record(r, pids),
            Emit::Trace(t) => t.is_fatal || self.keep_record(&t.records[0], pids),
            // Structural / unparseable lines are always passed through.
            Emit::Divider(_) | Emit::Raw(_) => true,
        }
    }

    fn keep_record(&self, r: &LogRecord, pids: &HashSet<u32>) -> bool {
        if self.app_only && !pids.contains(&r.pid) {
            return false;
        }
        if r.level.rank() < self.min_level.rank() {
            return false;
        }
        self.tag_ok(&r.tag)
    }

    fn tag_ok(&self, tag: &str) -> bool {
        if let Some(ex) = &self.excludes {
            if ex.is_match(tag) {
                return false;
            }
        }
        if let Some(inc) = &self.includes {
            return inc.is_match(tag);
        }
        true
    }
}

/// Build a glob set where `*` spans the whole tag (tags contain `/`, e.g.
/// `MyApp/Network`, and we do not want `/` to be a glob boundary).
fn build_set(patterns: &[String]) -> anyhow::Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(GlobBuilder::new(p).literal_separator(false).build()?);
    }
    Ok(Some(builder.build()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(level: Level, tag: &str, pid: u32) -> Emit {
        Emit::Record(LogRecord {
            ts: "05-30 12:00:00.000".to_string(),
            pid,
            tid: pid,
            uid: None,
            level,
            tag: tag.to_string(),
            msg: "m".to_string(),
        })
    }

    fn fatal_trace(tag: &str, pid: u32) -> Emit {
        Emit::Trace(crate::trace::Trace {
            is_fatal: true,
            kind: crate::trace::TraceKind::Java,
            records: vec![LogRecord {
                ts: "05-30 12:00:00.000".to_string(),
                pid,
                tid: pid,
                uid: None,
                level: Level::Error,
                tag: tag.to_string(),
                msg: "FATAL EXCEPTION: main".to_string(),
            }],
        })
    }

    fn pids(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn app_only_keeps_tracked_pids() {
        let f = Filter::new(true, &[], &[], Level::Verbose).unwrap();
        assert!(f.keep(&rec(Level::Info, "X", 4120), &pids(&[4120])));
        assert!(!f.keep(&rec(Level::Info, "X", 9999), &pids(&[4120])));
    }

    #[test]
    fn min_level_drops_below() {
        let f = Filter::new(false, &[], &[], Level::Warn).unwrap();
        assert!(!f.keep(&rec(Level::Info, "X", 1), &pids(&[])));
        assert!(f.keep(&rec(Level::Warn, "X", 1), &pids(&[])));
        assert!(f.keep(&rec(Level::Error, "X", 1), &pids(&[])));
    }

    #[test]
    fn tag_include_and_exclude_globs() {
        let inc = Filter::new(false, &["OkHttp*".to_string()], &[], Level::Verbose).unwrap();
        assert!(inc.keep(&rec(Level::Info, "OkHttpClient", 1), &pids(&[])));
        assert!(!inc.keep(&rec(Level::Info, "MyApp", 1), &pids(&[])));

        // '*' spans '/', so a tag with a slash matches.
        let slash = Filter::new(false, &["MyApp*".to_string()], &[], Level::Verbose).unwrap();
        assert!(slash.keep(&rec(Level::Info, "MyApp/Network", 1), &pids(&[])));

        let exc = Filter::new(false, &[], &["chatty".to_string()], Level::Verbose).unwrap();
        assert!(!exc.keep(&rec(Level::Info, "chatty", 1), &pids(&[])));
        assert!(exc.keep(&rec(Level::Info, "MyApp", 1), &pids(&[])));
    }

    #[test]
    fn fatal_trace_bypasses_all_filters() {
        // App-only (untracked pid), excluded tag, and a high min-level — all bypassed.
        let f = Filter::new(true, &[], &["AndroidRuntime".to_string()], Level::Fatal).unwrap();
        assert!(f.keep(&fatal_trace("AndroidRuntime", 9999), &pids(&[4120])));
    }

    #[test]
    fn non_fatal_trace_obeys_filters() {
        let f = Filter::new(false, &[], &["System.err".to_string()], Level::Verbose).unwrap();
        let t = Emit::Trace(crate::trace::Trace {
            is_fatal: false,
            kind: crate::trace::TraceKind::Java,
            records: vec![LogRecord {
                ts: "05-30 12:00:00.000".to_string(),
                pid: 1,
                tid: 1,
                uid: None,
                level: Level::Warn,
                tag: "System.err".to_string(),
                msg: "java.lang.RuntimeException: x".to_string(),
            }],
        });
        assert!(!f.keep(&t, &pids(&[])));
    }
}
