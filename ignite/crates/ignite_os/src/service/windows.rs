//! The Windows half of `service`: a scheduled task that starts the daemon when the user logs
//! on, and restarts it if it fails, registered with `schtasks` from a task definition in XML.
//!
//! The monitor is a second task, `ignite.monitor`, that runs at logon and then every
//! [`MONITOR_INTERVAL_S`] seconds and is not restarted when it exits. On the hub's machine the
//! hub is a third, `ignite.hub`, restarted like the daemon. A scheduled task carries no
//! environment variables of its own, so the hub's listen addresses must be set in the user's
//! environment, which the task inherits; no Windows machine serves the hub today.
//!
//! The task runs as the user who installs it, without elevation, only while they are logged
//! on. The XML is needed because `schtasks /Create /SC ONLOGON` registers a task for every
//! user, which needs an administrator, and has no restart setting.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use super::{MONITOR_INTERVAL_S, Service};
use crate::Native;

/// The scheduled task's name.
const TASK: &str = "ignite.daemon";
/// The monitor task's name.
const MONITOR_TASK: &str = "ignite.monitor";
/// The hub task's name, on the hub's machine only.
const HUB_TASK: &str = "ignite.hub";

/// How the scheduler runs a task.
#[derive(Clone, Copy)]
enum Schedule {
    /// At logon, and again a minute after each failure.
    KeepAlive,
    /// At logon, and then every this many seconds, without restarting after a failure.
    Interval(u32),
}

impl Service for Native {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        register(TASK, Schedule::KeepAlive, executable, args)
    }

    fn uninstall(&self) -> io::Result<()> {
        unregister(MONITOR_TASK)?;
        unregister(HUB_TASK)?;
        unregister(TASK)
    }

    fn is_installed(&self) -> io::Result<bool> {
        registered(TASK)
    }

    fn install_monitor(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        register(
            MONITOR_TASK,
            Schedule::Interval(MONITOR_INTERVAL_S),
            executable,
            args,
        )
    }

    fn daemon_running(&self) -> io::Result<bool> {
        // `/FO CSV /NH` prints `"ignite.daemon","<next run>","Running"`, or `Ready` when the
        // task is not running.
        let output = schtasks(&["/Query", "/TN", TASK, "/FO", "CSV", "/NH"])?;
        if !output.status.success() {
            return Ok(false);
        }
        Ok(String::from_utf8_lossy(&output.stdout).contains("\"Running\""))
    }

    fn kickstart_daemon(&self) -> io::Result<()> {
        kickstart(TASK)
    }

    fn install_hub(
        &self,
        executable: &Path,
        args: &[String],
        environment: &[(String, String)],
    ) -> io::Result<()> {
        // The task inherits the user's environment and can set none of its own, so a listen
        // address that only this process has would be lost.
        for (name, value) in environment {
            if std::env::var(name).ok().as_deref() != Some(value.as_str()) {
                return Err(io::Error::other(format!(
                    "set {name}={value} in your user environment first: the hub's scheduled \
                     task can only inherit it"
                )));
            }
        }
        register(HUB_TASK, Schedule::KeepAlive, executable, args)
    }

    fn hub_installed(&self) -> io::Result<bool> {
        registered(HUB_TASK)
    }

    fn kickstart_hub(&self) -> io::Result<()> {
        kickstart(HUB_TASK)
    }
}

/// Ends the task `task` and runs it again.
fn kickstart(task: &str) -> io::Result<()> {
    // Ending a task that is not running fails harmlessly.
    let _ = schtasks(&["/End", "/TN", task]);
    check(task, "run", &schtasks(&["/Run", "/TN", task])?)
}

