//! Phase 3 end-to-end: piping a captured session into `zlc --color never` renders
//! structured output (messages, tags, assembled crash) with no ANSI bytes, and the
//! process exits cleanly. Runs with no device and no adb.

use assert_cmd::Command;
use std::fs;

#[test]
fn renders_session_without_color() {
    let fixture = fs::read_to_string("tests/fixtures/session.log").unwrap();

    let assert = Command::cargo_bin("zlc")
        .unwrap()
        .arg("--color")
        .arg("never")
        .write_stdin(fixture)
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(!stdout.contains('\u{1b}'), "expected no ANSI with --color never");
    // Messages and tags survive rendering; the crash assembles with its chain.
    assert!(stdout.contains("FATAL EXCEPTION: main"));
    assert!(stdout.contains("Caused by:"));
    assert!(stdout.contains("AppDatabase.kt:31"));
    assert!(stdout.contains("MyApp/Network"));
    // Frames are not yet rewritten to clickable paths (that is Phase 4), but the
    // file:line is present in the text.
    assert!(stdout.contains("MainActivity.kt:42"));
}
