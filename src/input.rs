//! Input sources: read a piped stdin, or spawn `adb logcat` and own its lifecycle.
//!
//! The source is chosen once in `Config` (pidcat-style: a piped stdin is read
//! directly, otherwise we run adb). The stdin path is a plain blocking line iterator;
//! the live path delivers lines over a bounded channel from a reader thread, so the
//! main loop can `recv_timeout` and flush pending traces/dedup-runs on idle (a live
//! tail has no EOF, and a crash is usually the last thing emitted before a process
//! goes quiet).
//!
//! Process hygiene: `std::process::Child` does NOT kill the child on drop, and a bare
//! Ctrl-C would orphan `adb logcat`. We put adb in its own process group and kill the
//! group from the signal handler, and kill+reap on `Drop` for every other exit path.

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError};
use std::time::Duration;

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

/// Blocking line iterator over stdin (the piped-input path).
pub fn stdin_lines() -> impl Iterator<Item = io::Result<String>> {
    BufReader::new(io::stdin()).lines()
}

/// A live `adb logcat` stream: lines arrive over a bounded channel from a reader
/// thread; the child is killed+reaped on drop.
pub struct AdbStream {
    rx: Receiver<io::Result<String>>,
    child: Child,
}

impl AdbStream {
    /// Receive the next line, or time out so the caller can flush on idle.
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<io::Result<String>, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

impl Drop for AdbStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        CHILD_PGID.store(0, Ordering::SeqCst);
    }
}

pub fn spawn_adb_stream(cfg: &Config) -> anyhow::Result<AdbStream> {
    // Clearing the buffer is a separate one-shot invocation that runs and exits.
    if cfg.clear {
        let mut clear = base_command(cfg);
        clear
            .arg("logcat")
            .arg("-c")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
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
    // deadlock adb) and device warnings ("waiting for device") surface promptly.
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
    // Bounded so a flood backpressures the reader (and thus adb) instead of growing
    // memory without bound.
    let (tx, rx) = sync_channel::<io::Result<String>>(1024);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line).is_err() {
                break; // receiver gone — main loop exited
            }
        }
        // tx dropped here -> rx disconnects, signalling EOF / child exit.
    });

    Ok(AdbStream { rx, child })
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
