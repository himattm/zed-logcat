//! Render `Emit`s (records, assembled traces, dividers) as terminal text.
//!
//! Color uses the 16-color ANSI palette via raw SGR codes so it maps to the user's
//! Zed theme (no hardcoded truecolor) and survives `minimum_contrast`. The level chip
//! is a reverse-video letter, and the whole line — tag + message — is painted in that
//! same level color (red for E, yellow for W, …), so severity reads at a glance. The
//! tag sits in a fixed column, printed only when it changes. Messages wrap with a
//! hanging indent aligned under the message column.
//!
//! Phase 3 renders trace lines as styled text; Phase 4 rewrites app stack frames to
//! clickable project-relative `path:line:col` tokens.

use std::io::{self, Write};

use crate::model::Level;
use crate::resolve::{parse_frame, Frame, Resolver};
use crate::trace::{is_exception_header, Emit, Trace};

#[derive(Clone, Copy, Debug)]
pub enum ChipStyle {
    /// Reverse-video letter, e.g. a colored block containing `E` (the house style).
    Reverse,
    /// Bracketed colored letter, e.g. `[E]`.
    Bracket,
    /// Colored bar then letter, e.g. `▌ E`.
    Bar,
}

#[derive(Clone, Copy, Debug)]
pub enum TagAlign {
    Right,
    Left,
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub color: bool,
    pub width: usize,
    pub tag_width: usize,
    pub show_time: bool,
    pub wrap: bool,
    pub chip: ChipStyle,
    pub tag_align: TagAlign,
    /// Glyph drawn in the chip column on continuation lines (wrapped messages and
    /// trace bodies) so a multi-line block reads as one connected unit.
    pub connector: char,
}

impl RenderOptions {
    /// The approved house style.
    pub fn spec(color: bool, width: usize) -> Self {
        Self {
            color,
            width,
            tag_width: 17,
            show_time: false,
            wrap: true,
            chip: ChipStyle::Reverse,
            tag_align: TagAlign::Right,
            connector: '┃',
        }
    }
}

/// Visible width of the level chip column (all chip styles render to 3 columns).
const CHIP_W: usize = 3;
/// Visible width of the time column when shown ("12:00:00.123" + a space).
const TIME_W: usize = 13;

pub struct Renderer {
    opts: RenderOptions,
    last_tag: Option<String>,
    resolver: Resolver,
}

impl Renderer {
    pub fn new(opts: RenderOptions) -> Self {
        Self {
            opts,
            last_tag: None,
            resolver: Resolver::disabled(),
        }
    }

    /// Attach a resolver so app stack frames become clickable worktree-relative paths.
    pub fn set_resolver(&mut self, resolver: Resolver) {
        self.resolver = resolver;
    }

    pub fn render<W: Write>(&mut self, emit: &Emit, out: &mut W) -> io::Result<()> {
        match emit {
            Emit::Record(r) => self.record_line(r.level, &r.ts, &r.tag, &r.msg, out),
            Emit::Trace(t) => self.trace_block(t, out),
            Emit::Divider(buf) => self.divider(buf, out),
            Emit::Raw(s) => writeln!(out, "{s}"),
        }
    }

    /// Left offset of the message column (where continuation/wrapped lines align).
    fn indent(&self) -> usize {
        let time = if self.opts.show_time { TIME_W } else { 0 };
        time + CHIP_W + 1 + self.opts.tag_width + 1
    }

    fn record_line<W: Write>(
        &mut self,
        level: Level,
        ts: &str,
        tag: &str,
        msg: &str,
        out: &mut W,
    ) -> io::Result<()> {
        let changed = self.last_tag.as_deref() != Some(tag);
        self.last_tag = Some(tag.to_string());

        let lc = level_fg(level);
        let time = if self.opts.show_time {
            self.time_cell(ts)
        } else {
            String::new()
        };
        let prefix = format!("{time}{} {} ", self.chip(level), self.tag_cell(tag, changed, lc));
        let indent = self.indent();
        let avail = self.opts.width.saturating_sub(indent).max(8);

        let chunks = if self.opts.wrap {
            wrap_words(msg, avail)
        } else {
            vec![msg.to_string()]
        };
        for (i, chunk) in chunks.iter().enumerate() {
            let painted = paint(self.opts.color, lc, chunk);
            if i == 0 {
                writeln!(out, "{prefix}{painted}")?;
            } else {
                writeln!(out, "{}{painted}", self.cont_prefix(lc))?;
            }
        }
        Ok(())
    }

    /// Indent for continuation lines: a blank time column, the connector glyph in the
    /// chip column (painted in the block's level color), then blanks to the message
    /// column. Total visible width equals [`Self::indent`].
    fn cont_prefix(&self, lc: &str) -> String {
        let time = if self.opts.show_time {
            " ".repeat(TIME_W)
        } else {
            String::new()
        };
        let conn = paint(self.opts.color, lc, &format!(" {} ", self.opts.connector));
        let rest = " ".repeat(1 + self.opts.tag_width + 1);
        format!("{time}{conn}{rest}")
    }

