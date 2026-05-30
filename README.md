# zed-logcat (`zlc`)

Readable, navigable Android logcat for the [Zed](https://zed.dev) editor.

`zlc` runs `adb logcat` (or reads a piped stream) and renders it so logs are colorized,
app-filtered, and — the headline payoff — **stack frames are clickable**: Cmd+click
(macOS) / Ctrl+click (Linux/Win) a frame in Zed's integrated terminal and Zed opens the
source at that exact line.

It is a standalone CLI, intended as a dependency of a forthcoming **zed-android**
extension (Zed extensions are WASM and cannot render terminal output, so the log view
must be an external CLI). It wraps `adb` directly and needs only `adb` on your PATH.

## Features

- **Clickable app frames** — stack frames are rewritten to worktree-relative
  `path:line` tokens Zed links on click; framework frames are dimmed and made
  non-clickable (no dead clicks).
- **Level-painted output** — reverse-video level chip and the whole line colored by
  severity (16-color ANSI, so it follows your Zed theme); a vertical connector ties
  multi-line blocks (wrapped messages, crashes) together.
- **Assembled crashes** — multi-line Java traces collapse into one block with the
  `Caused by:` chain; the `FATAL EXCEPTION` block always shows (bypasses filters).
- **Robust crashes** — native (`#NN pc`) backtraces are grouped and non-navigable; an
  R8/minified trace gets a "retrace with mapping.txt" hint.
- **Filtering** — app-only via PID following (`ActivityManager` lifecycle + `pidof`
  seed, multi-process aware), tag include/exclude globs, and a minimum level.
- **Dedupe** — consecutive identical lines collapse to a `×N` counter.
- **Width-aware** — shrinks then drops the tag column on narrow panes.
- **Live or replay** — auto-detects a piped stdin; otherwise tails `adb logcat` with a
  responsive idle-flush so a quiet app's last line/crash surfaces promptly.

Deferred: `--json` NDJSON (awaiting a consumer); cross-compiled release/distribution.

## Usage

```sh
zlc                          # tails adb logcat; needs a device/emulator
zlc -p com.example.app       # app-only (follows the package across relaunches)
zlc < fixtures/crash.log     # reads stdin; replay a capture, no device
adb logcat | zlc             # reads stdin; live piped stream
```

Common flags (run `zlc --help` for all):

| flag | meaning |
|---|---|
| `-p, --package <PKG>` | follow an app package (app-only filtering); repeatable |
| `-t, --tag <GLOB>` / `-T, --exclude-tag <GLOB>` | include / exclude tags (crashes bypass) |
| `-m, --min-level <V..F>` | minimum level to show |
| `-s, --serial <SERIAL>` | target device (alias `--device`) |
| `-c, --clear` | clear the buffer before tailing |
| `--crash` | read the crash buffer |
| `--root <DIR>` | base for clickable paths (default: `$ZED_WORKTREE_ROOT` → Gradle root → cwd) |
| `--color <auto\|always\|never>` | color control (honors `NO_COLOR`) |
| `--no-dedupe` | disable `×N` collapsing |

`adb` must be on your PATH (same as pidcat).

## Configuration — `zlc.toml`

Commit a `zlc.toml` at the project root for a shared, per-project style (`app_packages`,
`source_roots`, `min_level`, `dedupe`, `tag_width`, `wrap`, `mute_tags`). CLI flags
override it. See `templates/` for the file and the `.zed/` task + settings assets the
zed-android extension places.

## Build & test

```sh
cargo build --release        # binary at target/release/zlc
cargo test                   # full suite runs with no device and no adb present
cargo run --example render_samples -- tests/fixtures/session.log   # render demo
```
