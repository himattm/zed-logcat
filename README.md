# zed-logcat (`zlc`)

Readable, navigable Android logcat for the [Zed](https://zed.dev) editor.

`zlc` runs `adb logcat` (or reads a piped stream) and renders it so logs are colorized,
app-filtered, and — the headline payoff — **stack frames are clickable**: Cmd+click
(macOS) / Ctrl+click (Linux/Win) a frame in Zed's integrated terminal and Zed opens the
source at that exact line.

It is a standalone CLI, intended as a dependency of a forthcoming **zed-android**
extension (Zed extensions are WASM and cannot render terminal output, so the log view
must be an external CLI). It wraps `adb` directly and needs only `adb` on your PATH.

## Status

Early, under active development. See the implementation plan and build phases. Currently
implemented: **Phase 1** — CLI skeleton, input layer (spawns `adb logcat -v threadtime`,
or reads a piped stdin), raw passthrough, signal/child teardown.

## Usage

```sh
zlc                       # runs adb itself; needs a device/emulator
zlc < fixtures/crash.log  # reads stdin; replay a capture, no device
adb logcat | zlc          # reads stdin; live piped stream
```

`adb` must be on your PATH (same as pidcat).

## Build & test

```sh
cargo build
cargo test          # full suite runs with no device and no adb present
cargo run --example zed_probe -- src/main.rs   # Phase 0 clickability probe
```
