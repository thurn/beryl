//! The Windows half of `occupants`: every process is listed with a Toolhelp snapshot, and each
//! one's executable is read with `QueryFullProcessImageNameW`. A process's working directory
//! lives in its own memory (its process environment block), which this does not read, so only
//! the executable decides whether a process is inside a directory.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
};

use super::{Occupant, Occupants};
use crate::Native;
use crate::proc::ENDED_EXIT_CODE;

impl Occupants for Native {
    fn occupants(&self, dir: &Path) -> io::Result<Vec<Occupant>> {
        let dir = comparable(&crate::paths::canonicalize(dir)?);
        let mut found = Vec::new();
        for entry in processes()? {
            let pid = entry.th32ProcessID;
            if pid == 0 {
                continue;
            }
            let Some(executable) = executable(pid) else {
                continue;
            };
            if comparable(&executable).starts_with(&dir) {
                found.push(Occupant {
                    pid,
                    executable: Some(executable),
                    cwd: None,
                });
            }
        }
        Ok(found)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        processes()
            .ok()?
            .into_iter()
            .find(|entry| entry.th32ProcessID == pid)
            .map(|entry| entry.th32ParentProcessID)
    }

    fn group(&self, _pid: u32) -> Option<u32> {
        None
    }

    fn end(&self, pids: &[u32], grace: Duration) -> io::Result<Vec<u32>> {
        let mut ended = Vec::new();
        for &pid in pids {
            let Some(process) = Process::open(pid, PROCESS_TERMINATE | PROCESS_SYNCHRONIZE)? else {
                continue;
            };
            // SAFETY: the handle is open with the right to terminate.
            if unsafe { TerminateProcess(process.0, ENDED_EXIT_CODE) } == 0 {
                let error = io::Error::last_os_error();
                // A process that is already exiting refuses to be terminated.
                if !process.exited_within(Duration::ZERO) {
                    return Err(error);
                }
                continue;
            }
            ended.push((pid, process));
        }
        let start = Instant::now();
        for (_, process) in &ended {
            process.exited_within(grace.saturating_sub(start.elapsed()));
        }
        Ok(ended.into_iter().map(|(pid, _)| pid).collect())
    }
}

/// An open process handle, closed when dropped.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by this value.
        unsafe { CloseHandle(self.0) };
    }
}

impl Process {
    /// Opens process `pid` with `access` rights, or `None` if it no longer exists.
    fn open(pid: u32, access: u32) -> io::Result<Option<Process>> {
        // SAFETY: plain call with integer arguments.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                return Ok(None);
            }
            return Err(error);
        }
        Ok(Some(Process(handle)))
    }

    /// Waits up to `wait` for the process to exit; true once it has.
    fn exited_within(&self, wait: Duration) -> bool {
        let ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: the handle is open with the right to wait on it.
        unsafe { WaitForSingleObject(self.0, ms) == WAIT_OBJECT_0 }
    }
}

/// Every process on the machine, as a Toolhelp snapshot lists it.
fn processes() -> io::Result<Vec<PROCESSENTRY32W>> {
    // SAFETY: plain call; the process ID is ignored for a process snapshot.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = Process(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    let mut found = Vec::new();
    // SAFETY: `entry` is a valid record whose `dwSize` gives its size.
    let mut more = unsafe { Process32FirstW(snapshot.0, &raw mut entry) } != 0;
    while more {
        found.push(entry);
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.0, &raw mut entry) } != 0;
    }
    Ok(found)
}

/// The full path of process `pid`'s executable, or `None` if this user may not read it or the
/// process has exited.
fn executable(pid: u32) -> Option<PathBuf> {
    let process = Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()??;
    // Long paths can exceed `MAX_PATH`; this is the longest path Windows allows.
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    // SAFETY: `buffer` is writable for `size` wide characters; the call writes at most that and
    // sets `size` to the length written.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &raw mut size,
        )
    };
    if ok == 0 || size == 0 {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buffer[..size as usize])))
}

/// `path` in a form two paths to the same place share: without the `\\?\` prefix that
/// `canonicalize` adds, and in lower case, since Windows paths ignore case.
fn comparable(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let plain = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    };
    PathBuf::from(plain.to_lowercase())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn finds_a_process_by_its_executable_and_ends_it() {
        let dir = ignite_core::test_files::temp_dir().join(format!("ignite-occupants-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // A copy of the command interpreter inside the directory, which waits for input that
        // never comes.
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let shell = dir.join("Shell.exe");
        fs::copy(Path::new(&system).join("System32").join("cmd.exe"), &shell).unwrap();
        let mut child = Command::new(&shell)
            .args(["/d", "/q", "/k"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let found = Native.occupants(&dir).unwrap();
        assert_eq!(
            found.iter().map(|o| o.pid).collect::<Vec<_>>(),
            vec![child.id()]
        );
        assert!(found[0].cwd.is_none());
        assert_eq!(Native.parent(child.id()), Some(std::process::id()));

        let ended = Native.end(&[child.id()], Duration::from_secs(3)).unwrap();
        assert_eq!(ended, vec![child.id()]);
        assert_eq!(child.wait().unwrap().code(), Some(ENDED_EXIT_CODE as i32));
        assert!(Native.occupants(&dir).unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_compare_without_the_verbatim_prefix_or_case() {
        assert_eq!(
            comparable(Path::new(r"\\?\C:\Users\Me\Trees")),
            comparable(Path::new(r"c:\users\me\trees"))
        );
        assert_eq!(
            comparable(Path::new(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\server\share\x")
        );
    }
}
