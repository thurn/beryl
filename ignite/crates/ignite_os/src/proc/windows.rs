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
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
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
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenProcess, OpenThread,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, ResumeThread, THREAD_SUSPEND_RESUME, TerminateProcess,
    WaitForSingleObject,
};

use super::{ENDED_EXIT_CODE, Group, Processes};
use crate::Native;

/// How long [`Processes::end_group`] waits for the processes of a terminated job to exit.
const END_WAIT: Duration = Duration::from_secs(10);

/// A process opened so that its exit can be waited for.
struct Exiting(HANDLE);

impl Exiting {
    fn open(pid: u32) -> Option<Exiting> {
        // SAFETY: plain call; the handle, if any, is closed when the `Exiting` is dropped.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        (!handle.is_null()).then_some(Exiting(handle))
    }

    /// Waits until the process has exited or `deadline` has passed.
    fn wait_until(&self, deadline: Instant) {
        let left = deadline.saturating_duration_since(Instant::now());
        let millis = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open with the right to wait on it.
        unsafe { WaitForSingleObject(self.0, millis) };
    }
}

impl Drop for Exiting {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned here.
        unsafe { CloseHandle(self.0) };
    }
}

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
        if unsafe { TerminateJobObject(self.0, ENDED_EXIT_CODE) } == 0 {
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
        spawn_group_with(command, create_job)
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
            // `TerminateJobObject` returns before the processes have exited, and a process that
            // is exiting has already left the job's list, so a caller that then deleted the tree
            // they ran in found their files still open. So each member is opened first and
            // waited for afterwards, for a bounded time.
            let members: Vec<Exiting> = job
                .members()?
                .into_iter()
                .filter_map(Exiting::open)
                .collect();
            job.terminate()?;
            let deadline = Instant::now() + END_WAIT;
            for member in members {
                member.wait_until(deadline);
            }
        }
        group.child.wait()?;
        Ok(())
    }

    /// Starts `command` in a process group of its own, so console control events sent to this
    /// process's group miss it, with a console of its own that has no window. This sets the
    /// command's creation flags, replacing any it had. A process that `spawn_group` starts
    /// from the command inherits that hidden console instead of opening a window.
    fn detach(&self, command: &mut Command) {
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
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

    /// Ends process `pid` if its executable lies inside `under` (after resolving links and
    /// junctions), and waits up to [`KILLED`] for it to exit. Returns `true` when a matching
    /// process was ended, or `false` when `pid` is gone or belongs to another program.
    pub fn terminate_under(&self, pid: u32, under: &Path) -> io::Result<bool> {
        let Ok(root) = crate::paths::canonicalize(under) else {
            return Ok(false);
        };
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
        // SAFETY: plain call; the handle is owned by `Exiting` and closed on drop.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            return Ok(false);
        }
        let process = Exiting(handle);
        let Some(image) = process_image(process.0) else {
            return Ok(false);
        };
        let Ok(real) = crate::paths::canonicalize(&image) else {
            return Ok(false);
        };
        if !real.starts_with(&root) {
            return Ok(false);
        }
        // SAFETY: `process.0` was opened with `PROCESS_TERMINATE`.
        if unsafe { TerminateProcess(process.0, ENDED_EXIT_CODE) } == 0 {
            return Err(io::Error::last_os_error());
        }
        process.wait_until(Instant::now() + KILLED);
        Ok(true)
    }
}

/// The executable path of an open process handle, or `None` if Windows could not report it.
fn process_image(handle: HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0u16; 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: `handle` has `PROCESS_QUERY_LIMITED_INFORMATION` and `buffer` has `len` elements.
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &raw mut len) };
    if ok == 0 {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buffer[..len as usize])))
}

/// Starts the command once its PID can name a new job. The test can supply a job creator that
/// forces a collision.
fn spawn_group_with(
    command: &mut Command,
    mut create: impl FnMut(u64, HANDLE) -> io::Result<Option<Job>>,
) -> io::Result<Group> {
    let mut rejected = Vec::new();
    command.creation_flags(CREATE_SUSPENDED);
    loop {
        let mut child = command.spawn()?;
        let id = u64::from(child.id());
        let started = create(id, child.as_raw_handle()).and_then(|job| {
            if job.is_some() {
                resume(child.id())?;
            }
            Ok(job)
        });
        match started {
            Ok(Some(job)) => {
                JOBS.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_or_insert_with(HashMap::new)
                    .insert(id, job);
                return Ok(Group { child, id });
            }
            Ok(None) => {
                child.kill()?;
                child.wait()?;
                // Windows can reuse a PID once all handles to the process are closed. Hold
                // this handle until a retry succeeds so it cannot pick the same PID again.
                rejected.push(child);
            }
            Err(error) => {
                if child.kill().is_ok() {
                    let _ = child.wait();
                }
                return Err(error);
            }
        }
    }
}

