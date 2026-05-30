//! Turn a stack frame into a clickable, worktree-relative `path:line` — or decide it
//! is a framework/unresolvable frame that must NOT be linkified.
//!
//! Key insight (verified): a Java/Kotlin `threadtime` frame already embeds the source
//! filename and line, e.g. `at com.example.app.MainActivity.onCreate(MainActivity.kt:42)`.
//! So we do NOT index class names (unreliable: Kotlin allows many classes per file and
//! compiles top-level funcs to `FooKt`). Instead we read the literal filename from the
//! frame, derive the package path from the fully-qualified method, and locate the file
//! under a worktree source root whose directory ends with that package path.
//!
//! Resolution is lazy and cached. Source roots are discovered once on first use (no
//! startup walk for sessions that never crash). A frame is "app/owned" iff it resolves
//! to a real file under a worktree source root (excluding generated `build/` output);
//! everything else is framework and is rendered without a clickable token.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// A parsed stack frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Fully-qualified `package.Class.method` (or `Outer$Inner.method`).
    pub fqmethod: String,
    /// Short `Class.method` for display on resolved app frames.
    pub short: String,
    /// Package as a path, e.g. `com/example/app` (empty for the default package).
    pub pkg_path: String,
    /// Source filename embedded in the frame, e.g. `MainActivity.kt`.
    pub file: Option<String>,
    /// 1-based source line, when present.
    pub line: Option<u32>,
    /// The location was `SourceFile` / `Unknown Source` — a sign of an R8/minified build.
    pub obfuscated: bool,
}

