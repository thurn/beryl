//! The macOS half of `occupants`: every process is listed with `proc_listallpids`, and each
//! one's working directory and executable are read with `proc_pidinfo` and `proc_pidpath`.

use std::ffi::CStr;
use std::io;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use super::{Occupant, Occupants};
use crate::Native;

/// How often the processes are checked while they exit.
const POLL: Duration = Duration::from_millis(20);

impl Occupants for Native {
    fn occupants(&self, dir: &Path) -> io::Result<Vec<Occupant>> {
        let dir = crate::paths::canonicalize(dir)?;
        let mut found = Vec::new();
        for pid in all_pids()? {
            let Ok(pid) = u32::try_from(pid) else {
                continue;
            };
            if pid == 0 || zombie_or_gone(pid) {
                continue;
            }
            let cwd = working_directory(pid);
            let executable = executable(pid);
            let inside =
                |path: &Option<PathBuf>| path.as_ref().is_some_and(|p| p.starts_with(&dir));
            if inside(&cwd) || inside(&executable) {
                found.push(Occupant {
                    pid,
                    executable,
                    cwd,
                });
            }
        }
        Ok(found)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        bsd_info(pid).map(|info| info.pbi_ppid)
    }

    fn children(&self, pid: u32) -> io::Result<Vec<u32>> {
        let parent = libc::pid_t::try_from(pid)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not a process ID"))?;
        let mut capacity = 16;
        loop {
            let mut pids: Vec<libc::pid_t> = vec![0; capacity];
            let bytes = libc::c_int::try_from(capacity * size_of::<libc::pid_t>())
                .map_err(|_| io::Error::other("too many children to list"))?;
            // SAFETY: `pids` is a writable buffer of `bytes` bytes; the call writes at most that.
            let count =
                unsafe { libc::proc_listchildpids(parent, pids.as_mut_ptr().cast(), bytes) };
            if count < 0 {
                return Err(io::Error::last_os_error());
            }
            // The call returns a count of PIDs; a full buffer may have been cut short.
            let count = count as usize;
            if count < capacity {
                pids.truncate(count);
                return Ok(pids
                    .into_iter()
                    .filter_map(|child| u32::try_from(child).ok())
                    .filter(|&child| child > 0 && !zombie_or_gone(child))
                    .collect());
            }
            capacity *= 2;
        }
    }

    fn group(&self, pid: u32) -> Option<u32> {
        bsd_info(pid).map(|info| info.pbi_pgid)
    }

    fn end(&self, pids: &[u32], grace: Duration) -> io::Result<Vec<u32>> {
        for &pid in pids {
            signal(pid, libc::SIGTERM)?;
        }
        let start = Instant::now();
        loop {
            let left: Vec<u32> = pids
                .iter()
                .copied()
                .filter(|&p| !zombie_or_gone(p))
                .collect();
            if left.is_empty() {
                return Ok(Vec::new());
            }
            if start.elapsed() >= grace {
                for &pid in &left {
                    signal(pid, libc::SIGKILL)?;
                }
                return Ok(left);
            }
            thread::sleep(POLL);
        }
    }
}

/// Every process ID on the machine.
fn all_pids() -> io::Result<Vec<libc::pid_t>> {
    let mut capacity = 1024;
    loop {
        let mut pids: Vec<libc::pid_t> = vec![0; capacity];
        let bytes = libc::c_int::try_from(capacity * size_of::<libc::pid_t>())
            .map_err(|_| io::Error::other("too many processes to list"))?;
        // SAFETY: `pids` is a writable buffer of `bytes` bytes; the call writes at most that.
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        let count = count as usize;
        // A full buffer may have been cut short, so list again with more room.
        if count < capacity {
            pids.truncate(count);
            return Ok(pids);
        }
        capacity *= 2;
    }
}

fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: `proc_bsdinfo` is plain data, for which all zero bytes are a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: `info` is a writable buffer of `size` bytes; the call writes at most that.
    let written =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (written == size).then_some(info)
}

