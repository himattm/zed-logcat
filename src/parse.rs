//! Parse a single logcat line into a structured record.
//!
//! We do NOT slice fixed columns — pid/tid are space-padded to a width that varies
//! by device, and modern logd inserts an optional `uid` column. Instead a tolerant
//! regex anchors on the single-char level and the `tag:` separator. Anything that
//! doesn't match (a buffer divider is handled separately; vendor noise, a format we
//! don't know) degrades to `Raw` and is passed through verbatim rather than dropped
//! or treated as a fatal error.

use std::sync::LazyLock;

use regex::Regex;

use crate::model::{Level, LogRecord};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedLine {
    Record(LogRecord),
    /// A buffer separator, e.g. `--------- beginning of crash`.
    Divider(String),
    /// A line we could not parse; passed through unchanged.
    Raw(String),
}

/// Tolerant threadtime matcher. Handles: optional leading year; variable-width
/// pid/tid; an optional `uid` column between tid and level; micro/milli fractional
/// seconds; level `S`; tags containing `/`.
static THREADTIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        ^
        (?P<ts>(?:\d{4}-)?\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d+)
        \s+ (?P<pid>\d+)
        \s+ (?P<tid>\d+)
        \s+ (?:(?P<uid>\S+)\s+)?
        (?P<level>[VDIWEFS])
        \s+ (?P<tag>.*?) :\x20?
        (?P<msg>.*)
        $
        ",
    )
    .expect("threadtime regex is valid")
});

pub fn parse_line(line: &str) -> ParsedLine {
    if let Some(rest) = line.strip_prefix("--------- beginning of ") {
        return ParsedLine::Divider(rest.trim().to_string());
    }

    if let Some(caps) = THREADTIME.captures(line) {
        if let Some(level) = caps
            .name("level")
            .and_then(|m| m.as_str().chars().next())
            .and_then(Level::from_char)
        {
            return ParsedLine::Record(LogRecord {
                ts: caps["ts"].to_string(),
                pid: caps["pid"].parse().unwrap_or(0),
                tid: caps["tid"].parse().unwrap_or(0),
                uid: caps.name("uid").map(|m| m.as_str().to_string()),
                level,
                tag: caps["tag"].to_string(),
                msg: caps["msg"].to_string(),
            });
        }
    }

    ParsedLine::Raw(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(line: &str) -> LogRecord {
        match parse_line(line) {
            ParsedLine::Record(r) => r,
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn parses_standard_threadtime() {
        let r = record("05-30 12:00:00.456  1234  1300 D OkHttp: <- 200 in 142 ms");
        assert_eq!(r.pid, 1234);
        assert_eq!(r.tid, 1300);
        assert_eq!(r.level, Level::Debug);
        assert_eq!(r.tag, "OkHttp");
        assert_eq!(r.msg, "<- 200 in 142 ms");
        assert!(r.uid.is_none());
    }

    #[test]
    fn parses_uid_column_variant() {
        let r = record("05-30 12:00:00.456  1234  1300 10123 W MyApp: careful");
        assert_eq!(r.level, Level::Warn);
        assert_eq!(r.tag, "MyApp");
        assert_eq!(r.uid.as_deref(), Some("10123"));
        assert_eq!(r.msg, "careful");
    }

    #[test]
    fn parses_year_prefix_and_five_digit_ids() {
        let r = record(
            "2026-05-30 12:00:00.456789  12345  67890 E AndroidRuntime: FATAL EXCEPTION: main",
        );
        assert_eq!(r.pid, 12345);
        assert_eq!(r.tid, 67890);
        assert_eq!(r.level, Level::Error);
        assert_eq!(r.tag, "AndroidRuntime");
        assert_eq!(r.msg, "FATAL EXCEPTION: main");
    }

    #[test]
    fn keeps_first_colon_as_tag_boundary() {
        let r = record("05-30 12:00:00.123  1  1 I MyApp/Network: request: GET /v1");
        assert_eq!(r.tag, "MyApp/Network");
        assert_eq!(r.msg, "request: GET /v1");
    }

    #[test]
    fn frame_message_preserves_leading_tab() {
        let r = record(
            "05-30 12:00:01.000  1234  1234 E AndroidRuntime: \tat com.example.app.MainActivity.onCreate(MainActivity.kt:42)",
        );
        assert!(r.msg.starts_with('\t'), "msg was {:?}", r.msg);
    }

    #[test]
    fn recognizes_buffer_divider() {
        assert_eq!(
            parse_line("--------- beginning of crash"),
            ParsedLine::Divider("crash".to_string())
        );
    }

    #[test]
    fn unparseable_falls_back_to_raw() {
        let s = "not a logcat line at all";
        assert_eq!(parse_line(s), ParsedLine::Raw(s.to_string()));
    }

    #[test]
    fn chatty_drop_parses_as_ordinary_record() {
        // The device's own dedupe ("chatty") is a normal record; phase 6 special-cases it.
        let r = record("05-30 12:00:00.000  1234  1234 I chatty: uid=10101(com.example) identical 47 lines dropped");
        assert_eq!(r.tag, "chatty");
        assert!(r.msg.contains("identical 47 lines dropped"));
    }
}
