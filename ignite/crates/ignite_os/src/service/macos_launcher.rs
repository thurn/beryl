//! The launcher that the daemon's launch agent starts on macOS: `bin/launcher launch <program>
//! <args>...`. It starts the program as its child, passes on the signals launchd sends it and
//! exits with the program's status.
//!
//! macOS scans every newly built program before it first runs (`syspolicyd` and
//! `XprotectService`), unless the program's responsible process is listed under Developer Tools
//! in System Settings, Privacy & Security. launchd makes each job its own responsible process,
//! and every process the job starts inherits it, so the program to list is the one the agent
//! runs. A release is linker-signed ad hoc, so macOS keys a Developer Tools grant on its code
//! hash and its path, and each release has a new hash and a new path. When the agent ran the
//! daemon directly, a grant lapsed at every release.
//!
//! The launcher is a copy of the `ignite` executable at a path that never changes. It is copied
//! once, when there is none, and never replaced, so its code hash never changes either and one
//! grant lasts. The daemon it starts runs from `bin/current` as before, and restarts into a new
//! release by replacing its own process image, which keeps the launcher as its parent and its
//! responsible process.
//!
//! The launcher stays frozen at the release that first wrote it, so what it does must not
//! change: [`launch`] reads its arguments as one program and that program's arguments, and does
//! nothing else.

use std::fs;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};

/// The launcher's file name, in the folder of the executable the agent would otherwise run
/// (`bin/launcher` beside `bin/current`).
pub const LAUNCHER: &str = "launcher";

/// The first argument that makes `ignite` act as the launcher.
pub const LAUNCH_VERB: &str = "launch";

/// The signals launchd and a person send the job that the launcher passes on to its child:
/// launchd stops a job with `SIGTERM`.
const FORWARDED: [libc::c_int; 4] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT];

/// The child's process ID, for the signal handler; 0 before it starts.
static CHILD: AtomicI32 = AtomicI32::new(0);

extern "C" fn forward(signal: libc::c_int) {
    let child = CHILD.load(Ordering::SeqCst);
    if child > 0 {
        // SAFETY: `kill` is async-signal-safe and takes plain integers.
        unsafe {
            libc::kill(child, signal);
        }
    }
}

/// Runs `program` (the program followed by its arguments) as a child, passes on the signals in
/// [`FORWARDED`] and returns the exit code to exit with: the child's, or 128 plus the signal
/// that killed it, as a shell reports it. A child that cannot start returns 127.
pub fn launch(program: &[String]) -> u8 {
    let Some((first, rest)) = program.split_first() else {
        eprintln!("usage: ignite {LAUNCH_VERB} <program> [arguments]...");
        return 64;
    };
    let mut child = match Command::new(first).args(rest).spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("ignite {LAUNCH_VERB}: cannot start {first}: {error}");
            return 127;
        }
    };
    CHILD.store(
        i32::try_from(child.id()).unwrap_or_default(),
        Ordering::SeqCst,
    );
    for signal in FORWARDED {
        // SAFETY: `forward` only reads an atomic and calls `kill`, both async-signal-safe.
        // macOS's `signal` keeps the handler installed and restarts interrupted calls.
        unsafe {
            libc::signal(
                signal,
                forward as extern "C" fn(libc::c_int) as libc::sighandler_t,
            );
        }
    }
    match child.wait() {
        Ok(status) => status.code().map_or_else(
            || u8::try_from(128 + status.signal().unwrap_or(0)).unwrap_or(u8::MAX),
            |code| u8::try_from(code).unwrap_or(u8::MAX),
        ),
        Err(error) => {
            eprintln!("ignite {LAUNCH_VERB}: cannot wait for {first}: {error}");
            1
        }
    }
}

/// Where the launcher for `executable` lives: beside it.
pub fn launcher_for(executable: &Path) -> PathBuf {
    executable.parent().unwrap_or(Path::new(".")).join(LAUNCHER)
}

/// Copies the executable that `executable` names (following links) to its launcher path when
/// there is no launcher yet, and returns that path. An existing launcher is kept as it is: a
/// new copy would have a new code hash, and macOS would forget the Developer Tools grant.
pub(super) fn ensure_launcher(executable: &Path) -> io::Result<PathBuf> {
    let launcher = launcher_for(executable);
    if launcher.is_file() {
        return Ok(launcher);
    }
    let source = crate::paths::canonicalize(executable)?;
    let partial = launcher.with_file_name(format!("{LAUNCHER}.partial-{}", std::process::id()));
    fs::copy(&source, &partial)?;
    fs::rename(&partial, &launcher)?;
    Ok(launcher)
}

/// Whether the agent program `program` already starts through the launcher.
pub(super) fn is_wrapped(program: &[String]) -> bool {
    program.get(1).map(String::as_str) == Some(LAUNCH_VERB)
        && program
            .first()
            .is_some_and(|first| Path::new(first).file_name() == Some(LAUNCHER.as_ref()))
}

/// The agent program that starts `program` through the launcher beside its executable, or
/// `program` itself when it already does.
pub(super) fn wrapped(program: &[String]) -> Vec<String> {
    let Some(first) = program.first() else {
        return Vec::new();
    };
    if is_wrapped(program) {
        return program.to_vec();
    }
    let mut out = vec![
        launcher_for(Path::new(first))
            .to_string_lossy()
            .into_owned(),
        LAUNCH_VERB.to_owned(),
    ];
    out.extend(program.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_daemon_program_starts_through_the_launcher_beside_it_once() {
        let program = strings(&["/opt/ignite/bin/current", "daemon"]);
        let once = wrapped(&program);
        assert_eq!(
            once,
            strings(&[
                "/opt/ignite/bin/launcher",
                "launch",
                "/opt/ignite/bin/current",
                "daemon"
            ])
        );
        assert!(is_wrapped(&once));
        assert_eq!(wrapped(&once), once);
    }

    #[test]
    fn the_launcher_passes_on_the_childs_exit_code() {
        assert_eq!(launch(&strings(&["/bin/sh", "-c", "exit 3"])), 3);
        assert_eq!(launch(&strings(&["/bin/sh", "-c", "kill -9 $$"])), 128 + 9);
        assert_eq!(launch(&strings(&["/nonexistent/ignite"])), 127);
        assert_eq!(launch(&[]), 64);
    }

    #[test]
    fn an_existing_launcher_is_never_replaced() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-launcher-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let release = dir.join("ignite");
        fs::write(&release, b"first release").unwrap();
        let current = dir.join("current");
        let _ = fs::remove_file(&current);
        std::os::unix::fs::symlink(&release, &current).unwrap();
        let launcher = ensure_launcher(&current).unwrap();
        fs::write(&release, b"second release").unwrap();
        let again = ensure_launcher(&current).unwrap();
        let kept = fs::read(&launcher).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(launcher, dir.join(LAUNCHER));
        assert_eq!(again, launcher);
        assert_eq!(kept, b"first release");
    }
}
