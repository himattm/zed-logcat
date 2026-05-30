//! Phase 1 end-to-end: piping a stream into `zlc` streams it back out (raw
//! passthrough), and the process exits cleanly. Runs with no device and no adb.

use assert_cmd::Command;
use std::fs;

#[test]
fn piped_stream_passes_through_and_exits() {
    let fixture = fs::read_to_string("tests/fixtures/sample.log").unwrap();

    let assert = Command::cargo_bin("zlc")
        .unwrap()
        .write_stdin(fixture.clone())
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();

    // Every input line appears in the output (phase 1 == verbatim passthrough).
    for line in fixture.lines() {
        assert!(stdout.contains(line), "output missing line: {line:?}");
    }
    assert!(stdout.contains("FATAL EXCEPTION: main"));
    assert!(stdout.contains("MainActivity.kt:42"));
}
