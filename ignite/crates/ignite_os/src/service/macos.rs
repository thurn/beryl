//! The macOS half of `service`: a launchd agent in `~/Library/LaunchAgents`, loaded into the
//! user's login session, that starts the daemon at login and restarts it if it exits.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Service;
use crate::Native;

/// The agent's launchd label, which also names its property list file.
const LABEL: &str = "ignite.daemon";
/// The monitor agent's launchd label.
const MONITOR_LABEL: &str = "ignite.monitor";

/// How launchd runs an agent.
#[derive(Clone, Copy)]
enum Launch {
    /// At load and login, and again whenever it exits.
    KeepAlive,
    /// At load and login, and then every this many seconds, whether or not the last run is
    /// finished.
    Interval(u32),
}

/// The installer's environment variables that the agent keeps. launchd starts an agent with only
/// `/usr/bin:/bin:/usr/sbin:/sbin` on its `PATH`, where the daemon finds no `cargo`, `just`,
/// `node` or Homebrew tools, and with none of the `IGNITE_*` settings, so the hub's machine
/// would not serve the hub. `IGNITE_SESSION` and `IGNITE_TOKEN` belong to one session and stay
/// out.
const KEPT_VARIABLES: &[&str] = &["PATH", "IGNITE_HOME", "IGNITE_HUB_LISTEN"];

/// Writes the agent `label`'s property list and loads it, replacing an earlier registration.
fn install_agent(
    label: &str,
    launch: Launch,
    executable: &Path,
    args: &[String],
) -> io::Result<()> {
    {
        let path = plist_path(label)?;
        let logs = home()?.join("Library").join("Logs");
        fs::create_dir_all(path.parent().expect("the agent's path has a folder"))?;
        fs::create_dir_all(&logs)?;
        let mut program = vec![executable.to_string_lossy().into_owned()];
        program.extend(args.iter().cloned());
        let environment: Vec<(String, String)> = KEPT_VARIABLES
            .iter()
            .filter_map(|name| Some(((*name).to_owned(), std::env::var(name).ok()?)))
            .collect();
        fs::write(
            &path,
            plist(
                label,
                launch,
                &program,
                &environment,
                &logs.join(format!("{label}.log")),
            ),
        )?;
        // Unload an earlier registration first; it fails harmlessly when there is none.
        let _ = launchctl(&["bootout", &format!("{}/{label}", domain())]);
        let loaded = launchctl(&["bootstrap", &domain(), &path.to_string_lossy()])?;
        if loaded.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "launchctl bootstrap {}: {}",
                path.display(),
                String::from_utf8_lossy(&loaded.stderr).trim()
            )))
        }
    }
}

fn uninstall_agent(label: &str) -> io::Result<()> {
    let _ = launchctl(&["bootout", &format!("{}/{label}", domain())]);
    match fs::remove_file(plist_path(label)?) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

impl Service for Native {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        install_agent(LABEL, Launch::KeepAlive, executable, args)
    }

    fn uninstall(&self) -> io::Result<()> {
        uninstall_agent(MONITOR_LABEL)?;
        uninstall_agent(LABEL)
    }

    fn is_installed(&self) -> io::Result<bool> {
        Ok(plist_path(LABEL)?.exists())
    }

    fn install_monitor(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        install_agent(
            MONITOR_LABEL,
            Launch::Interval(super::MONITOR_INTERVAL_S),
            executable,
            args,
        )
    }

    fn daemon_running(&self) -> io::Result<bool> {
        let listed = launchctl(&["list", LABEL])?;
        // `launchctl list <label>` prints a `"PID" = n;` line only while the job runs.
        Ok(listed.status.success() && String::from_utf8_lossy(&listed.stdout).contains("\"PID\""))
    }

    fn kickstart_daemon(&self) -> io::Result<()> {
        let kicked = launchctl(&["kickstart", "-k", &format!("{}/{LABEL}", domain())])?;
        if kicked.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "launchctl kickstart: {}",
                String::from_utf8_lossy(&kicked.stderr).trim()
            )))
        }
    }
}

