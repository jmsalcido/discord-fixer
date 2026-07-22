//! Stopping Discord before we delete anything underneath it.
//!
//! Matching is on the executable path only — never the command line. The shell
//! script this app replaces used `pkill -f Discord`, which matches any process
//! whose *arguments* mention Discord: an editor with a Discord path open, a
//! build running in a directory called `discord-fixer`, a `grep`. We enumerate
//! processes and ask the flavor whether each executable is really one of its
//! own.

use crate::discord::Install;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, Signal, System};

/// How long we let Discord shut down cleanly before insisting.
const GRACEFUL_TIMEOUT: Duration = Duration::from_secs(8);
/// And how long we wait for the kernel to reap it after SIGKILL.
const FORCE_TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Default)]
pub struct KillReport {
    pub was_running: bool,
    /// Number of processes that exited after a polite request.
    pub stopped_gracefully: usize,
    /// Number that needed to be killed outright.
    pub force_killed: usize,
    /// Still alive when we gave up. Deleting under these is likely to fail on
    /// Windows and is worth telling the user about.
    pub survivors: usize,
}

impl KillReport {
    pub fn all_stopped(&self) -> bool {
        self.survivors == 0
    }
}

/// Terminate every process belonging to `install`, escalating from SIGTERM to
/// SIGKILL, then wait for Discord's singleton lock to clear.
pub fn stop(install: &Install, dry_run: bool) -> KillReport {
    let mut sys = System::new();
    let mut report = KillReport::default();

    let initial = matching_pids(&mut sys, install);
    if initial.is_empty() {
        return report;
    }
    report.was_running = true;

    if dry_run {
        report.stopped_gracefully = initial.len();
        return report;
    }

    // Ask nicely. Discord flushes its state on SIGTERM, which is worth waiting
    // for — an unflushed profile is its own kind of corruption.
    for pid in &initial {
        if let Some(proc) = sys.process(*pid) {
            // `kill_with` returns None where the signal isn't supported
            // (Windows), in which case there is only one way to stop a process.
            if proc.kill_with(Signal::Term).is_none() {
                proc.kill();
            }
        }
    }

    let remaining = wait_until_gone(&mut sys, install, GRACEFUL_TIMEOUT);
    report.stopped_gracefully = initial.len() - remaining.len();

    if !remaining.is_empty() {
        for pid in &remaining {
            if let Some(proc) = sys.process(*pid) {
                proc.kill();
            }
        }
        let stubborn = wait_until_gone(&mut sys, install, FORCE_TIMEOUT);
        report.force_killed = remaining.len() - stubborn.len();
        report.survivors = stubborn.len();
    }

    // Electron holds `SingletonLock` for a moment after the process is gone.
    // Deleting the profile out from under a lock that's still being released is
    // how you get a half-cleaned directory.
    if report.all_stopped() {
        wait_for_lock_release(install, Duration::from_secs(3));
    }

    report
}

fn matching_pids(sys: &mut System, install: &Install) -> Vec<sysinfo::Pid> {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .filter(|(_, proc)| {
            proc.exe()
                .is_some_and(|exe| install.flavor.matches_process(exe))
        })
        .map(|(pid, _)| *pid)
        .collect()
}

/// Poll until nothing matches or `timeout` elapses; returns whatever is left.
fn wait_until_gone(sys: &mut System, install: &Install, timeout: Duration) -> Vec<sysinfo::Pid> {
    let deadline = Instant::now() + timeout;
    loop {
        let alive = matching_pids(sys, install);
        if alive.is_empty() || Instant::now() >= deadline {
            return alive;
        }
        std::thread::sleep(POLL);
    }
}

fn wait_for_lock_release(install: &Install, timeout: Duration) {
    let lock = install.data_dir.join("SingletonLock");
    let deadline = Instant::now() + timeout;
    while lock.symlink_metadata().is_ok() && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
}
