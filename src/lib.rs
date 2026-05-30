//! zed-logcat (`zlc`) — readable, navigable Android logcat for the Zed editor.
//!
//! The pipeline core is the pure [`run`] function: it consumes an iterator of lines
//! and writes rendered output to a sink, with no knowledge of adb, threads, or the
//! terminal. Tests drive it from a `Vec<&str>`; production wraps it around a live
//! `adb logcat` source.

pub mod cli;
pub mod config;
pub mod dedupe;
pub mod filter;
pub mod input;
pub mod model;
pub mod parse;
pub mod pidtrack;
pub mod render;
pub mod resolve;
pub mod trace;

use std::io::{self, Write};
use std::path::PathBuf;

use config::Config;
use dedupe::Deduper;
use filter::Filter;
use parse::{parse_line, ParsedLine};
use pidtrack::Tracker;
use render::{RenderOptions, Renderer};
use resolve::Resolver;
use trace::{Assembler, Emit};

/// Core pipeline: parse each line, assemble multi-line traces, render to `out`.
///
/// A prompt flush after each line keeps a live tail responsive (stdout is
/// block-buffered to a pipe by default). End-of-stream flushes any trace still being
/// assembled — in the live path an idle-timeout drives the same `flush` so a crash
/// that is the last thing emitted still surfaces.
/// Owned per-stream processing state: parse → assemble → pidtrack → filter → dedupe →
/// render. Shared by the stdin and live paths so behavior is identical; only the
/// driving loop (and when `flush` is called) differs.
struct Pipeline {
    asm: Assembler,
    tracker: Tracker,
    filter: Filter,
    deduper: Deduper,
    renderer: Renderer,
}

impl Pipeline {
    fn new(cfg: &Config) -> anyhow::Result<Self> {
        let tracker = Tracker::new(cfg.packages.clone(), cfg.seed_pids.clone());
        let filter = Filter::new(
            tracker.enabled(),
            &cfg.tag_includes,
            &cfg.tag_excludes,
            cfg.min_level,
        )?;

        let mut opts = RenderOptions::spec(cfg.color, cfg.width);
        opts.tag_width = cfg.tag_width;
        opts.wrap = cfg.wrap;
        let mut renderer = Renderer::new(opts);

        // Explicit source roots from zlc.toml short-circuit auto-discovery.
        let resolver = if cfg.source_roots.is_empty() {
            Resolver::new(cfg.root.clone())
        } else {
            let roots = cfg.source_roots.iter().map(PathBuf::from).collect();
            Resolver::with_source_roots(cfg.root.clone(), roots)
        };
        renderer.set_resolver(resolver);

        Ok(Self {
            asm: Assembler::new(),
            tracker,
            filter,
            deduper: Deduper::new(cfg.dedupe),
            renderer,
        })
    }

    fn line<W: Write>(&mut self, line: &str, out: &mut W) -> io::Result<()> {
        let parsed = parse_line(line);
        // pidtrack observes the full pre-filter stream so it sees ActivityManager
        // lines (a different pid/tag than the app) before any record is dropped.
        if let ParsedLine::Record(r) = &parsed {
            self.tracker.observe(r);
        }
        for emit in self.asm.push(parsed) {
            self.emit(emit, out)?;
        }
        Ok(())
    }

    fn emit<W: Write>(&mut self, emit: Emit, out: &mut W) -> io::Result<()> {
        if self.filter.keep(&emit, self.tracker.pids()) {
            for (e, n) in self.deduper.push(emit) {
                self.renderer.render_counted(&e, n, out)?;
            }
        }
        Ok(())
    }

    /// Release pending assembler + dedupe state — on idle-timeout, child-exit, or EOF.
    fn flush<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        for emit in self.asm.flush() {
            self.emit(emit, out)?;
        }
        for (e, n) in self.deduper.flush() {
            self.renderer.render_counted(&e, n, out)?;
        }
        Ok(())
    }
}

/// Core pipeline over a finite line source (stdin / fixtures): process each line, then
/// flush at end-of-stream. Pure and synchronous — the unit of test coverage.
pub fn run<I, W>(lines: I, out: &mut W, cfg: &Config) -> anyhow::Result<()>
where
    I: IntoIterator<Item = io::Result<String>>,
    W: Write,
{
    let mut pipeline = Pipeline::new(cfg)?;
    for line in lines {
        let line = line?;
        pipeline.line(&line, out)?;
        out.flush()?;
    }
    pipeline.flush(out)?;
    out.flush()?;
    Ok(())
}