/// Whether the process has exited, counting one that waits for its parent to reap it.
fn zombie_or_gone(pid: u32) -> bool {
    bsd_info(pid).is_none_or(|info| info.pbi_status == libc::SZOMB)
}

fn working_directory(pid: u32) -> Option<PathBuf> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: `proc_vnodepathinfo` is plain data, for which all zero bytes are a valid value.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(size_of::<libc::proc_vnodepathinfo>()).ok()?;
    // SAFETY: `info` is a writable buffer of `size` bytes; the call writes at most that.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let path = info.pvi_cdir.vip_path.as_flattened();
    path_from(path)
}

fn executable(pid: u32) -> Option<PathBuf> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut buffer = vec![0 as c_char; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is a writable buffer of `size` bytes; the call writes at most that.
    let written = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), size) };
    if written <= 0 {
        return None;
    }
    path_from(&buffer)
}

/// The path in a NUL-terminated C buffer, or `None` for an empty one.
fn path_from(buffer: &[c_char]) -> Option<PathBuf> {
    // SAFETY: `c_char` and `u8` have the same size and alignment.
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), buffer.len()) };
    let text = CStr::from_bytes_until_nul(bytes).ok()?.to_str().ok()?;
    (!text.is_empty()).then(|| PathBuf::from(text))
}

fn signal(pid: u32, signal: libc::c_int) -> io::Result<()> {
    let id = libc::pid_t::try_from(pid)
        .ok()
        .filter(|&id| id > 1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a process ID"))?;
    // SAFETY: `kill` only reads its integer arguments.
    if unsafe { libc::kill(id, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        // The process exited since it was listed.
        Some(libc::ESRCH) => Ok(()),
        _ => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    #[test]
    fn finds_a_process_by_its_working_directory_and_ends_it() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-occupants-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new("/bin/sleep")
            .arg("1000")
            .current_dir(&dir)
            .spawn()
            .unwrap();
        let found = Native.occupants(&dir).unwrap();
        assert_eq!(
            found.iter().map(|o| o.pid).collect::<Vec<_>>(),
            vec![child.id()]
        );
        assert_eq!(Native.parent(child.id()), Some(std::process::id()));

        let killed = Native.end(&[child.id()], Duration::from_secs(3)).unwrap();
        assert!(killed.is_empty(), "sleep exits on SIGTERM");
        assert!(!child.wait().unwrap().success());
        assert!(Native.occupants(&dir).unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A shell that starts `sleep` in the background and prints its PID, so the grandchild is
    /// known to exist once the line is read.
    fn shell_with_a_background_sleep() -> (std::process::Child, u32) {
        use std::io::BufRead;
        let mut shell = Command::new("/bin/sh")
            .args(["-c", "/bin/sleep 1000 & echo $!; wait"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(shell.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        (shell, line.trim().parse().unwrap())
    }

    #[test]
    fn lists_what_runs_under_a_process_leaving_out_the_branch_of_the_asker() {
        let me = std::process::id();
        let (mut waiting, sleep) = shell_with_a_background_sleep();
        let (mut hook, asker) = shell_with_a_background_sleep();
        let found = Native.live_descendants(me, Some(asker)).unwrap();
        assert!(found.contains(&waiting.id()), "{found:?}");
        assert!(found.contains(&sleep), "a grandchild counts: {found:?}");
        assert!(!found.contains(&hook.id()), "{found:?}");
        assert!(!found.contains(&asker), "{found:?}");
        // An `exclude` that is not under the root leaves nothing out.
        let all = Native.live_descendants(me, Some(1)).unwrap();
        assert!(all.contains(&hook.id()) && all.contains(&asker), "{all:?}");
        // A process waiting to be reaped no longer counts.
        Native
            .end(&[sleep, asker, waiting.id()], Duration::from_secs(3))
            .unwrap();
        assert!(!Native.children(me).unwrap().contains(&waiting.id()));
        waiting.wait().unwrap();
        hook.wait().unwrap();
    }
}
