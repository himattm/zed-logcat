//! Phase 8 end-to-end: a committed `zlc.toml` is loaded from `--root` and applied.

use assert_cmd::Command;
use std::fs;

#[test]
fn zlc_toml_mute_tags_are_applied() {
    let session = fs::read_to_string("tests/fixtures/session.log").unwrap();

    let assert = Command::cargo_bin("zlc")
        .unwrap()
        .args(["--color", "never", "--root", "tests/fixtures/configtest"])
        .write_stdin(session)
        .assert()
        .success();

    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // zlc.toml mutes OkHttp, so those records are gone…
    assert!(!out.contains("OkHttp"), "muted tag should be gone:\n{out}");
    // …but other app logs remain, and the crash bypasses filtering.
    assert!(out.contains("MyApp/Network"));
    assert!(out.contains("FATAL EXCEPTION"));
}
