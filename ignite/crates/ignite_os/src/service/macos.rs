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

/// The installer's environment variables that the agent keeps. launchd starts an agent with only
/// `/usr/bin:/bin:/usr/sbin:/sbin` on its `PATH`, where the daemon finds no `cargo`, `just`,
/// `node` or Homebrew tools, and with none of the `IGNITE_*` settings, so the hub's machine
/// would not serve the hub. `IGNITE_SESSION` and `IGNITE_TOKEN` belong to one session and stay
/// out.
const KEPT_VARIABLES: &[&str] = &["PATH", "IGNITE_HOME", "IGNITE_HUB_LISTEN"];

impl Service for Native {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        let path = plist_path()?;
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
            plist(&program, &environment, &logs.join(format!("{LABEL}.log"))),
        )?;
        // Unload an earlier registration first; it fails harmlessly when there is none.
        let _ = launchctl(&["bootout", &format!("{}/{LABEL}", domain())]);
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

    fn uninstall(&self) -> io::Result<()> {
        let _ = launchctl(&["bootout", &format!("{}/{LABEL}", domain())]);
        match fs::remove_file(plist_path()?) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn is_installed(&self) -> io::Result<bool> {
        Ok(plist_path()?.exists())
    }
}

fn home() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

fn plist_path() -> io::Result<PathBuf> {
    Ok(home()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

/// The user's login session, where agents that start at login are loaded.
fn domain() -> String {
    // SAFETY: `getuid` takes no arguments and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("/bin/launchctl").args(args).output()
}

/// The agent's property list: run `program` with `environment` at load and login, and again
/// whenever it exits.
fn plist(program: &[String], environment: &[(String, String)], log: &Path) -> String {
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
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}  </array>
  <key>EnvironmentVariables</key>
  <dict>
{variables}  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
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
        let text = plist(&program, &environment, Path::new("/tmp/ignite.log"));
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
}
