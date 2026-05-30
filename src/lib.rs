//! zed-logcat (`zlc`) — readable, navigable Android logcat for the Zed editor.
//!
//! The pipeline core is the pure [`run`] function: it consumes an iterator of lines
//! and writes rendered output to a sink, with no knowledge of adb, threads, or the
//! terminal. Tests drive it from a `Vec<&str>`; production wraps it around a live
//! `adb logcat` source. Phase 1 is a raw passthrough — parsing and rendering land in
//! later phases behind this same signature.

pub mod cli;
pub mod config;
pub mod input;

use std::io::{self, Write};

use config::Config;

/// Core pipeline. Phase 1: raw passthrough with a prompt flush after every line so a
/// live tail appears immediately (stdout is block-buffered to a pipe by default).
pub fn run<I, W>(lines: I, out: &mut W, _cfg: &Config) -> io::Result<()>
where
    I: IntoIterator<Item = io::Result<String>>,
    W: Write,
{
    for line in lines {
        let line = line?;
        writeln!(out, "{line}")?;
        out.flush()?;
    }
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
    fn run_passes_lines_through_verbatim() {
        let input = vec![
            Ok("05-30 12:00:00.123  1234  1234 I MyApp: hello".to_string()),
            Ok("05-30 12:00:01.000  1234  1234 E AndroidRuntime: FATAL EXCEPTION: main".to_string()),
        ];
        let mut out = Vec::new();
        run(input, &mut out, &test_cfg()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "05-30 12:00:00.123  1234  1234 I MyApp: hello\n\
             05-30 12:00:01.000  1234  1234 E AndroidRuntime: FATAL EXCEPTION: main\n"
        );
    }

    #[test]
    fn run_propagates_read_errors() {
        let input = vec![Err(io::Error::other("boom"))];
        let mut out = Vec::new();
        let err = run(input, &mut out, &test_cfg()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
    }
}