/// Creates the job for group `id`, set to end its processes when its last handle closes, and
/// puts `process` in it. Returns `None` if an older job still holds the name for this PID.
fn create_job(id: u64, process: HANDLE) -> io::Result<Option<Job>> {
    let name = job_name(id);
    // SAFETY: `name` is a NUL-terminated wide string; a null security record means the default.
    let handle = unsafe { CreateJobObjectW(ptr::null(), name.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = Job(handle);
    // SAFETY: plain call that reads the thread's last error, set by `CreateJobObjectW`.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(None);
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
    Ok(Some(job))
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

/// Runs `program` with `args` in place of this process, as far as Windows allows: it has no
/// `exec`, so the program starts as a child, which joins this process's job as every child does,
/// with the variables named in `remove` taken out of its environment. This process waits for it,
/// keeping the files in `keep` open (and any lock on them held) meanwhile, and exits with its
/// exit code. Returns only if the program could not be started.
pub fn become_program(
    program: &str,
    args: &[String],
    remove: &[&str],
    keep: &[&std::fs::File],
) -> io::Error {
    let mut command = Command::new(program);
    command.args(args);
    for name in remove {
        command.env_remove(name);
    }
    let status = match command.status() {
        Ok(status) => status,
        Err(error) => return error,
    };
    // The files stay open until this process exits, after the child has.
    let _ = keep;
    std::process::exit(status.code().unwrap_or(1))
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
    use crate::usage::{ProcessUsage, TreeMeter};

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
    /// prints `pid <id>` for each and waits for them; `sleep` prints `ready` and sleeps.
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
            "sleep" => {
                println!("ready");
                thread::sleep(Duration::from_secs(1000));
            }
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
            // libtest has already printed `test <name> ... ` on the helper's first line, without
            // a newline, so a `pid` line can start partway.
            .filter_map(|line| {
                line.rsplit_once("pid ")
                    .map(|(_, pid)| pid.parse().unwrap())
            })
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
    fn a_reused_job_name_retries_with_a_fresh_pid() {
        let mut old = Native
            .spawn_group(helper_command("sleep").stdout(Stdio::null()))
            .unwrap();
        let mut old_id = Some(old.id);
        let mut attempted = Vec::new();
        let mut group = spawn_group_with(
            helper_command("sleep").stdout(Stdio::piped()),
            |id, process| {
                attempted.push(id);
                create_job(old_id.take().unwrap_or(id), process)
            },
        )
        .unwrap();
        assert_eq!(attempted.len(), 2);
        assert_ne!(attempted[0], attempted[1]);
        assert_eq!(group.id, u64::from(group.child.id()));
        assert_eq!(Native.group_members(group.id).unwrap(), [group.child.id()]);
        assert_eq!(Native.group_members(old.id).unwrap(), [old.child.id()]);

        let lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        assert!(
            lines
                .map(Result::unwrap)
                .any(|line| line.ends_with("ready"))
        );
        let mut meter = TreeMeter::new(group.id, group.child.id());
        meter.sample().unwrap();
        assert!(meter.finish(ProcessUsage::default()).peak_bytes > 0);

        Native.end_group_id(group.id).unwrap();
        assert!(!group.child.wait().unwrap().success());
        assert!(old.child.try_wait().unwrap().is_none());
        assert_eq!(Native.group_members(old.id).unwrap(), [old.child.id()]);
        Native.end_group(&mut group).unwrap();
        Native.end_group(&mut old).unwrap();
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

    #[test]
    fn terminate_under_ends_only_a_process_inside_the_directory() {
        let mut child = helper_command("sleep").spawn().unwrap();
        let pid = child.id();
        let other = ignite_core::test_files::temp_dir().join("unrelated-dir");
        std::fs::create_dir_all(&other).unwrap();
        assert!(!Native.terminate_under(pid, &other).unwrap());
        assert!(alive(pid));

        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(Native.terminate_under(pid, &exe_dir).unwrap());
        let _ = child.wait();
        assert!(!alive(pid));
    }
}
