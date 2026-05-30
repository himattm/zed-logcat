//! Phase 5 end-to-end: app-only PID following and the crash bypass through the binary.

use assert_cmd::Command;
use std::fs;

fn run(args: &[&str]) -> String {
    let fixture = fs::read_to_string("tests/fixtures/filter.log").unwrap();
    let assert = Command::cargo_bin("zlc")
        .unwrap()
        .args(["--color", "never"])
        .args(args)
        .write_stdin(fixture)
        .assert()
        .success();
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

#[test]
fn app_only_follows_pid_from_activitymanager() {
    // -p enables app-only; pidtrack learns pid 4120 from the Start proc line.
    let out = run(&["-p", "com.example.app"]);

    assert!(out.contains("app started"), "{out}");
    assert!(out.contains("app network call"));
    // Another app's pid is filtered out.
    assert!(!out.contains("noise from another app"));
    // The ActivityManager line itself (system_server pid 600) is filtered too, but it
    // still seeded the tracker.
    assert!(!out.contains("Start proc"));
    // The crash from the tracked app shows.
    assert!(out.contains("FATAL EXCEPTION"));
}

#[test]
fn crash_bypasses_tag_exclusion() {
    // Even excluding AndroidRuntime, the FATAL EXCEPTION block punches through.
    let out = run(&["-p", "com.example.app", "-T", "AndroidRuntime"]);
    assert!(out.contains("FATAL EXCEPTION"), "{out}");
}

#[test]
fn min_level_drops_low_priority() {
    // Without app filtering: -m W keeps W/E and up, drops I/D.
    let out = run(&["-m", "W"]);
    assert!(!out.contains("app started")); // I
    assert!(!out.contains("app network call")); // D
    assert!(out.contains("FATAL EXCEPTION")); // E
}
