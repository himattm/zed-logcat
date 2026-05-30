# Manual runbook — verify clickable frames in Zed

This is the one verification step that cannot run in CI: it needs a live Zed GUI and a
human click. Everything about *which token we emit* is covered by automated tests; this
step only confirms Zed's live link detector behaves as researched.

Pinned tested Zed version: _record the version you tested here_ (Zed → About).

## 1. Get a real project with real source paths

Use any existing Android app, or scaffold a throwaway one. If you have the `android`
CLI handy, the single template is `empty-activity` (note: there is **no**
`empty-activity-agp-9`):

```sh
android create empty-activity --name "Zlc Sample" -o /tmp/zlc-sample
```

Any project with files at real `path:line` locations works — it does not have to be
Android for the Phase 0 probe.

## 2. Run the Phase 0 probe from a Zed terminal opened at that project root

```sh
cargo run --example zed_probe -- app/src/main/java/com/example/zlcsample/MainActivity.kt
```

(Use a path that actually exists under the project root.)

## 3. Cmd/Ctrl+click each printed token

Record which forms open the file at the right line:

- [ ] `path:line`
- [ ] `path:line:col`
- [ ] in parens `(path:line)` (expected: not clickable — we strip parens)
- [ ] styled / bold path
- [ ] reset-isolated token (style reset immediately around the token)
- [ ] token after a wide right-aligned tag column
- [ ] path containing `+` (expected: NOT clickable)
- [ ] absolute path

**Go/no-go:** confirm the exact token format Zed opens reliably. If worktree-relative
`path:line:col` does not resolve, fall back to a shipped `terminal.path_hyperlink_regexes`
(in `.zed/settings.json`) or to absolute paths — decide this *before* Phase 4 wires the
token format into `resolve`/`render`.

## 4. Later: real logcat end-to-end (Phase 4+)

```sh
adb logcat -d -v threadtime > fixtures/crash.log   # capture once
cargo run -- < fixtures/crash.log                  # replay, render, click app frames
```

Confirm app frames are clickable and jump to `file:line`; framework frames stay dim and
non-clickable. Also click a *wrapped* frame and a frame on a *long-tag* line.