    fn trace_block<W: Write>(&mut self, t: &Trace, out: &mut W) -> io::Result<()> {
        let head = &t.records[0];
        self.record_line(head.level, &head.ts, &head.tag, &head.msg, out)?;
        let lc = level_fg(head.level);
        for rec in &t.records[1..] {
            let body = self.trace_line(&rec.msg, lc);
            writeln!(out, "{}{body}", self.cont_prefix(lc))?;
        }
        Ok(())
    }

    fn divider<W: Write>(&mut self, buf: &str, out: &mut W) -> io::Result<()> {
        self.last_tag = None; // a buffer break resets changes-only tracking
        let line = format!("──── beginning of {buf} ────");
        writeln!(out, "{}", paint(self.opts.color, "2", &line))
    }

    fn chip(&self, level: Level) -> String {
        let letter = level.letter();
        let fg = level_fg(level);
        match self.opts.chip {
            ChipStyle::Reverse => paint(self.opts.color, &format!("7;{fg}"), &format!(" {letter} ")),
            ChipStyle::Bracket => paint(self.opts.color, fg, &format!("[{letter}]")),
            ChipStyle::Bar => format!("{} {letter}", paint(self.opts.color, fg, "▌")),
        }
    }

    fn tag_cell(&self, tag: &str, changed: bool, lc: &str) -> String {
        let w = self.opts.tag_width;
        let text = if changed { truncate(tag, w) } else { String::new() };
        let padded = match self.opts.tag_align {
            TagAlign::Right => format!("{text:>w$}"),
            TagAlign::Left => format!("{text:<w$}"),
        };
        paint(self.opts.color && changed, lc, &padded)
    }

    fn time_cell(&self, ts: &str) -> String {
        // ts is "MM-DD HH:MM:SS.mmm" (optionally year-prefixed); show just the time.
        let time = ts.rsplit(' ').next().unwrap_or(ts);
        let time: String = time.chars().take(12).collect();
        paint(self.opts.color, "2", &format!("{time:<12} "))
    }

    /// Trace continuation lines, painted in the block's level color (`lc`). The
    /// exception type and `Caused by:`/`Suppressed:` delimiters are bolded; frames are
    /// rewritten ([`Self::frame_line`]); `… N more` is dimmed-indented.
    fn trace_line(&self, msg: &str, lc: &str) -> String {
        let t = msg.trim_start();
        let c = self.opts.color;
        let bold = format!("1;{lc}");
        if t.starts_with("...") && t.ends_with("more") {
            paint(c, lc, &format!("  {t}"))
        } else if let Some(frame) = parse_frame(t) {
            self.frame_line(&frame, lc)
        } else if t.starts_with("Caused by:") || t.starts_with("Suppressed:") || is_exception_header(t) {
            paint(c, &bold, t)
        } else {
            paint(c, lc, t) // e.g. "Process: …, PID:" header
        }
    }

    /// Render one stack frame. App/owned frames (resolvable under a worktree source
    /// root) show `at Class.method   <relative-path>:line`, with the path token left
    /// UNSTYLED and space-isolated so Zed's detector links exactly it. Framework /
    /// unresolved frames are dimmed and keep their full FQN, with the embedded
    /// `(File.ext:NN)` dropped so Zed has no token to dead-click.
    fn frame_line(&self, f: &Frame, lc: &str) -> String {
        let c = self.opts.color;
        if let (Some(file), Some(line)) = (f.file.as_deref(), f.line) {
            if let Some(rel) = self.resolver.resolve(&f.pkg_path, file) {
                let head = paint(c, lc, &format!("  at {}", f.short));
                // The reset that `paint` appends leaves the token in the terminal's
                // default style — bare, space-surrounded, exactly what Zed links.
                return format!("{head}   {rel}:{line}");
            }
        }
        paint(c, "2", &format!("  at {}", f.fqmethod))
    }
}

