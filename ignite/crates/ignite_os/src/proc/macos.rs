//! The macOS half of `proc`: a command starts as the leader of a new process group, so every
//! process it starts (unless one leaves the group on purpose with `setsid`) can be listed and
//! signalled at once with the group's ID.

use std::ffi::{CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use super::{Group, Processes};
use crate::Native;

/// How long the group has to exit after `SIGTERM` before it gets `SIGKILL`.
const GRACE: Duration = Duration::from_secs(5);
/// How long to wait for killed processes to be reaped before giving up on watching them.
const KILLED: Duration = Duration::from_secs(5);
/// How often the group is checked while it exits.
const POLL: Duration = Duration::from_millis(20);

impl Processes for Native {
    fn spawn_group(&self, command: &mut Command) -> io::Result<Group> {
        // A process group ID of 0 makes the child the leader of a group whose ID is its PID.
        let child = command.process_group(0).spawn()?;
        let id = u64::from(child.id());
        Ok(Group { child, id })
    }

    fn end_group(&self, group: &mut Group) -> io::Result<()> {
        let id = group_id(group.id)?;
        if !signal_group(id, libc::SIGTERM)? {
            group.child.wait()?;
            return Ok(());
        }
        if !wait_until_empty(group, id, GRACE)? {
            signal_group(id, libc::SIGKILL)?;
            wait_until_empty(group, id, KILLED)?;
        }
        group.child.wait()?;
        Ok(())
    }

    fn detach(&self, command: &mut Command) {
        command.process_group(0);
    }
}

impl Native {
    /// The PIDs of every process in the process group `group`, its leader included while it
    /// has not been reaped.
    pub fn group_members(&self, group: u64) -> io::Result<Vec<u32>> {
        let id = group_id(group)?;
        let mut capacity = 64;
        loop {
            let mut pids: Vec<libc::pid_t> = vec![0; capacity];
            let bytes = libc::c_int::try_from(capacity * size_of::<libc::pid_t>())
                .map_err(|_| io::Error::other("process group too large to list"))?;
            // SAFETY: `pids` is a writable buffer of `bytes` bytes; the call writes at most that.
            let count = unsafe { libc::proc_listpgrppids(id, pids.as_mut_ptr().cast(), bytes) };
            if count < 0 {
                return Err(io::Error::last_os_error());
            }
            let count = count as usize;
            // A full buffer may have been cut short, so list again with more room.
            if count < capacity {
                pids.truncate(count);
                return Ok(pids
                    .into_iter()
                    .filter(|&pid| pid > 0)
                    .map(|pid| pid as u32)
                    .collect());
            }
            capacity *= 2;
        }
    }
}

unsafe extern "C" {
    /// Marks `fd` to be kept by the new image even under `POSIX_SPAWN_CLOEXEC_DEFAULT`. In
    /// macOS's libSystem since 10.7; the `libc` crate does not declare it.
    fn posix_spawn_file_actions_addinherit_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        fd: libc::c_int,
    ) -> libc::c_int;
}

