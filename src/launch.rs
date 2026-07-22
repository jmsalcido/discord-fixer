//! Starting Discord back up.

use crate::discord::{Install, LaunchSpec};
use anyhow::{Context, Result, anyhow};
use std::process::{Command, Stdio};

pub fn launch(install: &Install) -> Result<()> {
    let spec = install
        .launch
        .as_ref()
        .ok_or_else(|| anyhow!("couldn't work out how to start {}", install.label()))?;

    // Some of these are stubs that hand off and exit straight away, so we can
    // wait on them and report a real failure. Others *are* Discord, and waiting
    // would block until the user quits it.
    let (mut cmd, hands_off) = match spec {
        LaunchSpec::MacOsBundleId(bundle_id) => {
            let mut cmd = Command::new("open");
            cmd.arg("-b").arg(bundle_id);
            (cmd, true)
        }
        // Squirrel's stub. Launching `app-x.y.z\Discord.exe` directly works
        // today and silently breaks the auto-updater tomorrow.
        LaunchSpec::WindowsSquirrel {
            update_exe,
            exe_name,
        } => {
            let mut cmd = Command::new(update_exe);
            cmd.arg("--processStart").arg(exe_name);
            (cmd, true)
        }
        LaunchSpec::Command { program, args } => {
            let mut cmd = Command::new(program);
            cmd.args(args);
            (cmd, false)
        }
    };

    // Discord outlives this app, and we want none of its output on our streams.
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if hands_off {
        let status = cmd
            .status()
            .with_context(|| format!("failed to start {}", install.label()))?;
        if !status.success() {
            return Err(anyhow!("{} failed to start ({status})", install.label()));
        }
    } else {
        cmd.spawn()
            .with_context(|| format!("failed to start {}", install.label()))?;
    }

    Ok(())
}