fn paint(on: bool, codes: &str, s: &str) -> String {
    if on {
        format!("\x1b[{codes}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn level_fg(level: Level) -> &'static str {
    match level {
        Level::Verbose => "90",
        Level::Debug => "34",
        Level::Info => "32",
        Level::Warn => "33",
        Level::Error => "31",
        Level::Fatal => "1;91",
        Level::Silent => "90",
    }
}

/// Truncate to `w` display columns, keeping the most-specific tail behind a `…`.
fn truncate(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w || w == 0 {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - (w - 1)).collect();
    format!("…{tail}")
}

fn hard_chunks(word: &str, width: usize) -> Vec<String> {
    word.chars()
        .collect::<Vec<_>>()
        .chunks(width)
        .map(|c| c.iter().collect())
        .collect()
}

/// Greedy word wrap to `width` columns; a single over-long word is hard-split.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;

    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let wlen = word.chars().count();
        if wlen > width {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            let chunks = hard_chunks(word, width);
            let last = chunks.len() - 1;
            for (i, ch) in chunks.into_iter().enumerate() {
                if i < last {
                    lines.push(ch);
                } else {
                    cur_len = ch.chars().count();
                    cur = ch;
                }
            }
            continue;
        }
        let projected = if cur.is_empty() { wlen } else { cur_len + 1 + wlen };
        if projected <= width {
            if !cur.is_empty() {
                cur.push(' ');
                cur_len += 1;
            }
            cur.push_str(word);
            cur_len += wlen;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
            cur_len = wlen;
        }
    }
    lines.push(cur);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LogRecord;

    fn rec(level: Level, tag: &str, msg: &str) -> Emit {
        Emit::Record(LogRecord {
            ts: "05-30 12:00:00.123".to_string(),
            pid: 1,
            tid: 1,
            uid: None,
            level,
            tag: tag.to_string(),
            msg: msg.to_string(),
        })
    }

    fn render(emits: &[Emit], opts: RenderOptions) -> String {
        let mut r = Renderer::new(opts);
        let mut out = Vec::new();
        for e in emits {
            r.render(e, &mut out).unwrap();
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn structural_layout_is_deterministic_without_color() {
        let emits = [
            rec(Level::Info, "MyApp", "hello"),
            rec(Level::Debug, "MyApp", "world"),
        ];
        let got = render(&emits, RenderOptions::spec(false, 80));
        // chip(3) + space + right-aligned tag(17, blank on repeat) + space + message.
        let expected = format!(
            "{} {:>17} {}\n{} {:>17} {}\n",
            " I ", "MyApp", "hello", " D ", "", "world",
        );
        assert_eq!(got, expected);
    }

    #[test]
    fn changes_only_tag_blanks_repeats() {
        let emits = [
            rec(Level::Info, "MyApp", "one"),
            rec(Level::Info, "MyApp", "two"),
            rec(Level::Info, "Other", "three"),
        ];
        let got = render(&emits, RenderOptions::spec(false, 80));
        assert_eq!(got.matches("MyApp").count(), 1, "tag repeated:\n{got}");
        assert_eq!(got.matches("Other").count(), 1);
    }

    #[test]
    fn no_color_means_no_escape_bytes() {
        let got = render(&[rec(Level::Error, "Boom", "kaboom")], RenderOptions::spec(false, 80));
        assert!(!got.contains('\x1b'));
    }

    #[test]
    fn whole_line_painted_in_level_color() {
        // Error -> red (31) tag + message; Warn -> yellow (33).
        let err = render(&[rec(Level::Error, "Boom", "kaboom")], RenderOptions::spec(true, 80));
        assert!(err.contains("\x1b[31mkaboom\x1b[0m"), "{err:?}");
        assert!(err.contains("\x1b[31m") && err.contains("Boom"));

        let warn = render(&[rec(Level::Warn, "W", "careful")], RenderOptions::spec(true, 80));
        assert!(warn.contains("\x1b[33mcareful\x1b[0m"), "{warn:?}");
    }

    #[test]
    fn app_frame_becomes_clickable_token_and_framework_is_neutralized() {
        use std::fs;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let f = dir
            .path()
            .join("app/src/main/kotlin/com/example/app/MainActivity.kt");
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(&f, "// stub\n").unwrap();

        let base = LogRecord {
            ts: "05-30 12:00:01.000".to_string(),
            pid: 1,
            tid: 1,
            uid: None,
            level: Level::Error,
            tag: "AndroidRuntime".to_string(),
            msg: "FATAL EXCEPTION: main".to_string(),
        };
        let app_frame = LogRecord {
            msg: "\tat com.example.app.MainActivity.onCreate(MainActivity.kt:42)".to_string(),
            ..base.clone()
        };
        let fw_frame = LogRecord {
            msg: "\tat android.app.Activity.performCreate(Activity.java:8000)".to_string(),
            ..base.clone()
        };
        let trace = Emit::Trace(Trace {
            is_fatal: true,
            records: vec![base, app_frame, fw_frame],
        });

        let mut r = Renderer::new(RenderOptions::spec(false, 200));
        r.set_resolver(crate::resolve::Resolver::new(dir.path().to_path_buf()));
        let mut out = Vec::new();
        r.render(&trace, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();

        // App frame: rewritten to a worktree-relative clickable token.
        assert!(
            text.contains("at MainActivity.onCreate   app/src/main/kotlin/com/example/app/MainActivity.kt:42"),
            "{text}"
        );
        // Framework frame: dimmed full FQN, embedded (Activity.java:8000) dropped so
        // Zed has no dead token to click.
        assert!(text.contains("at android.app.Activity.performCreate"));
        assert!(!text.contains("Activity.java:8000"));
        // The only `path:line`-shaped token is the resolved app path.
        assert_eq!(text.matches(".kt:42").count(), 1);
    }

    #[test]
    fn long_message_wraps_with_hanging_indent() {
        let emits = [rec(Level::Info, "T", "alpha beta gamma delta epsilon zeta eta theta iota kappa")];
        let got = render(&emits, RenderOptions::spec(false, 40));
        let lines: Vec<&str> = got.lines().collect();
        assert!(lines.len() >= 2, "expected wrap: {got:?}");
        // continuation starts with the connector glyph in the chip column, then aligns.
        assert!(lines[1].starts_with(" ┃ "), "{:?}", lines[1]);
        assert!(lines[1].chars().any(|c| !c.is_whitespace() && c != '┃'));
    }
}
