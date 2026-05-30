//! Collapse consecutive *identical* records into a single line with a `×N` counter.
//!
//! Exact back-to-back only (same level/tag/message) — the safe, surprise-free form.
//! Like trace assembly, a run has no end until a different line arrives, so the
//! pending record must be flushable on idle/EOF ([`Deduper::flush`]).
//!
//! Output is a stream of `(Emit, count)` pairs; the renderer draws the `×count` suffix
//! when `count > 1`. Non-record emits (traces, dividers, raw) pass straight through and
//! flush any pending run first.

use crate::model::LogRecord;
use crate::trace::Emit;

pub struct Deduper {
    enabled: bool,
    pending: Option<(LogRecord, u32)>,
}

impl Deduper {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            pending: None,
        }
    }

    pub fn push(&mut self, emit: Emit) -> Vec<(Emit, u32)> {
        if !self.enabled {
            return vec![(emit, 1)];
        }
        match emit {
            Emit::Record(r) => self.push_record(r),
            other => {
                let mut out = self.flush();
                out.push((other, 1));
                out
            }
        }
    }

    /// Release any pending run (call on idle-timeout / child-exit / end-of-stream).
    pub fn flush(&mut self) -> Vec<(Emit, u32)> {
        match self.pending.take() {
            Some((r, n)) => vec![(Emit::Record(r), n)],
            None => Vec::new(),
        }
    }

    fn push_record(&mut self, r: LogRecord) -> Vec<(Emit, u32)> {
        if let Some((pending, n)) = self.pending.as_mut() {
            if same(pending, &r) {
                *n += 1;
                return Vec::new();
            }
            let (prev, count) = self.pending.take().expect("checked above");
            self.pending = Some((r, 1));
            return vec![(Emit::Record(prev), count)];
        }
        self.pending = Some((r, 1));
        Vec::new()
    }
}

fn same(a: &LogRecord, b: &LogRecord) -> bool {
    a.level == b.level && a.tag == b.tag && a.msg == b.msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Level;

    fn rec(msg: &str) -> Emit {
        Emit::Record(LogRecord {
            ts: "t".to_string(),
            pid: 1,
            tid: 1,
            uid: None,
            level: Level::Info,
            tag: "T".to_string(),
            msg: msg.to_string(),
        })
    }

    fn run(emits: Vec<Emit>) -> Vec<(String, u32)> {
        let mut d = Deduper::new(true);
        let mut out = Vec::new();
        for e in emits {
            out.extend(d.push(e));
        }
        out.extend(d.flush());
        out.into_iter()
            .map(|(e, n)| match e {
                Emit::Record(r) => (r.msg, n),
                Emit::Raw(s) => (s, n),
                _ => ("?".to_string(), n),
            })
            .collect()
    }

    #[test]
    fn collapses_consecutive_identicals_with_count() {
        let out = run(vec![rec("tick"), rec("tick"), rec("tick"), rec("tock")]);
        assert_eq!(out, vec![("tick".to_string(), 3), ("tock".to_string(), 1)]);
    }

    #[test]
    fn non_record_flushes_pending_run() {
        let out = run(vec![rec("tick"), rec("tick"), Emit::Raw("---".to_string())]);
        assert_eq!(out, vec![("tick".to_string(), 2), ("---".to_string(), 1)]);
    }

    #[test]
    fn disabled_passes_everything_through() {
        let mut d = Deduper::new(false);
        let out: Vec<_> = [rec("x"), rec("x")]
            .into_iter()
            .flat_map(|e| d.push(e))
            .collect();
        assert_eq!(out.len(), 2);
    }
}