/// Live `adb logcat` path: drive the same pipeline from a channel, flushing pending
/// traces/dedup-runs whenever the app goes quiet (no line for `IDLE`), so a crash that
/// is the last thing emitted before a process dies still surfaces promptly.
pub fn run_live<W: Write>(
    stream: &input::AdbStream,
    out: &mut W,
    cfg: &Config,
) -> anyhow::Result<()> {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    const IDLE: Duration = Duration::from_millis(150);
    let mut pipeline = Pipeline::new(cfg)?;
    loop {
        match stream.recv_timeout(IDLE) {
            Ok(Ok(line)) => pipeline.line(&line, out)?,
            Ok(Err(e)) => return Err(e.into()),
            Err(RecvTimeoutError::Timeout) => pipeline.flush(out)?,
            Err(RecvTimeoutError::Disconnected) => {
                pipeline.flush(out)?;
                out.flush()?;
                break;
            }
        }
        out.flush()?;
    }
    Ok(())
}

/// Real entry point: parse args, resolve config, install signal handling, then drive
/// the chosen source through `run`.
pub fn real_main() -> anyhow::Result<()> {
    use clap::Parser;

    let args = cli::Args::parse();
    let mut cfg = args.resolve()?;

    // Seed the followed PID set for an already-running app (live adb path only).
    if !cfg.read_stdin && !cfg.packages.is_empty() {
        cfg.seed_pids = input::seed_pids(&cfg);
    }

    // On Ctrl-C, kill the adb process group (Drop won't run on exit) then exit 130.
    let _ = ctrlc::set_handler(|| {
        input::kill_child_group();
        std::process::exit(130);
    });

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if cfg.read_stdin {
        run(input::stdin_lines(), &mut out, &cfg)?;
    } else {
        let stream = input::spawn_adb_stream(&cfg)?;
        run_live(&stream, &mut out, &cfg)?;
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Level;
    use std::path::PathBuf;

    fn test_cfg() -> Config {
        Config {
            serial: None,
            clear: false,
            crash: false,
            root: PathBuf::from("."),
            color: false,
            width: 80,
            read_stdin: true,
            packages: Vec::new(),
            tag_includes: Vec::new(),
            tag_excludes: Vec::new(),
            min_level: Level::Verbose,
            dedupe: true,
            tag_width: 17,
            wrap: true,
            source_roots: Vec::new(),
            seed_pids: std::collections::HashSet::new(),
        }
    }

    #[test]
    fn run_renders_records_without_color() {
        let lines = vec![
            Ok("05-30 12:00:00.123  1  1 I MyApp: hello".to_string()),
            Ok("05-30 12:00:00.200  1  1 D MyApp: world".to_string()),
        ];
        let mut out = Vec::new();
        run(lines, &mut out, &test_cfg()).unwrap();
        let text = String::from_utf8(out).unwrap();

        assert!(!text.contains('\x1b'), "color must be off: {text:?}");
        assert!(text.contains("hello") && text.contains("world"));
        // changes-only tag: the repeated tag is printed once.
        assert_eq!(text.matches("MyApp").count(), 1);
    }

    #[test]
    fn run_drops_other_tags_when_including() {
        let mut cfg = test_cfg();
        cfg.tag_includes = vec!["MyApp".to_string()];
        let lines = vec![
            Ok("05-30 12:00:00.123  1  1 I MyApp: keep".to_string()),
            Ok("05-30 12:00:00.200  1  1 D OkHttp: drop".to_string()),
        ];
        let mut out = Vec::new();
        run(lines, &mut out, &cfg).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("keep"));
        assert!(!text.contains("drop"));
    }

    #[test]
    fn run_propagates_read_errors() {
        let input = vec![Err(io::Error::other("boom"))];
        let mut out = Vec::new();
        let err = run(input, &mut out, &test_cfg()).unwrap_err();
        assert_eq!(
            err.downcast::<io::Error>().unwrap().kind(),
            io::ErrorKind::Other
        );
    }
}
