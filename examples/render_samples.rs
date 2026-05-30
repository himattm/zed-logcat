//! Render one captured logcat session in several candidate styles, for choosing the
//! house style. Plain text by default (set ZLC_DEMO_COLOR=1 in a real terminal to see
//! the 16-color ANSI version mapped to your theme).
//!
//!   cargo run --example render_samples                       # default fixture
//!   cargo run --example render_samples -- tests/fixtures/session.log
//!   ZLC_DEMO_COLOR=1 cargo run --example render_samples

use std::fs;
use std::path::PathBuf;

use zlc::parse::parse_line;
use zlc::render::{ChipStyle, RenderOptions, Renderer, TagAlign};
use zlc::resolve::Resolver;
use zlc::trace::Assembler;

fn render_all(lines: &[String], opts: RenderOptions, root: Option<PathBuf>) -> String {
    let mut asm = Assembler::new();
    let mut renderer = Renderer::new(opts);
    if let Some(root) = root {
        renderer.set_resolver(Resolver::new(root));
    }
    let mut out = Vec::new();
    for l in lines {
        for emit in asm.push(parse_line(l)) {
            renderer.render(&emit, &mut out).unwrap();
        }
    }
    for emit in asm.flush() {
        renderer.render(&emit, &mut out).unwrap();
    }
    String::from_utf8(out).unwrap()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tests/fixtures/session.log".to_string());
    let content = fs::read_to_string(&path).expect("read fixture");
    let lines: Vec<String> = content.lines().map(str::to_string).collect();

    let color = std::env::var_os("ZLC_DEMO_COLOR").is_some();
    // Set ZLC_ROOT to a project root to see app stack frames rewritten to clickable
    // worktree-relative paths (e.g. ZLC_ROOT=tests/fixtures/project).
    let root = std::env::var_os("ZLC_ROOT").map(PathBuf::from);
    // ZLC_CONNECTOR overrides the continuation-line connector glyph (default '│').
    let connector = std::env::var("ZLC_CONNECTOR")
        .ok()
        .and_then(|s| s.chars().next())
        .unwrap_or('│');
    let width = 90;

    let variants = [
        (
            "A  reverse chip · right tag(17) · no time   [approved spec]",
            RenderOptions { color, width, tag_width: 17, show_time: false, wrap: true, chip: ChipStyle::Reverse, tag_align: TagAlign::Right, connector },
        ),
        (
            "B  reverse chip · right tag(17) · time",
            RenderOptions { color, width, tag_width: 17, show_time: true, wrap: true, chip: ChipStyle::Reverse, tag_align: TagAlign::Right, connector },
        ),
        (
            "C  bar chip · left tag(20) · time",
            RenderOptions { color, width, tag_width: 20, show_time: true, wrap: true, chip: ChipStyle::Bar, tag_align: TagAlign::Left, connector },
        ),
        (
            "D  bracket chip · right tag(23) · no time",
            RenderOptions { color, width, tag_width: 23, show_time: false, wrap: true, chip: ChipStyle::Bracket, tag_align: TagAlign::Right, connector },
        ),
    ];

    for (title, opts) in variants {
        println!("\n┌─ {title}");
        print!("{}", render_all(&lines, opts, root.clone()));
    }
}
