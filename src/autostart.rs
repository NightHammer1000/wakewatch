//! Autostart via a Scheduled Task.
//!
//! WakeWatch needs administrator rights to read the power request list, so a
//! Run-key entry would fire a UAC prompt at every logon. A scheduled task with
//! "run with highest privileges" starts silently instead.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub const TASK_NAME: &str = "WakeWatch";

/// Keeps schtasks from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn schtasks(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("schtasks")
        .args(args)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

pub fn is_enabled() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn enable(exe: &Path) -> Result<(), String> {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let target = format!("\"{}\"", exe.display());

    let mut args = vec![
        "/Create", "/TN", TASK_NAME, "/SC", "ONLOGON", "/RL", "HIGHEST", "/F", "/TR", &target,
    ];
    if !user.is_empty() {
        args.push("/RU");
        args.push(&user);
    }

    let out = schtasks(&args).map_err(|e| format!("could not run schtasks: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(describe_failure(&out))
    }
}

pub fn disable() -> Result<(), String> {
    let out = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])
        .map_err(|e| format!("could not run schtasks: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(describe_failure(&out))
    }
}

fn describe_failure(out: &std::process::Output) -> String {
    // schtasks is localized, so surface whatever it said rather than guessing.
    let err = String::from_utf8_lossy(&out.stderr);
    let msg = if err.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        err.trim().to_string()
    };
    if msg.is_empty() {
        format!("schtasks failed with {}", out.status)
    } else {
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn querying_a_missing_task_is_false_not_a_panic() {
        // The real task may or may not exist; either way this must not panic.
        let _ = is_enabled();
    }

    #[test]
    fn failure_description_is_never_empty() {
        let out = schtasks(&["/Query", "/TN", "WakeWatch-does-not-exist-xyz"])
            .expect("schtasks should be present on Windows");
        assert!(!out.status.success());
        assert!(!describe_failure(&out).is_empty());
    }
}