fn home() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

fn plist_path(label: &str) -> io::Result<PathBuf> {
    Ok(home()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{label}.plist")))
}

/// The user's login session, where agents that start at login are loaded.
fn domain() -> String {
    // SAFETY: `getuid` takes no arguments and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("/bin/launchctl").args(args).output()
}

/// The agent's property list: run `program` with `environment` at load and login, and then as
/// `launch` says.
fn plist(
    label: &str,
    launch: Launch,
    program: &[String],
    environment: &[(String, String)],
    log: &Path,
) -> String {
    let schedule = match launch {
        Launch::KeepAlive => "  <key>KeepAlive</key>\n  <true/>\n".to_owned(),
        Launch::Interval(seconds) => {
            format!("  <key>StartInterval</key>\n  <integer>{seconds}</integer>\n")
        }
    };
    let mut arguments = String::new();
    for argument in program {
        let _ = writeln!(arguments, "    <string>{}</string>", escape(argument));
    }
    let mut variables = String::new();
    for (name, value) in environment {
        let _ = writeln!(
            variables,
            "    <key>{}</key>\n    <string>{}</string>",
            escape(name),
            escape(value)
        );
    }
    let log = escape(&log.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}  </array>
  <key>EnvironmentVariables</key>
  <dict>
{variables}  </dict>
  <key>RunAtLoad</key>
  <true/>
{schedule}  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_runs_the_program_and_is_a_valid_property_list() {
        let program = [
            "/opt/ignite/bin/ignite".to_owned(),
            "daemon".to_owned(),
            "a<&b>".to_owned(),
        ];
        let environment = [
            ("PATH".to_owned(), "/opt/homebrew/bin:/usr/bin".to_owned()),
            ("IGNITE_HUB_LISTEN".to_owned(), "127.0.0.1:7345".to_owned()),
        ];
        let text = plist(
            LABEL,
            Launch::KeepAlive,
            &program,
            &environment,
            Path::new("/tmp/ignite.log"),
        );
        assert!(text.contains("<key>KeepAlive</key>"));
        assert!(text.contains("<string>daemon</string>"));
        assert!(text.contains("<string>a&lt;&amp;b&gt;</string>"));

        let path = std::env::temp_dir().join(format!("ignite-{}.plist", std::process::id()));
        fs::write(&path, &text).unwrap();
        let lint = Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&path)
            .output()
            .unwrap();
        let converted = Command::new("/usr/bin/plutil")
            .args(["-extract", "ProgramArguments.2", "raw", "-o", "-"])
            .arg(&path)
            .output()
            .unwrap();
        let listen = Command::new("/usr/bin/plutil")
            .args([
                "-extract",
                "EnvironmentVariables.IGNITE_HUB_LISTEN",
                "raw",
                "-o",
                "-",
            ])
            .arg(&path)
            .output()
            .unwrap();
        fs::remove_file(&path).unwrap();
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stdout)
        );
        assert_eq!(String::from_utf8_lossy(&converted.stdout).trim(), "a<&b>");
        assert_eq!(
            String::from_utf8_lossy(&listen.stdout).trim(),
            "127.0.0.1:7345"
        );
    }

    #[test]
    fn the_monitor_agent_runs_on_an_interval_and_is_not_kept_alive() {
        let program = ["/opt/ignite/bin/last-good".to_owned(), "monitor".to_owned()];
        let text = plist(
            MONITOR_LABEL,
            Launch::Interval(120),
            &program,
            &[],
            Path::new("/tmp/monitor.log"),
        );
        assert!(text.contains("<string>ignite.monitor</string>"));
        assert!(text.contains("<key>StartInterval</key>\n  <integer>120</integer>"));
        assert!(text.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(!text.contains("KeepAlive"));
        let path = std::env::temp_dir().join(format!("ignite-mon-{}.plist", std::process::id()));
        fs::write(&path, &text).unwrap();
        let lint = Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&path)
            .output()
            .unwrap();
        fs::remove_file(&path).unwrap();
        assert!(lint.status.success());
    }
}