/// Replaces this process's image with `executable` run with `args`, keeping its process ID, its
/// environment and its working directory, as `exec` does. Returns only if it could not.
///
/// Unlike `exec`, the new image keeps only standard input, output and error. macOS creates a
/// socket, such as one `accept` returns, and marks it close-on-exec in two steps, so a plain
/// `exec` on one thread while another thread accepts a connection can carry that connection
/// into the new image. Nothing there knows of it, so it is never answered and never closed, and
/// the client waits on it for ever. `posix_spawn` with `POSIX_SPAWN_SETEXEC` replaces the
/// image, and `POSIX_SPAWN_CLOEXEC_DEFAULT` closes every descriptor not named to be kept, in the
/// one call. As `std::process::Command` does for a child, the new image starts with no signal
/// blocked and with `SIGPIPE` at its default action.
pub fn replace_image(executable: &Path, args: &[String]) -> io::Error {
    let invalid = |_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL byte in an argument");
    let c_string = |text: &OsStr| CString::new(text.as_bytes()).map_err(invalid);
    let built = (|| -> io::Result<(CString, Vec<CString>, Vec<CString>)> {
        let program = c_string(executable.as_os_str())?;
        let mut arguments = vec![program.clone()];
        for arg in args {
            arguments.push(c_string(OsStr::new(arg))?);
        }
        let mut variables = Vec::new();
        for (name, value) in std::env::vars_os() {
            let mut pair = name.into_encoded_bytes();
            pair.push(b'=');
            pair.extend(value.into_encoded_bytes());
            variables.push(CString::new(pair).map_err(invalid)?);
        }
        Ok((program, arguments, variables))
    })();
    let (program, arguments, variables) = match built {
        Ok(built) => built,
        Err(error) => return error,
    };
    let pointers = |strings: &[CString]| -> Vec<*mut libc::c_char> {
        strings
            .iter()
            .map(|s| s.as_ptr().cast_mut())
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect()
    };
    let (argument_pointers, variable_pointers) = (pointers(&arguments), pointers(&variables));
    let check = |code: libc::c_int| match code {
        0 => Ok(()),
        code => Err(io::Error::from_raw_os_error(code)),
    };
    // SAFETY: every pointer passed points into `program`, `arguments`, `variables` or the
    // attributes and actions initialised here, all of which live until the end of the block;
    // both pointer lists end with a null pointer. The attributes and actions are destroyed only
    // after `posix_spawn`, which with `POSIX_SPAWN_SETEXEC` does not return when it succeeds.
    unsafe {
        let mut attributes: libc::posix_spawnattr_t = std::ptr::null_mut();
        if let Err(error) = check(libc::posix_spawnattr_init(&raw mut attributes)) {
            return error;
        }
        let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
        if let Err(error) = check(libc::posix_spawn_file_actions_init(&raw mut actions)) {
            libc::posix_spawnattr_destroy(&raw mut attributes);
            return error;
        }
        let mut none: libc::sigset_t = 0;
        libc::sigemptyset(&raw mut none);
        let mut pipe: libc::sigset_t = 0;
        libc::sigemptyset(&raw mut pipe);
        libc::sigaddset(&raw mut pipe, libc::SIGPIPE);
        let flags = libc::POSIX_SPAWN_SETEXEC
            | libc::POSIX_SPAWN_CLOEXEC_DEFAULT
            | libc::POSIX_SPAWN_SETSIGMASK
            | libc::POSIX_SPAWN_SETSIGDEF;
        let prepared = check(libc::posix_spawnattr_setflags(
            &raw mut attributes,
            // The flags all fit in the `short` the call takes.
            flags as libc::c_short,
        ))
        .and_then(|()| {
            check(libc::posix_spawnattr_setsigmask(
                &raw mut attributes,
                &raw const none,
            ))
        })
        .and_then(|()| {
            check(libc::posix_spawnattr_setsigdefault(
                &raw mut attributes,
                &raw const pipe,
            ))
        })
        .and_then(|()| {
            (0..=2).try_for_each(|fd| {
                check(posix_spawn_file_actions_addinherit_np(&raw mut actions, fd))
            })
        });
        let error = if let Err(error) = prepared {
            error
        } else {
            let code = libc::posix_spawn(
                std::ptr::null_mut(),
                program.as_ptr(),
                &raw const actions,
                &raw const attributes,
                argument_pointers.as_ptr(),
                variable_pointers.as_ptr(),
            );
            io::Error::from_raw_os_error(code)
        };
        libc::posix_spawn_file_actions_destroy(&raw mut actions);
        libc::posix_spawnattr_destroy(&raw mut attributes);
        error
    }
}

