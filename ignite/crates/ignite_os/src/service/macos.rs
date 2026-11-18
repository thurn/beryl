//! The macOS half of `service`: a launchd agent in `~/Library/LaunchAgents`, loaded into the
//! user's login session, that starts the daemon at login and restarts it if it exits.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

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
    reload(&SystemLaunchctl, &domain(), label, &path)
}

/// How often and how long to wait for launchd while it replaces a job.
const POLL: Duration = Duration::from_millis(100);
/// `launchctl bootout` returns as soon as it has sent the job SIGTERM, and launchd keeps the job
/// until its process exits, which it gives 20 seconds (`ExitTimeOut`) before SIGKILL. This many
/// polls of `launchctl print` (30 seconds in all) wait for it to go.
const GONE_POLLS: u32 = 300;
/// Attempts at `launchctl bootstrap` that fail with error 5 before install gives up.
const BOOTSTRAP_ATTEMPTS: u32 = 5;

/// Runs `launchctl` and waits; the tests replace it.
trait Launchctl {
    fn run(&self, args: &[&str]) -> io::Result<Output>;
    fn sleep(&self, duration: Duration);
}

struct SystemLaunchctl;

impl Launchctl for SystemLaunchctl {
    fn run(&self, args: &[&str]) -> io::Result<Output> {
        launchctl(args)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Replaces the job `label` in `domain` with the property list at `path`, and fails unless
/// launchd has the new job loaded afterwards.
///
/// `launchctl bootout` returns while launchd is still tearing the old job down, and a
/// `launchctl bootstrap` sent in that window fails with "Bootstrap failed: 5: Input/output
/// error". So this waits until `launchctl print` no longer finds the job, and retries a
/// bootstrap that fails with error 5, waiting longer each time.
fn reload(launchctl: &dyn Launchctl, domain: &str, label: &str, path: &Path) -> io::Result<()> {
    let target = format!("{domain}/{label}");
    let path_text = path.to_string_lossy();
    // Unload an earlier registration first; it fails harmlessly when there is none.
    let _ = launchctl.run(&["bootout", &target]);
    let mut backoff = POLL * 5;
    let mut attempt = 1;
    loop {
        wait_until_gone(launchctl, &target)?;
        let loaded = launchctl.run(&["bootstrap", domain, &path_text])?;
        if loaded.status.success() {
            break;
        }
        let stderr = String::from_utf8_lossy(&loaded.stderr);
        if !is_busy(&loaded) || attempt == BOOTSTRAP_ATTEMPTS {
            return Err(io::Error::other(format!(
                "launchctl bootstrap {} failed after {attempt} attempt(s): {}",
                path.display(),
                stderr.trim()
            )));
        }
        launchctl.sleep(backoff);
        backoff *= 2;
        attempt += 1;
    }
    if is_loaded(launchctl, &target)? {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "launchctl bootstrap {} succeeded but launchd has no job {target}",
            path.display()
        )))
    }
}

/// Polls until launchd has no job `target`, for at most [`GONE_POLLS`] polls. A job still there
/// afterwards is left to the bootstrap, whose failure names it.
fn wait_until_gone(launchctl: &dyn Launchctl, target: &str) -> io::Result<()> {
    for _ in 0..GONE_POLLS {
        if !is_loaded(launchctl, target)? {
            return Ok(());
        }
        launchctl.sleep(POLL);
    }
    Ok(())
}

/// Whether launchd has the job `target` loaded: `launchctl print` fails for one it does not
/// know.
fn is_loaded(launchctl: &dyn Launchctl, target: &str) -> io::Result<bool> {
    Ok(launchctl.run(&["print", target])?.status.success())
}

/// Whether a failed bootstrap is launchd's error 5, which it answers while the old job is still
/// being removed, and which a later attempt gets past.
fn is_busy(output: &Output) -> bool {
    output.status.code() == Some(5)
        || String::from_utf8_lossy(&output.stderr).contains("Bootstrap failed: 5:")
}

