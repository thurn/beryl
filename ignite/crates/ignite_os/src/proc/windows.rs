//! The Windows half of `proc`: each command runs in a job object of its own, and every process
//! it starts joins that job. Ending the job ends them all, and the job's accounting keeps the
//! CPU time of every process it ever held, which `usage` reads.
//!
//! The job is named `Local\ignite-job-<pid>` after the command's process ID, which is also the
//! group's `id`. Another process, such as one ending a run, opens the job by that name.
//!
//! The command starts suspended and is resumed only once it is in the job, so it cannot start
//! a process outside the job first. The job is also set to end its processes when its last
//! handle closes, so a supervisor that exits or crashes leaves none of them running.

use std::collections::HashMap;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, OpenJobObjectW,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

use super::{Group, Processes};
use crate::Native;

/// The right to read a job's information. `windows-sys` has it only in its large
/// `SystemServices` module.
pub(crate) const JOB_OBJECT_QUERY: u32 = 0x0004;
/// The right to end a job's processes.
const JOB_OBJECT_TERMINATE: u32 = 0x0008;

/// How long to wait for a terminated job's processes to exit.
const KILLED: Duration = Duration::from_secs(5);
/// How often the job is checked while its processes exit.
const POLL: Duration = Duration::from_millis(20);

/// The jobs this process created, by group ID. Each stays open, and so alive, until its group
/// is ended or this process exits.
static JOBS: Mutex<Option<HashMap<u64, Job>>> = Mutex::new(None);

/// An open handle to a job object, closed when dropped.
pub(crate) struct Job(HANDLE);

// SAFETY: a job handle is a kernel object reference that any thread may use or close.
unsafe impl Send for Job {}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Job").field(&(self.0 as usize)).finish()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by this value.
        unsafe { CloseHandle(self.0) };
    }
}