fn group_id(group: u64) -> io::Result<libc::pid_t> {
    libc::pid_t::try_from(group)
        .ok()
        .filter(|&id| id > 1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a process group ID"))
}

/// Sends `signal` to every process in the group. Returns false if the group no longer exists.
fn signal_group(id: libc::pid_t, signal: libc::c_int) -> io::Result<bool> {
    // SAFETY: `killpg` only reads its integer arguments.
    if unsafe { libc::killpg(id, signal) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        // macOS answers EPERM when every process left in the group is a zombie.
        Some(libc::EPERM) => Ok(true),
        _ => Err(error),
    }
}

/// Reaps the leader if it has exited and waits up to `limit` for the group to empty.
/// Returns true once no process is left in it. A process killed or exited stays in the group
/// as a zombie until its parent reaps it: the leader's parent is this process, and launchd
/// reaps the descendants whose parents have gone.
fn wait_until_empty(group: &mut Group, id: libc::pid_t, limit: Duration) -> io::Result<bool> {
    let start = Instant::now();
    loop {
        group.child.try_wait()?;
        if !signal_group(id, 0)? {
            return Ok(true);
        }
        if start.elapsed() >= limit {
            return Ok(false);
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    use super::*;

    fn alive(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn ending_a_group_ends_its_grandchildren() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 1000 & echo $!; sleep 1000 & echo $!; wait"])
            .stdout(Stdio::piped());
        let mut group = Native.spawn_group(&mut command).unwrap();
        assert_eq!(group.id, u64::from(group.child.id()));

        // The shell prints each grandchild's PID once it has started it.
        let mut lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        let grandchildren: Vec<libc::pid_t> = (0..2)
            .map(|_| lines.next().unwrap().unwrap().parse().unwrap())
            .collect();
        let members = Native.group_members(group.id).unwrap();
        for pid in &grandchildren {
            assert!(alive(*pid));
            assert!(members.contains(&(*pid as u32)), "{pid} not in {members:?}");
        }
        assert!(members.contains(&group.child.id()));

        Native.end_group(&mut group).unwrap();
        for pid in grandchildren {
            assert!(!alive(pid), "grandchild {pid} survived");
        }
        assert!(Native.group_members(group.id).unwrap().is_empty());
    }

    /// Set for the copy of this test binary that [`replacing_the_image_keeps_only_standard_io`]
    /// starts: its value is a descriptor the copy opens without close-on-exec.
    const REPLACED: &str = "IGNITE_OS_TEST_REPLACED";

    /// A replaced image keeps standard output, the environment and the working directory, and
    /// loses a descriptor that has no close-on-exec mark, which a plain `exec` would keep.
    #[test]
    fn replacing_the_image_keeps_only_standard_io() {
        let dir = std::env::temp_dir().join(format!("ign-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let expected = format!(
            "replaced 1 closed {}",
            dir.canonicalize().unwrap().display()
        );
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "proc::macos::tests::replaced_image",
                "--nocapture",
            ])
            .env(REPLACED, "1")
            .current_dir(&dir)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let out = String::from_utf8_lossy(&output.stdout);
        // The test harness prints `test <name> ... ` without a newline before the test runs,
        // so the replaced image's line may follow that prefix on the same line.
        let line = out
            .lines()
            .find_map(|line| line.find("replaced 1 ").map(|start| &line[start..]));
        assert_eq!(line, Some(expected.as_str()), "{out}");
    }

    /// The other half of [`replacing_the_image_keeps_only_standard_io`]: does nothing unless
    /// that test started this binary.
    #[test]
    fn replaced_image() {
        if std::env::var_os(REPLACED).is_none() {
            return;
        }
        let mut fds = [0; 2];
        // SAFETY: `pipe` writes two descriptors into `fds`; neither is marked close-on-exec.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let fd = fds[0];
        let script = format!(
            "if [ -e /dev/fd/{fd} ]; then kept=kept; else kept=closed; fi; \
             echo \"replaced ${REPLACED} $kept $(pwd -P)\""
        );
        let error = replace_image(Path::new("/bin/sh"), &["-c".to_owned(), script]);
        panic!("not replaced: {error}");
    }

    #[test]
    fn ending_a_finished_group_succeeds() {
        let mut group = Native
            .spawn_group(&mut Command::new("/usr/bin/true"))
            .unwrap();
        group.child.wait().unwrap();
        Native.end_group(&mut group).unwrap();
    }
}
