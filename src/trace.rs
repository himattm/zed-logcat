//! Assemble multi-line stack traces from a stream of parsed lines.
//!
//! In `-v threadtime`, a Java/Kotlin exception is NOT one multi-line record — each
//! continuation line (`\tat …`, `Caused by:`, `… N more`, `Suppressed:`) is its own
//! logcat record carrying the same pid/tid. So "assembly" means grouping a run of
//! consecutive records from the same thread whose messages match the trace grammar.
//!
//! Crucially there is no EOF in a live tail and the crash is usually the LAST thing a
//! process emits before dying — so a pending trace must be flushable on demand
//! ([`Assembler::flush`], driven by an idle-timeout / child-exit in the live path),
//! not only when the next non-matching line happens to arrive.

use std::sync::LazyLock;

use regex::Regex;

use crate::model::{Level, LogRecord};
use crate::parse::ParsedLine;

/// Whether an assembled block is a managed (ART) stack trace or a native crash
/// backtrace (`#NN pc …` from debuggerd). Native frames have no source `file:line`
/// and are never made clickable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceKind {
    Java,
    Native,
}

/// An assembled stack-trace block (header records + frames), kept in arrival order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    pub records: Vec<LogRecord>,
    /// True when the block contains a `FATAL EXCEPTION` / `Process: …, PID:` header,
    /// or is a native crash. This is the precise signal the crash bypass uses (not a
    /// blanket "level E/F").
    pub is_fatal: bool,
    pub kind: TraceKind,
}

impl Trace {
    pub fn key(&self) -> (u32, u32) {
        let head = &self.records[0];
        (head.pid, head.tid)
    }
}

/// What the assembler emits downstream (renderer / json consume these).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Emit {
    Record(LogRecord),
    Trace(Trace),
    Divider(String),
    Raw(String),
}

#[derive(Default)]
pub struct Assembler {
    pending: Option<Trace>,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one parsed line; returns whatever became complete as a result.
    pub fn push(&mut self, line: ParsedLine) -> Vec<Emit> {
        match line {
            ParsedLine::Record(rec) => self.push_record(rec),
            ParsedLine::Divider(buf) => self.flush_then(Emit::Divider(buf)),
            ParsedLine::Raw(s) => self.flush_then(Emit::Raw(s)),
        }
    }

    /// Emit any pending trace. Call on idle-timeout, child-exit, or end-of-stream.
    pub fn flush(&mut self) -> Vec<Emit> {
        match self.pending.take() {
            Some(trace) => vec![Emit::Trace(trace)],
            None => Vec::new(),
        }
    }

    fn flush_then(&mut self, tail: Emit) -> Vec<Emit> {
        let mut out = self.flush();
        out.push(tail);
        out
    }

    fn push_record(&mut self, rec: LogRecord) -> Vec<Emit> {
        if let Some(pending) = self.pending.as_ref() {
            let (pid, tid) = pending.key();
            let same_thread = rec.pid == pid && rec.tid == tid;
            let continues = same_thread
                && match pending.kind {
                    TraceKind::Java => {
                        is_continuation(&rec.msg)
                            || is_exception_header(&rec.msg)
                            || is_fatal_header(&rec.msg)
                    }
                    TraceKind::Native => is_native_frame(&rec.msg),
                };
            if continues {
                let pending = self.pending.as_mut().expect("checked above");
                if is_fatal_header(&rec.msg) {
                    pending.is_fatal = true;
                }
                pending.records.push(rec);
                return Vec::new();
            }
            // The trace ended; emit it and reconsider this record fresh.
            let done = self.pending.take().expect("checked above");
            let mut out = vec![Emit::Trace(done)];
            out.extend(self.start_or_emit(rec));
            return out;
        }
        self.start_or_emit(rec)
    }

    fn start_or_emit(&mut self, rec: LogRecord) -> Vec<Emit> {
        // A Java trace begins at a FATAL header, or at an exception header logged at a
        // severity where a real stack trace is plausible (E/F) — gating on level avoids
        // treating an Info log that merely mentions "NullPointerException" as a trace.
        // A native crash backtrace begins at its first `#NN pc` frame.
        if is_fatal_header(&rec.msg)
            || (is_exception_header(&rec.msg) && matches!(rec.level, Level::Error | Level::Fatal))
        {
            self.pending = Some(Trace {
                is_fatal: is_fatal_header(&rec.msg),
                kind: TraceKind::Java,
                records: vec![rec],
            });
            Vec::new()
        } else if is_native_frame(&rec.msg) {
            self.pending = Some(Trace {
                is_fatal: true, // a native crash always shows
                kind: TraceKind::Native,
                records: vec![rec],
            });
            Vec::new()
        } else {
            vec![Emit::Record(rec)]
        }
    }
}

fn is_frame(msg: &str) -> bool {
    msg.trim_start().starts_with("at ")
}

fn is_cause(msg: &str) -> bool {
    msg.trim_start().starts_with("Caused by:")
}

fn is_suppressed(msg: &str) -> bool {
    msg.trim_start().starts_with("Suppressed:")
}

fn is_more(msg: &str) -> bool {
    let t = msg.trim_start();
    t.starts_with("...") && t.ends_with("more")
}