/// Parse a trimmed trace line of the form `at <fqmethod>(<loc>)`. Returns `None` for
/// non-frame lines. `loc` is `File.ext:line`, or `Native Method` / `Unknown Source` /
/// `SourceFile` (minified) — the latter yield no file/line and resolve to framework.
pub fn parse_frame(line: &str) -> Option<Frame> {
    let rest = line.strip_prefix("at ")?;
    let open = rest.find('(')?;
    let close = rest.rfind(')')?;
    if close < open {
        return None;
    }
    let fqmethod = rest[..open].trim();
    let loc = &rest[open + 1..close];

    let (file, line) = match loc.rsplit_once(':') {
        Some((f, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
            (Some(f.to_string()), n.parse().ok())
        }
        _ => (None, None),
    };
    let obfuscated = loc.starts_with("SourceFile") || loc.starts_with("Unknown Source");

    let segs: Vec<&str> = fqmethod.split('.').collect();
    if segs.len() < 2 {
        return None;
    }
    let short = format!("{}.{}", segs[segs.len() - 2], segs[segs.len() - 1]);
    let pkg_path = if segs.len() >= 3 {
        segs[..segs.len() - 2].join("/")
    } else {
        String::new()
    };

    Some(Frame {
        fqmethod: fqmethod.to_string(),
        short,
        pkg_path,
        file,
        line,
        obfuscated,
    })
}

/// Resolves frame `(package, filename)` pairs to worktree-relative paths.
pub struct Resolver {
    root: PathBuf,
    enabled: bool,
    source_roots: OnceCell<Vec<PathBuf>>,
    cache: RefCell<HashMap<(String, String), Option<String>>>,
}

impl Resolver {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            enabled: true,
            source_roots: OnceCell::new(),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Like [`Self::new`] but with explicit source roots (from `zlc.toml`), skipping
    /// auto-discovery. Relative roots are resolved against `root`.
    pub fn with_source_roots(root: PathBuf, roots: Vec<PathBuf>) -> Self {
        let resolver = Self::new(root.clone());
        let abs = roots
            .into_iter()
            .map(|r| if r.is_absolute() { r } else { root.join(r) })
            .collect();
        let _ = resolver.source_roots.set(abs);
        resolver
    }

    /// A resolver that resolves nothing (used where there is no worktree, e.g. tests
    /// that exercise only layout). Every frame is treated as framework.
    pub fn disabled() -> Self {
        Self {
            root: PathBuf::from("."),
            enabled: false,
            source_roots: OnceCell::new(),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Resolve to a worktree-relative path (without the `:line`), or `None` if the file
    /// is not under a source root (framework/generated) or would not be Zed-clickable.
    pub fn resolve(&self, pkg_path: &str, file: &str) -> Option<String> {
        if !self.enabled || pkg_path.is_empty() {
            return None;
        }
        let key = (pkg_path.to_string(), file.to_string());
        if let Some(hit) = self.cache.borrow().get(&key) {
            return hit.clone();
        }
        let result = self.compute(pkg_path, file);
        self.cache.borrow_mut().insert(key, result.clone());
        result
    }

    fn compute(&self, pkg_path: &str, file: &str) -> Option<String> {
        let roots = self
            .source_roots
            .get_or_init(|| discover_source_roots(&self.root));
        let suffix = Path::new(pkg_path).join(file);

        let mut hits: Vec<PathBuf> = roots
            .iter()
            .map(|r| r.join(&suffix))
            .filter(|p| p.is_file())
            .collect();
        if hits.is_empty() {
            return None;
        }
        // Deterministic tie-break: prefer src/main, then lexically first.
        hits.sort_by(|a, b| {
            let am = a.to_string_lossy().contains("/src/main/");
            let bm = b.to_string_lossy().contains("/src/main/");
            bm.cmp(&am).then_with(|| a.cmp(b))
        });

        let rel = hits[0].strip_prefix(&self.root).ok()?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        // Paths containing '+' or '$' are not reliably clickable in Zed; better to dim
        // the frame than emit a dead link.
        if rel.contains('+') || rel.contains('$') {
            return None;
        }
        Some(rel)
    }
}

/// Find directories that look like source roots: `.../src/<variant>/{java,kotlin}`,
/// skipping hidden dirs (via the walker) and any `build/` (generated) output.
fn discover_source_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for dent in WalkBuilder::new(root).build().flatten() {
        if !dent.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        let path = dent.path();
        if path.components().any(|c| c.as_os_str() == "build") {
            continue;
        }
        let parts: Vec<&std::ffi::OsStr> = path.iter().collect();
        let n = parts.len();
        if n >= 3 && parts[n - 3] == "src" && (parts[n - 1] == "java" || parts[n - 1] == "kotlin") {
            roots.push(path.to_path_buf());
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn touch(root: &Path, rel: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "// stub\n").unwrap();
    }

    // ---- parse_frame ----

    #[test]
    fn parses_app_frame() {
        let f = parse_frame("at com.example.app.MainActivity.onCreate(MainActivity.kt:42)").unwrap();
        assert_eq!(f.pkg_path, "com/example/app");
        assert_eq!(f.short, "MainActivity.onCreate");
        assert_eq!(f.file.as_deref(), Some("MainActivity.kt"));
        assert_eq!(f.line, Some(42));
    }

    #[test]
    fn parses_inner_class_without_dollar_in_path() {
        let f = parse_frame("at com.example.app.Outer$Inner.run(Outer.kt:7)").unwrap();
        assert_eq!(f.pkg_path, "com/example/app"); // '$' stays in the class, not the path
        assert_eq!(f.file.as_deref(), Some("Outer.kt"));
        assert_eq!(f.line, Some(7));
    }

    #[test]
    fn native_and_unknown_have_no_location() {
        assert_eq!(parse_frame("at dalvik.system.VMStack.getThreadStackTrace(Native Method)").unwrap().file, None);
        assert_eq!(parse_frame("at a.b.c(Unknown Source)").unwrap().file, None);
        assert_eq!(parse_frame("at a.b.c(SourceFile)").unwrap().file, None);
    }

    #[test]
    fn non_frame_lines_return_none() {
        assert!(parse_frame("Caused by: java.lang.IllegalStateException: x").is_none());
        assert!(parse_frame("... 12 more").is_none());
    }

    // ---- Resolver ----

    fn tree() -> TempDir {
        let dir = TempDir::new().unwrap();
        let r = dir.path();
        touch(r, "app/src/main/kotlin/com/example/app/MainActivity.kt");
        touch(r, "app/src/main/kotlin/com/example/app/data/AppDatabase.kt");
        touch(r, "core/src/main/java/com/example/app/Helper.kt"); // .kt under java/
        touch(r, "app/src/main/kotlin/com/example/app/MainKt.kt"); // top-level funcs file
        // duplicate filename across two modules (same package)
        touch(r, "app/src/main/kotlin/com/example/app/Dup.kt");
        touch(r, "lib/src/main/kotlin/com/example/app/Dup.kt");
        // generated output that must be ignored
        touch(r, "app/build/generated/source/kapt/com/example/app/Gen.kt");
        dir
    }

    #[test]
    fn resolves_app_frame_to_relative_path() {
        let dir = tree();
        let res = Resolver::new(dir.path().to_path_buf());
        assert_eq!(
            res.resolve("com/example/app", "MainActivity.kt").as_deref(),
            Some("app/src/main/kotlin/com/example/app/MainActivity.kt")
        );
        assert_eq!(
            res.resolve("com/example/app/data", "AppDatabase.kt").as_deref(),
            Some("app/src/main/kotlin/com/example/app/data/AppDatabase.kt")
        );
    }

    #[test]
    fn resolves_kotlin_file_under_java_source_root() {
        let dir = tree();
        let res = Resolver::new(dir.path().to_path_buf());
        assert_eq!(
            res.resolve("com/example/app", "Helper.kt").as_deref(),
            Some("core/src/main/java/com/example/app/Helper.kt")
        );
    }

    #[test]
    fn top_level_function_file_resolves_by_embedded_filename() {
        let dir = tree();
        let res = Resolver::new(dir.path().to_path_buf());
        // Frame class is `MainKt` but the file is `MainKt.kt`; we use the filename.
        assert_eq!(
            res.resolve("com/example/app", "MainKt.kt").as_deref(),
            Some("app/src/main/kotlin/com/example/app/MainKt.kt")
        );
    }

    #[test]
    fn duplicate_filename_resolves_deterministically() {
        let dir = tree();
        let res = Resolver::new(dir.path().to_path_buf());
        // Both app/ and lib/ have it; tie-break (src/main, then lexical) picks app/.
        assert_eq!(
            res.resolve("com/example/app", "Dup.kt").as_deref(),
            Some("app/src/main/kotlin/com/example/app/Dup.kt")
        );
    }

    #[test]
    fn framework_frame_does_not_resolve() {
        let dir = tree();
        let res = Resolver::new(dir.path().to_path_buf());
        assert_eq!(res.resolve("android/app", "Activity.java"), None);
    }

    #[test]
    fn generated_build_output_is_not_resolved() {
        let dir = tree();
        let res = Resolver::new(dir.path().to_path_buf());
        assert_eq!(res.resolve("com/example/app", "Gen.kt"), None);
    }

    #[test]
    fn missing_file_in_known_package_does_not_resolve() {
        let dir = tree();
        let res = Resolver::new(dir.path().to_path_buf());
        assert_eq!(res.resolve("com/example/app", "Nope.kt"), None);
    }

    #[test]
    fn disabled_resolver_resolves_nothing() {
        let res = Resolver::disabled();
        assert_eq!(res.resolve("com/example/app", "MainActivity.kt"), None);
    }
}
