//! zed-logcat (`zlc`) — readable, navigable Android logcat for the Zed editor.
//!
//! The pipeline core is the pure [`run`] function: it consumes an iterator of lines
//! and writes rendered output to a sink, with no knowledge of adb, threads, or the
//! terminal. Tests drive it from a `Vec<&str>`; production wraps it around a live
//! `adb logcat` source.

pub mod cli;
pub mod config;
pub mod input;
pub mod model;
pub mod parse;
pub mod render;
pub mod resolve;
pub mod trace;

use std::io::{self, Write};

use config::Config;
use parse::parse_line;
use render::{RenderOptions, Renderer};
use resolve::Resolver;
use trace::Assembler;

/// Core pipeline: parse each line, assemble multi-line traces, render to `out`.
///
/// A prompt flush after each line keeps a live tail responsive (stdout is
/// block-buffered to a pipe by default). End-of-stream flushes any trace still being
/// assembled — in the live path an idle-timeout drives the same `flush` so a crash
/// that is the last thing emitted still surfaces.
pub fn run<I, W>(lines: I, out: &mut W, cfg: &Config) -> io::Result<()>
where
    I: IntoIterator<Item = io::Result<String>>,
    W: Write,
{
    let mut asm = Assembler::new();
    let mut renderer = Renderer::new(RenderOptions::spec(cfg.color, cfg.width));
    renderer.set_resolver(Resolver::new(cfg.root.clone()));

    for line in lines {
        let line = line?;
        for emit in asm.push(parse_line(&line)) {
            renderer.render(&emit, out)?;
        }
        out.flush()?;
    }
    for emit in asm.flush() {
        renderer.render(&emit, out)?;
    }
    out.flush()?;
    Ok(())
}

/// Real entry point: parse args, resolve config, install signal handling, then drive
/// the chosen source through `run`.
pub fn real_main() -> anyhow::Result<()> {
    use clap::Parser;

    let args = cli::Args::parse();
    let cfg = args.resolve()?;

    // On Ctrl-C, kill the adb process group (Drop won't run on exit) then exit 130.
    let _ = ctrlc::set_handler(|| {
        input::kill_child_group();
        std::process::exit(130);
    });

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    let source = input::make_source(&cfg)?;
    run(source, &mut out, &cfg)?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn run_propagates_read_errors() {
        let input = vec![Err(io::Error::other("boom"))];
        let mut out = Vec::new();
        let err = run(input, &mut out, &test_cfg()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
    }
}