fn uninstall_agent(label: &str) -> io::Result<()> {
    let target = format!("{}/{label}", domain());
    let _ = launchctl(&["bootout", &target]);
    // So that an install straight after this one finds the job gone.
    wait_until_gone(&SystemLaunchctl, &target)?;
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
        revive(&SystemLaunchctl, &domain(), LABEL, &plist_path(LABEL)?)
    }
}

/// Starts the job `label` again: `launchctl kickstart -k` when launchd has it loaded, and
/// otherwise loads its property list at `path` with [`reload`]. A job that was booted out, by a
/// person or by an install that failed half way, is not loaded, and kickstart alone answered
/// "Could not find service" every time, so the monitor could never bring the daemon back.
fn revive(launchctl: &dyn Launchctl, domain: &str, label: &str, path: &Path) -> io::Result<()> {
    let target = format!("{domain}/{label}");
    if !is_loaded(launchctl, &target)? {
        return reload(launchctl, domain, label, path);
    }
    let kicked = launchctl.run(&["kickstart", "-k", &target])?;
    if kicked.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "launchctl kickstart: {}",
            String::from_utf8_lossy(&kicked.stderr).trim()
        )))
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
///
/// Both agents are `ProcessType` `Standard`. Every supervisor and build the daemon starts
/// inherits its agent's process type, and with `Background` macOS gives them low CPU and I/O
/// priority and keeps them mostly on the efficiency cores. On a Mac with 6 performance and 12
/// efficiency cores at load 20 to 35, a cold `cargo build -p ignite_daemon -j 6` took 205 s and
/// 213 s of wall time (0.7 cores busy) under a `Background` agent, 28 s and 15 s (2.2 and 3.7
/// cores) under a `Standard` one, and 13 s (3.7 cores) from a shell. The monitor is
/// `Standard` too, so that it still runs on time when the machine is busy, which is when the
/// daemon is most likely to stop answering.
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
  <string>Standard</string>
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
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use super::*;

    /// A `launchctl` that answers each subcommand from a script of exit codes, records every
    /// call and sleep, and answers `print` with the last scripted code once its script runs out.
    #[derive(Default)]
    struct ScriptedLaunchctl {
        prints: RefCell<VecDeque<i32>>,
        bootstraps: RefCell<VecDeque<i32>>,
        calls: RefCell<Vec<String>>,
        slept: RefCell<Duration>,
    }

    impl ScriptedLaunchctl {
        fn new(prints: &[i32], bootstraps: &[i32]) -> Self {
            Self {
                prints: RefCell::new(prints.iter().copied().collect()),
                bootstraps: RefCell::new(bootstraps.iter().copied().collect()),
                ..Self::default()
            }
        }

        fn count(&self, verb: &str) -> usize {
            self.calls
                .borrow()
                .iter()
                .filter(|call| call.as_str() == verb)
                .count()
        }
    }

    impl Launchctl for ScriptedLaunchctl {
        fn run(&self, args: &[&str]) -> io::Result<Output> {
            self.calls.borrow_mut().push(args[0].to_owned());
            let (code, stderr) = match args[0] {
                "print" => {
                    let mut prints = self.prints.borrow_mut();
                    let code = if prints.len() > 1 {
                        prints.pop_front().unwrap()
                    } else {
                        prints.front().copied().unwrap_or(113)
                    };
                    (code, "")
                }
                "bootstrap" => match self.bootstraps.borrow_mut().pop_front().unwrap() {
                    0 => (0, ""),
                    5 => (5, "Bootstrap failed: 5: Input/output error"),
                    code => (code, "Bootstrap failed: 37: Operation already in progress"),
                },
                _ => (0, ""),
            };
            Ok(Output {
                status: ExitStatus::from_raw(code << 8),
                stdout: Vec::new(),
                stderr: stderr.as_bytes().to_vec(),
            })
        }

        fn sleep(&self, duration: Duration) {
            *self.slept.borrow_mut() += duration;
        }
    }

    fn reload_with(launchctl: &ScriptedLaunchctl) -> io::Result<()> {
        reload(
            launchctl,
            "gui/501",
            LABEL,
            Path::new("/tmp/ignite.daemon.plist"),
        )
    }

    #[test]
    fn revive_bootstraps_a_job_that_is_not_loaded() {
        // Not loaded: print fails, bootout is harmless, bootstrap loads it, print finds it.
        let launchctl = ScriptedLaunchctl::new(&[113, 113, 0], &[0]);
        revive(
            &launchctl,
            "gui/501",
            LABEL,
            Path::new("/tmp/ignite.daemon.plist"),
        )
        .unwrap();
        let calls = launchctl.calls.borrow();
        assert!(calls.contains(&"bootstrap".to_owned()), "{calls:?}");
        assert!(!calls.contains(&"kickstart".to_owned()), "{calls:?}");
    }

    #[test]
    fn revive_kickstarts_a_loaded_job() {
        let launchctl = ScriptedLaunchctl::new(&[0], &[]);
        revive(
            &launchctl,
            "gui/501",
            LABEL,
            Path::new("/tmp/ignite.daemon.plist"),
        )
        .unwrap();
        assert_eq!(*launchctl.calls.borrow(), ["print", "kickstart"]);
    }

    #[test]
    fn reload_waits_for_the_old_job_to_go_before_bootstrapping() {
        // The old job is still listed for three polls after bootout, then gone, then the new
        // one is loaded.
        let launchctl = ScriptedLaunchctl::new(&[0, 0, 0, 113, 0], &[0]);
        reload_with(&launchctl).unwrap();
        assert_eq!(
            *launchctl.calls.borrow(),
            [
                "bootout",
                "print",
                "print",
                "print",
                "print",
                "bootstrap",
                "print"
            ]
        );
        assert_eq!(*launchctl.slept.borrow(), POLL * 3);
    }

    #[test]
    fn reload_retries_a_bootstrap_that_fails_with_error_5() {
        let launchctl = ScriptedLaunchctl::new(&[113, 113, 113, 0], &[5, 5, 0]);
        reload_with(&launchctl).unwrap();
        assert_eq!(launchctl.count("bootstrap"), 3);
    }

    #[test]
    fn reload_fails_when_bootstrap_keeps_failing_with_error_5() {
        let launchctl = ScriptedLaunchctl::new(&[113], &[5; BOOTSTRAP_ATTEMPTS as usize]);
        let error = reload_with(&launchctl).unwrap_err();
        assert!(error.to_string().contains("Input/output error"), "{error}");
        assert_eq!(launchctl.count("bootstrap"), BOOTSTRAP_ATTEMPTS as usize);
    }

    #[test]
    fn reload_does_not_retry_other_bootstrap_errors() {
        let launchctl = ScriptedLaunchctl::new(&[113], &[37]);
        let error = reload_with(&launchctl).unwrap_err();
        assert!(error.to_string().contains("37"), "{error}");
        assert_eq!(launchctl.count("bootstrap"), 1);
    }

    #[test]
    fn reload_fails_when_the_job_is_not_loaded_after_bootstrap() {
        let launchctl = ScriptedLaunchctl::new(&[113], &[0]);
        assert!(reload_with(&launchctl).is_err());
    }

    #[test]
    fn waiting_for_a_job_that_never_goes_is_bounded() {
        let launchctl = ScriptedLaunchctl::new(&[0], &[]);
        wait_until_gone(&launchctl, "gui/501/ignite.daemon").unwrap();
        assert_eq!(launchctl.count("print"), GONE_POLLS as usize);
    }

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
        assert!(text.contains("<key>ProcessType</key>\n  <string>Standard</string>"));
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
        assert!(!text.contains("Background"));
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
