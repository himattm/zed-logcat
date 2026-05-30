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

    assert!(
        !stdout.contains('\u{1b}'),
        "expected no ANSI with --color never"
    );
    // Messages and tags survive rendering; the crash assembles with its chain.
    assert!(stdout.contains("FATAL EXCEPTION: main"));
    assert!(stdout.contains("Caused by:"));
    assert!(stdout.contains("AppDatabase.kt:31"));
    assert!(stdout.contains("MyApp/Network"));
}

#[test]
fn app_frames_resolve_to_clickable_paths_via_root() {
    let fixture = fs::read_to_string("tests/fixtures/session.log").unwrap();

    let assert = Command::cargo_bin("zlc")
        .unwrap()
        .args(["--color", "never", "--root", "tests/fixtures/project"])
        .write_stdin(fixture)
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // App frames -> worktree-relative clickable tokens.
    assert!(
        stdout.contains("app/src/main/kotlin/com/example/app/MainActivity.kt:42"),
        "{stdout}"
    );
    assert!(stdout.contains("app/src/main/kotlin/com/example/app/data/AppDatabase.kt:31"));
    // Framework frames are neutralized (no dead-clickable embedded token).
    assert!(!stdout.contains("Activity.java:8000"));
    assert!(stdout.contains("at android.app.Activity.performCreate"));
}