impl Job {
    /// Opens the job of group `group` with `access` rights, or `None` if no process holds it
    /// open any more.
    pub(crate) fn open(group: u64, access: u32) -> io::Result<Option<Job>> {
        let name = job_name(group);
        // SAFETY: `name` is a NUL-terminated wide string that outlives the call.
        let handle = unsafe { OpenJobObjectW(access, 0, name.as_ptr()) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) {
                return Ok(None);
            }
            return Err(error);
        }
        Ok(Some(Job(handle)))
    }

    /// Reads one fixed-size record of the job's information.
    fn query<T: Default>(&self, class: i32) -> io::Result<T> {
        let mut info = T::default();
        // SAFETY: `info` is a writable `T`, and its size is the length passed.
        let ok = unsafe {
            QueryInformationJobObject(
                self.0,
                class,
                (&raw mut info).cast(),
                size_of::<T>() as u32,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    /// The job's totals so far: every process it held, including those that have exited.
    pub(crate) fn accounting(&self) -> io::Result<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION> {
        self.query(JobObjectBasicAccountingInformation)
    }

    /// The process IDs now in the job.
    pub(crate) fn members(&self) -> io::Result<Vec<u32>> {
        // The record is two counts followed by an array of IDs, so it is read into a buffer of
        // pointer-sized words, in which the array starts at word `FIRST`.
        const FIRST: usize = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
            / size_of::<usize>();
        let mut capacity = 64;
        loop {
            let mut buffer = vec![0usize; FIRST + capacity];
            // SAFETY: `buffer` is writable and as long as the length passed, and is aligned for
            // the record.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * size_of::<usize>()) as u32,
                    ptr::null_mut(),
                )
            };
            // SAFETY: plain call that reads the thread's last error.
            if ok == 0 && unsafe { GetLastError() } != ERROR_MORE_DATA {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: the buffer starts with the record's header, which the call filled.
            let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let listed = (list.NumberOfProcessIdsInList as usize).min(capacity);
            if ok != 0 && listed >= list.NumberOfAssignedProcesses as usize {
                return Ok(buffer[FIRST..FIRST + listed]
                    .iter()
                    .map(|&pid| pid as u32)
                    .collect());
            }
            capacity = capacity.max(list.NumberOfAssignedProcesses as usize) * 2;
        }
    }

    /// Ends every process in the job and waits up to [`KILLED`] for them to exit.
    fn terminate(&self) -> io::Result<()> {
        // SAFETY: the handle is open with `JOB_OBJECT_TERMINATE`.
        if unsafe { TerminateJobObject(self.0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let start = Instant::now();
        while self.accounting()?.ActiveProcesses > 0 && start.elapsed() < KILLED {
            thread::sleep(POLL);
        }
        Ok(())
    }
}

/// `Local\ignite-job-<group>` as a NUL-terminated wide string.
fn job_name(group: u64) -> Vec<u16> {
    format!("Local\\ignite-job-{group}")
        .encode_utf16()
        .chain([0])
        .collect()
}

impl Processes for Native {
    /// Starts `command` in a job of its own. This sets the command's creation flags, replacing
    /// any it had.
    fn spawn_group(&self, command: &mut Command) -> io::Result<Group> {
        let mut child = command.creation_flags(CREATE_SUSPENDED).spawn()?;
        let id = u64::from(child.id());
        let started = create_job(id, child.as_raw_handle()).and_then(|job| {
            resume(child.id())?;
            Ok(job)
        });
        match started {
            Ok(job) => {
                JOBS.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_or_insert_with(HashMap::new)
                    .insert(id, job);
                Ok(Group { child, id })
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    fn end_group(&self, group: &mut Group) -> io::Result<()> {
        let owned = JOBS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .and_then(|jobs| jobs.remove(&group.id));
        let job = match owned {
            Some(job) => Some(job),
            None => Job::open(group.id, JOB_OBJECT_TERMINATE | JOB_OBJECT_QUERY)?,
        };
        if let Some(job) = job {
            job.terminate()?;
        }
        group.child.wait()?;
        Ok(())
    }
}

impl Native {
    /// The process IDs now in the group `group`.
    pub fn group_members(&self, group: u64) -> io::Result<Vec<u32>> {
        match Job::open(group, JOB_OBJECT_QUERY)? {
            Some(job) => job.members(),
            None => Ok(Vec::new()),
        }
    }

    /// Ends every process in the group `group`, which may have been started by another process.
    /// Does nothing if the group no longer exists.
    pub fn end_group_id(&self, group: u64) -> io::Result<()> {
        match Job::open(group, JOB_OBJECT_TERMINATE | JOB_OBJECT_QUERY)? {
            Some(job) => job.terminate(),
            None => Ok(()),
        }
    }
}

/// Creates the job for group `id`, set to end its processes when its last handle closes, and
/// puts `process` in it.
fn create_job(id: u64, process: HANDLE) -> io::Result<Job> {
    let name = job_name(id);
    // SAFETY: `name` is a NUL-terminated wide string; a null security record means the default.
    let handle = unsafe { CreateJobObjectW(ptr::null(), name.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = Job(handle);
    // SAFETY: plain call that reads the thread's last error, set by `CreateJobObjectW`.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // The process ID was reused while an earlier command's job is still held open.
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("a job for process {id} already exists"),
        ));
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `limits` is a valid record of the size passed.
    let ok = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both handles are open; the process handle belongs to the child, which is alive.
    if unsafe { AssignProcessToJobObject(job.0, process) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

/// Resumes the threads of the suspended process `pid`. A process created suspended has one.
fn resume(pid: u32) -> io::Result<()> {
    // SAFETY: plain call; the snapshot handle is closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut resumed = 0;
    let mut result = Ok(());
    // SAFETY: `entry` is a valid record whose `dwSize` gives its size.
    let mut more = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: plain call; the thread handle is closed below.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                result = Err(io::Error::last_os_error());
                break;
            }
            // SAFETY: the thread handle is open with the right to resume it.
            let previous = unsafe { ResumeThread(thread) };
            // SAFETY: the thread handle is open and owned here.
            unsafe { CloseHandle(thread) };
            if previous == u32::MAX {
                result = Err(io::Error::last_os_error());
                break;
            }
            resumed += 1;
        }
        // SAFETY: as for `Thread32First`.
        more = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: the snapshot handle is open and owned here.
    unsafe { CloseHandle(snapshot) };
    result?;
    if resumed == 0 {
        return Err(io::Error::other(format!(
            "process {pid} has no thread to resume"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Stdio};

    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    use super::*;

    /// The variable that tells [`helper`] what to do.
    const ROLE: &str = "IGNITE_TEST_PROC_ROLE";

    fn helper_command(role: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "proc::windows::tests::helper", "--ignored"])
            .args(["--nocapture", "--test-threads=1"])
            .env(ROLE, role);
        command
    }

    /// Run by the tests below as a separate process. `parent` starts two `sleep` children,
    /// prints `pid <id>` for each and waits for them; `sleep` sleeps.
    #[test]
    #[ignore = "run by the other proc tests"]
    fn helper() {
        let Ok(role) = std::env::var(ROLE) else {
            return;
        };
        match role.as_str() {
            "parent" => {
                let children: Vec<Child> = (0..2)
                    .map(|_| {
                        helper_command("sleep")
                            .stdout(Stdio::null())
                            .spawn()
                            .unwrap()
                    })
                    .collect();
                for child in &children {
                    println!("pid {}", child.id());
                }
                for mut child in children {
                    let _ = child.wait();
                }
            }
            "sleep" => thread::sleep(Duration::from_secs(1000)),
            other => panic!("unknown role {other}"),
        }
        std::process::exit(0);
    }

    fn alive(pid: u32) -> bool {
        // SAFETY: plain call; the handle is closed below.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return false;
        }
        // SAFETY: the handle is open with the right to wait on it.
        let running = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
        // SAFETY: the handle is open and owned here.
        unsafe { CloseHandle(handle) };
        running
    }

    #[test]
    fn ending_a_group_ends_its_grandchildren() {
        let mut command = helper_command("parent");
        command.stdout(Stdio::piped());
        let mut group = Native.spawn_group(&mut command).unwrap();
        assert_eq!(group.id, u64::from(group.child.id()));

        let lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        let grandchildren: Vec<u32> = lines
            .map(Result::unwrap)
            .filter_map(|line| line.strip_prefix("pid ").map(|pid| pid.parse().unwrap()))
            .take(2)
            .collect();
        assert_eq!(grandchildren.len(), 2);
        let members = Native.group_members(group.id).unwrap();
        for pid in &grandchildren {
            assert!(alive(*pid));
            assert!(members.contains(pid), "{pid} not in {members:?}");
        }
        assert!(members.contains(&group.child.id()));

        Native.end_group(&mut group).unwrap();
        for pid in grandchildren {
            assert!(!alive(pid), "grandchild {pid} survived");
        }
        assert!(Native.group_members(group.id).unwrap().is_empty());
    }

    #[test]
    fn another_process_can_end_a_group_by_its_id() {
        let mut command = helper_command("sleep");
        let mut group = Native.spawn_group(&mut command).unwrap();
        Native.end_group_id(group.id).unwrap();
        assert!(!group.child.wait().unwrap().success());
        Native.end_group(&mut group).unwrap();
    }

    #[test]
    fn ending_a_finished_group_succeeds() {
        let mut command = Command::new("cmd");
        command.args(["/c", "exit 0"]);
        let mut group = Native.spawn_group(&mut command).unwrap();
        group.child.wait().unwrap();
        Native.end_group(&mut group).unwrap();
        Native.end_group_id(group.id).unwrap();
    }
}
