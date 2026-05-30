//! Phase 0 spike — prove Zed makes our path tokens clickable BEFORE building the
//! real pipeline around a token format.
//!
//! Run it from a Zed integrated terminal opened at a real project root, passing a
//! path that actually exists under that root:
//!
//!     cargo run --example zed_probe -- src/main.rs
//!
//! Then Cmd+click (macOS) / Ctrl+click (Linux/Win) each printed token and record
//! which forms open the file at the right line. Findings drive the emitted-token
//! contract in `render`/`resolve`. See tests/manual/zed-click.md.

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "src/main.rs".to_string());

    // ANSI helpers (raw escapes so the probe has no dependencies).
    let dim = "\x1b[2m";
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";

    println!("Phase 0 probe — Cmd/Ctrl+click each token; note which ones open in Zed.\n");
    println!("Worktree-relative path under test: {path}\n");

    println!("  1. bare path:line          {path}:10");
    println!("  2. path:line:col           {path}:10:1");
    println!("  3. wrapped in parens       ({path}:10)");
    println!("  4. styled (path bold)      {bold}{path}:10:1{reset}");
    println!(
        "  5. reset-isolated token    {dim}at com.example.Foo.bar({reset} {path}:10:1 {dim}){reset}"
    );
    println!("  6. after a wide tag column           SomeReallyLongTagName  {path}:10:1");
    println!("  7. path containing '+'     odd+dir/{path}:10:1   (expected: NOT clickable)");
    println!("  8. absolute path           {}:10:1", std::env::current_dir().unwrap().join(&path).display());

    println!(
        "\nExpectation from prior research: #2 (path:line:col), bare/relative, reset-isolated,\n\
         resolved against the worktree root, should be the most reliable. Confirm here."
    );
}
