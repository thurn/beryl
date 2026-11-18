//! The Windows half of `service`: a scheduled task that starts the daemon when the user logs
//! on, and restarts it if it fails, registered with `schtasks` from a task definition in XML.
//!
//! The task runs as the user who installs it, without elevation, only while they are logged
//! on. The XML is needed because `schtasks /Create /SC ONLOGON` registers a task for every
//! user, which needs an administrator, and has no restart setting.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use super::Service;
use crate::Native;

/// The scheduled task's name.
const TASK: &str = "ignite.daemon";

impl Service for Native {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        let user = current_user()?;
        let definition = task_xml(&user, &executable.to_string_lossy(), args);
        let path = std::env::temp_dir().join(format!("{TASK}-{}.xml", std::process::id()));
        // `schtasks` reads a definition in UTF-16 with a byte order mark.
        let bytes: Vec<u8> = std::iter::once(0xFEFF)
            .chain(definition.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        fs::write(&path, bytes)?;
        // `/F` replaces an earlier registration.
        let path_text = path.to_string_lossy().into_owned();
        let created = schtasks(&["/Create", "/TN", TASK, "/XML", &path_text, "/F"]);
        let _ = fs::remove_file(&path);
        check("create", &created?)?;
        // Start it now, as logging on would.
        check("run", &schtasks(&["/Run", "/TN", TASK])?)
    }

    fn uninstall(&self) -> io::Result<()> {
        if !self.is_installed()? {
            return Ok(());
        }
        // Stop a running daemon first; it fails harmlessly when none is running.
        let _ = schtasks(&["/End", "/TN", TASK]);
        check("delete", &schtasks(&["/Delete", "/TN", TASK, "/F"])?)
    }

    fn is_installed(&self) -> io::Result<bool> {
        Ok(schtasks(&["/Query", "/TN", TASK])?.status.success())
    }
}

fn schtasks(args: &[&str]) -> io::Result<Output> {
    Command::new("schtasks")
        .args(args)
        .stdin(Stdio::null())
        .output()
}

fn check(step: &str, output: &Output) -> io::Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "schtasks could not {step} the task {TASK}: {}",
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

/// The task definition: a logon trigger and a principal for `user`, restarting every minute
/// after a failure, with no time limit.
fn task_xml(user: &str, executable: &str, args: &[String]) -> String {
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
    <Description>Starts the Ignite daemon at logon.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
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
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>999</Count>
    </RestartOnFailure>
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
        );
        assert!(xml.contains("<LogonTrigger>"));
        assert_eq!(xml.matches(r"<UserId>CYGNUS\me</UserId>").count(), 2);
        assert!(xml.contains(r"<Command>C:\ignite\bin\ignite.exe</Command>"));
        assert!(xml.contains("<Arguments>daemon &quot;a &amp; b&quot;</Arguments>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
    }
}
