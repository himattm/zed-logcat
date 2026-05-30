# zed-logcat templates

These are project-level assets the **zed-android** extension places into a user's
project (or you can copy them by hand). `zlc` itself does not install them.

## Files

- **`zlc.toml`** → project root. Committable per-project style (app package, source
  roots, min level, dedupe, tag width, muted tags). CLI flags override it.
- **`zed/tasks.json`** → `<project>/.zed/tasks.json`. Three tasks (`Logcat: app`,
  `Logcat: crash`, `Logcat: clear + tail`) that run `zlc` from `$ZED_WORKTREE_ROOT`
  with sensible defaults for a long-running tail (`reveal: no_focus`,
  `use_new_terminal: false`, `allow_concurrent_runs: true`). The `Logcat: app` task
  relies on `app_packages` in `zlc.toml` for app-only filtering.
- **`zed/settings.json`** → merge into `<project>/.zed/settings.json`. Ships a custom
  `terminal.path_hyperlink_regexes` to harden frame-link detection.

## Binding tasks to keys

Zed keymaps are **global**, not per-project, so a key binding cannot ship with the
project. Add to your `~/.config/zed/keymap.json`:

```json
[
  { "bindings": { "cmd-l": ["task::Spawn", { "task_name": "Logcat: app" }] } }
]
```

Or just run `task::Spawn` (the task picker) with no binding.

## Notes

- `zlc` needs `adb` on PATH. The `Logcat: crash` task reads the crash buffer
  (`-b crash`).
- Zed's `terminal.minimum_contrast` (default 45) may lighten dimmed framework frames;
  the clickable/non-clickable distinction does not rely on brightness alone (only
  resolvable app frames carry a `path:line` token), so links stay correct regardless.
