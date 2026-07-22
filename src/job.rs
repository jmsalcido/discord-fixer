//! The fix itself: stop Discord, delete its caches, start it again.
//!
//! Shared by the GUI and the CLI so there is exactly one description of what
//! "fixing Discord" means. Progress is reported through a callback, which the
//! GUI funnels into a channel and the CLI prints directly.

use crate::clean::{self, Report};
use crate::discord::{Install, Scope};
use crate::kill::{self, KillReport};
use crate::launch;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub scope: Scope,
    pub relaunch: bool,
    pub dry_run: bool,
}

/// A human-readable progress line.
#[derive(Debug, Clone)]
pub struct Step(pub String);

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub report: Report,
    /// Things that didn't go to plan but didn't stop the fix either.
    pub warnings: Vec<String>,
    pub elapsed: Duration,
    pub relaunched: bool,
}

impl Summary {
    pub fn bytes_freed(&self) -> u64 {
        self.report.bytes_freed()
    }

    /// A one-line result, which is all most users will read.
    pub fn headline(&self, dry_run: bool) -> String {
        let size = clean::format_bytes(self.bytes_freed());
        let count = self.report.removed_count();
        if dry_run {
            format!("Would clear {count} items ({size})")
        } else if count == 0 {
            "Nothing to clear — Discord's caches were already empty".to_string()
        } else {
            format!("Cleared {count} items, freeing {size}")
        }
    }
}

pub fn run(installs: &[Install], opts: Options, mut emit: impl FnMut(Step)) -> Summary {
    let started = Instant::now();
    let mut summary = Summary::default();

    for install in installs {
        let label = install.label();

        emit(Step(format!("Stopping {label}…")));
        let kill = kill::stop(install, opts.dry_run);
        emit(Step(describe_kill(&label, &kill)));
        if !kill.all_stopped() {
            summary.warnings.push(format!(
                "{} process(es) from {label} wouldn't stop; some files may be locked",
                kill.survivors
            ));
        }

        let what = match opts.scope {
            Scope::Safe => "caches",
            Scope::Deep => "caches and stored modules",
        };
        emit(Step(format!("Clearing {label} {what}…")));
        let report = clean::clean(install, opts.scope, opts.dry_run);

        for problem in report.problems() {
            summary.warnings.push(format!(
                "{}: {}",
                problem.path.display(),
                describe(&problem.outcome)
            ));
        }
        emit(Step(format!(
            "Cleared {} ({} items)",
            clean::format_bytes(report.bytes_freed()),
            report.removed_count()
        )));
        summary.report.merge(report);

        // Only reopen what we closed. Relaunching a Discord the user had
        // deliberately quit would be presumptuous.
        if opts.relaunch && kill.was_running && !opts.dry_run {
            emit(Step(format!("Reopening {label}…")));
            match launch::launch(install) {
                Ok(()) => summary.relaunched = true,
                Err(e) => summary
                    .warnings
                    .push(format!("Couldn't reopen {label}: {e}")),
            }
        }
    }

    summary.elapsed = started.elapsed();
    summary
}

fn describe_kill(label: &str, kill: &KillReport) -> String {
    if !kill.was_running {
        return format!("{label} wasn't running");
    }
    let total = kill.stopped_gracefully + kill.force_killed;
    if kill.force_killed > 0 {
        format!(
            "Stopped {total} {label} process(es) ({} forced)",
            kill.force_killed
        )
    } else {
        format!("Stopped {total} {label} process(es)")
    }
}

fn describe(outcome: &clean::Outcome) -> String {
    match outcome {
        clean::Outcome::Refused(why) => format!("skipped — {why}"),
        clean::Outcome::Failed(why) => format!("couldn't delete — {why}"),
        clean::Outcome::Removed { bytes } => format!("removed {}", clean::format_bytes(*bytes)),
        clean::Outcome::NotPresent => "not present".to_string(),
    }
}
