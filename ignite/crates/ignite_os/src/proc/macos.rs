//! The macOS half of `proc`: a command starts as the leader of a new process group, so every
//! process it starts (unless one leaves the group on purpose with `setsid`) can be listed and
//! signalled at once with the group's ID.

use std::io;
use std::os::unix::process::CommandExt;
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

    #[test]
    fn ending_a_finished_group_succeeds() {
        let mut group = Native
            .spawn_group(&mut Command::new("/usr/bin/true"))
            .unwrap();
        group.child.wait().unwrap();
        Native.end_group(&mut group).unwrap();
    }
}
