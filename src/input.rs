//! Input sources: read a piped stdin, or spawn `adb logcat` and own its lifecycle.
//!
//! The source is chosen once in `Config` (pidcat-style: a piped stdin is read
//! directly, otherwise we run adb). Both paths yield the same thing — an iterator
//! of `io::Result<String>` lines — so the rest of the pipeline never knows which
//! source it is draining.
//!
//! Process hygiene: `std::process::Child` does NOT kill the child on drop, and a
//! bare Ctrl-C would orphan `adb logcat` (leaving a logcat reader open against the
//! device). We therefore (1) put adb in its own process group and kill the group
//! from the signal handler, and (2) kill+reap on `Drop` to cover every other exit
//! path.

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::Context;

use crate::config::Config;

/// Process-group id of the spawned adb, published so the Ctrl-C handler can kill the
/// whole group without threading a handle through the call stack. 0 = nothing spawned.
static CHILD_PGID: AtomicI32 = AtomicI32::new(0);

/// Kill the spawned adb process group, if any. Safe to call when nothing was spawned.
/// Invoked from the Ctrl-C handler, where `Drop` will not run.
pub fn kill_child_group() {
    let pgid = CHILD_PGID.load(Ordering::SeqCst);
    if pgid > 0 {
        #[cfg(unix)]
        // SAFETY: killpg with a valid pgid; SIGTERM lets adb exit cleanly.
        unsafe {
            libc::killpg(pgid, libc::SIGTERM);
        }
    }
}

/// Build the line source dictated by `Config`.
pub fn make_source(cfg: &Config) -> anyhow::Result<Box<dyn Iterator<Item = io::Result<String>>>> {
    if cfg.read_stdin {
        Ok(Box::new(BufReader::new(io::stdin()).lines()))
    } else {
        Ok(Box::new(spawn_adb(cfg)?))
    }
}

fn spawn_adb(cfg: &Config) -> anyhow::Result<AdbLines> {
    // Clearing the buffer is a separate one-shot invocation that runs and exits.
    if cfg.clear {
        let mut clear = base_command(cfg);
        clear
            .arg("logcat")
            .arg("-c")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // A failure here (e.g. nothing to clear) should not abort the tail.
        let _ = clear.status();
    }

    let mut cmd = base_command(cfg);
    cmd.arg("logcat").arg("-v").arg("threadtime");
    if cfg.crash {
        // Focus on the crash buffer (uncaught fatal exceptions + wtf).
        cmd.arg("-b").arg("crash");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setpgid(0, 0) is async-signal-safe; it places the child in its own
        // process group so the Ctrl-C handler can kill the whole group.
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
    }

    let mut child = cmd
        .spawn()
        .context("failed to spawn `adb logcat` — is `adb` on your PATH?")?;

    #[cfg(unix)]
    CHILD_PGID.store(child.id() as i32, Ordering::SeqCst);

    // Drain adb's stderr on its own thread so the pipe never fills (which would
    // deadlock adb) and so device warnings ("waiting for device") surface promptly.
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(l) => eprintln!("adb: {l}"),
                    Err(_) => break,
                }
            }
        });
    }

    let stdout = child.stdout.take().expect("stdout was piped");
    Ok(AdbLines {
        child,
        lines: BufReader::new(stdout).lines(),
    })
}

fn base_command(cfg: &Config) -> Command {
    let mut cmd = Command::new("adb");
    if let Some(serial) = &cfg.serial {
        cmd.arg("-s").arg(serial);
    }
    cmd
}

/// Seed the followed PID set for an already-running app via `adb shell pidof <pkg>`.
/// Best-effort: a missing/failed `pidof` just yields no seed (ActivityManager lines
/// then pick the app up on its next (re)launch).
pub fn seed_pids(cfg: &Config) -> HashSet<u32> {
    let mut pids = HashSet::new();
    for pkg in &cfg.packages {
        let mut cmd = base_command(cfg);
        cmd.arg("shell").arg("pidof").arg(pkg);
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                for tok in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                    if let Ok(pid) = tok.parse() {
                        pids.insert(pid);
                    }
                }
            }
        }
    }
    pids
}

/// Owning iterator over `adb logcat` stdout that kills+reaps the child when dropped.
struct AdbLines {
    child: Child,
    lines: io::Lines<BufReader<ChildStdout>>,
}

impl Iterator for AdbLines {
    type Item = io::Result<String>;
    fn next(&mut self) -> Option<Self::Item> {
        self.lines.next()
    }
}

impl Drop for AdbLines {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        CHILD_PGID.store(0, Ordering::SeqCst);
    }
}