fn is_continuation(msg: &str) -> bool {
    is_frame(msg) || is_cause(msg) || is_suppressed(msg) || is_more(msg)
}

/// A fully-qualified type ending in Exception/Error/Throwable, optionally followed
/// by `: message` — the first line of a Java/Kotlin trace.
static EXC_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\w.$]+(?:Exception|Error|Throwable)(?::|\b)").unwrap());

pub(crate) fn is_exception_header(msg: &str) -> bool {
    EXC_HEADER.is_match(msg)
}

fn is_fatal_header(msg: &str) -> bool {
    msg.starts_with("FATAL EXCEPTION:") || (msg.starts_with("Process:") && msg.contains("PID:"))
}

/// A native crash backtrace frame, e.g. `#00 pc 0001234 /system/lib64/libc.so (…)`.
static NATIVE_FRAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#\d+\s+pc\b").unwrap());

pub(crate) fn is_native_frame(msg: &str) -> bool {
    NATIVE_FRAME.is_match(msg.trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_line;

    fn assemble(lines: &[&str]) -> Vec<Emit> {
        let mut a = Assembler::new();
        let mut out = Vec::new();
        for l in lines {
            out.extend(a.push(parse_line(l)));
        }
        out.extend(a.flush()); // the injected end-of-stream / idle sentinel
        out
    }

    #[test]
    fn assembles_fatal_exception_block_into_one_trace() {
        let lines = [
            "05-30 12:00:00.123  1234  1234 I MyApp: hello",
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: FATAL EXCEPTION: main",
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: Process: com.example.app, PID: 1234",
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: kotlin.IllegalStateException: boom",
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: \tat com.example.app.MainActivity.onCreate(MainActivity.kt:42)",
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: \t... 12 more",
        ];
        let emits = assemble(&lines);
        assert_eq!(emits.len(), 2, "got {emits:#?}");
        assert!(matches!(&emits[0], Emit::Record(r) if r.msg == "hello"));
        match &emits[1] {
            Emit::Trace(t) => {
                assert!(t.is_fatal);
                assert_eq!(t.records.len(), 5);
                assert_eq!(t.key(), (1234, 1234));
            }
            other => panic!("expected fatal trace, got {other:?}"),
        }
    }

    #[test]
    fn thread_change_closes_the_trace() {
        let lines = [
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: kotlin.IllegalStateException: boom",
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: \tat com.example.app.A.f(A.kt:1)",
            "05-30 12:00:01.000  1234  9999 D Other: unrelated, different thread",
        ];
        let emits = assemble(&lines);
        assert_eq!(emits.len(), 2, "got {emits:#?}");
        assert!(matches!(&emits[0], Emit::Trace(t) if t.records.len() == 2 && !t.is_fatal));
        assert!(matches!(&emits[1], Emit::Record(r) if r.tag == "Other"));
    }

    #[test]
    fn flush_emits_a_still_pending_trace() {
        // Trace with no following non-matching line: only flush() releases it,
        // proving the live path can surface a crash that is the last thing emitted.
        let lines = [
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: kotlin.IllegalStateException: boom",
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: \tat com.example.app.A.f(A.kt:1)",
        ];
        let emits = assemble(&lines);
        assert_eq!(emits.len(), 1);
        assert!(matches!(&emits[0], Emit::Trace(_)));
    }

    #[test]
    fn info_log_mentioning_exception_is_not_a_trace() {
        let lines = ["05-30 12:00:00.000  1  1 I MyApp: NullPointerException seen in retry path"];
        let emits = assemble(&lines);
        assert_eq!(emits.len(), 1);
        assert!(matches!(&emits[0], Emit::Record(_)));
    }

    #[test]
    fn assembles_native_backtrace_block() {
        let lines = [
            "10-01 10:00:00.000  4120  4120 F DEBUG: signal 11 (SIGSEGV), code 1, fault addr 0x0",
            "10-01 10:00:00.000  4120  4120 F DEBUG: #00 pc 0000000000012345  /system/lib64/libc.so (abort+164)",
            "10-01 10:00:00.000  4120  4120 F DEBUG: #01 pc 0000000000067890  /data/app/lib/libnative.so (Java_x+8)",
        ];
        let emits = assemble(&lines);
        // The signal line is a standalone record; the two #NN pc frames form a native trace.
        assert_eq!(emits.len(), 2, "got {emits:#?}");
        assert!(matches!(&emits[0], Emit::Record(r) if r.msg.contains("SIGSEGV")));
        match &emits[1] {
            Emit::Trace(t) => {
                assert_eq!(t.kind, TraceKind::Native);
                assert!(t.is_fatal);
                assert_eq!(t.records.len(), 2);
            }
            other => panic!("expected native trace, got {other:?}"),
        }
    }

    #[test]
    fn divider_flushes_pending_trace_then_passes_through() {
        let lines = [
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: kotlin.IllegalStateException: boom",
            "--------- beginning of crash",
        ];
        let emits = assemble(&lines);
        assert_eq!(emits.len(), 2);
        assert!(matches!(&emits[0], Emit::Trace(_)));
        assert!(matches!(&emits[1], Emit::Divider(b) if b == "crash"));
    }
}