/// Registers the scheduled task `task` and starts it now, as logging on would.
fn register(task: &str, schedule: Schedule, executable: &Path, args: &[String]) -> io::Result<()> {
    let user = current_user()?;
    let definition = task_xml(&user, &executable.to_string_lossy(), args, schedule);
    let path = std::env::temp_dir().join(format!("{task}-{}.xml", std::process::id()));
    // `schtasks` reads a definition in UTF-16 with a byte order mark.
    let bytes: Vec<u8> = std::iter::once(0xFEFF)
        .chain(definition.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    fs::write(&path, bytes)?;
    // `/F` replaces an earlier registration.
    let path_text = path.to_string_lossy().into_owned();
    let created = schtasks(&["/Create", "/TN", task, "/XML", &path_text, "/F"]);
    let _ = fs::remove_file(&path);
    check(task, "create", &created?)?;
    check(task, "run", &schtasks(&["/Run", "/TN", task])?)
}

/// Stops and deletes the task `task`, if it is registered.
fn unregister(task: &str) -> io::Result<()> {
    if !registered(task)? {
        return Ok(());
    }
    // Stop a running task first; it fails harmlessly when none is running.
    let _ = schtasks(&["/End", "/TN", task]);
    check(task, "delete", &schtasks(&["/Delete", "/TN", task, "/F"])?)
}

fn registered(task: &str) -> io::Result<bool> {
    Ok(schtasks(&["/Query", "/TN", task])?.status.success())
}

fn schtasks(args: &[&str]) -> io::Result<Output> {
    Command::new("schtasks")
        .args(args)
        .stdin(Stdio::null())
        .output()
}

fn check(task: &str, step: &str, output: &Output) -> io::Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "schtasks could not {step} the task {task}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// The current user as `DOMAIN\name`.
fn current_user() -> io::Result<String> {
    let name = std::env::var("USERNAME")
        .map_err(|_| io::Error::other("USERNAME is not set, so the user is unknown"))?;
    Ok(match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{name}"),
        _ => name,
    })
}

/// The task definition: a logon trigger and a principal for `user`, with no time limit. A
/// [`Schedule::Interval`] task also repeats forever at its interval; a [`Schedule::KeepAlive`]
/// task restarts every minute after a failure.
fn task_xml(user: &str, executable: &str, args: &[String], schedule: Schedule) -> String {
    let (repetition, restart, description) = match schedule {
        Schedule::KeepAlive => (
            String::new(),
            "<RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>999</Count>\n    \
             </RestartOnFailure>"
                .to_owned(),
            "Starts the Ignite daemon at logon.",
        ),
        Schedule::Interval(seconds) => (
            format!(
                "<Repetition>\n        <Interval>PT{seconds}S</Interval>\n        \
                 <StopAtDurationEnd>false</StopAtDurationEnd>\n      </Repetition>\n      "
            ),
            String::new(),
            "Checks the Ignite daemon at logon and every two minutes.",
        ),
    };
    let user = escape(user);
    let arguments = args
        .iter()
        .map(|arg| quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let mut xml = String::new();
    let _ = write!(
        xml,
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>{description}</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      {repetition}<Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    {restart}
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>
      <Arguments>{}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        escape(executable),
        escape(&arguments),
    );
    xml
}

/// `text` with the characters XML reserves replaced by entities.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// `arg` quoted so that a program's command-line parser reads it back as one argument: inside
/// quotes, a quote is escaped with a backslash, and the backslashes before a quote are doubled.
fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_only_when_needed() {
        assert_eq!(quote("daemon"), "daemon");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote(r"C:\Program Files\x"), r#""C:\Program Files\x""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"dir with space\"), r#""dir with space\\""#);
    }

    #[test]
    fn the_task_runs_the_executable_for_the_user_at_logon() {
        let xml = task_xml(
            r"CYGNUS\me",
            r"C:\ignite\bin\ignite.exe",
            &["daemon".to_owned(), "a & b".to_owned()],
            Schedule::KeepAlive,
        );
        assert!(xml.contains("<LogonTrigger>"));
        assert_eq!(xml.matches(r"<UserId>CYGNUS\me</UserId>").count(), 2);
        assert!(xml.contains(r"<Command>C:\ignite\bin\ignite.exe</Command>"));
        assert!(xml.contains("<Arguments>daemon &quot;a &amp; b&quot;</Arguments>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
    }

    #[test]
    fn the_monitor_task_repeats_and_is_not_restarted() {
        let xml = task_xml(
            r"CYGNUS\me",
            r"C:\ignite\ignite.exe",
            &[],
            Schedule::Interval(120),
        );
        assert!(xml.contains("<Interval>PT120S</Interval>"));
        assert!(xml.contains("<StopAtDurationEnd>false</StopAtDurationEnd>"));
        assert!(!xml.contains("RestartOnFailure"));
    }
}
