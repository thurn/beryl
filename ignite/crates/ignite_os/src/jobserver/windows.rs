//! The Windows half of `jobserver`: a named semaphore whose count is the number of slots. A job
//! takes a slot by waiting on the semaphore and puts it back with `ReleaseSemaphore`, as the
//! `jobserver` crate in Cargo does. The supervisor puts slots in the same way, and takes them back
//! by waiting with a zero timeout, which takes one slot if the count is above zero and otherwise
//! returns at once.
//!
//! The semaphore is named `ignite-jobserver-<pid>-<n>` after the supervisor's process ID, in the
//! session's namespace, where the command's processes open it by that name. It lives while any
//! process holds it open, so the supervisor's handle keeps it for the whole command.

use std::io;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    CreateSemaphoreW, OpenSemaphoreW, ReleaseSemaphore, SEMAPHORE_MODIFY_STATE,
    SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
};

use super::{Jobservers, Slots};
use crate::Native;

/// The most slots the semaphore may hold. A grant never comes near it; it only has to be set.
const MOST: i32 = i32::MAX;

/// Semaphores made by this process so far, so that each gets a name of its own.
static MADE: AtomicU32 = AtomicU32::new(0);

/// An open handle to a semaphore, closed when dropped.
pub(super) struct Semaphore(HANDLE);

// SAFETY: a semaphore handle is a kernel object reference that any thread may use or close.
unsafe impl Send for Semaphore {}

impl Drop for Semaphore {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by this value.
        unsafe { CloseHandle(self.0) };
    }
}

impl Jobservers for Native {
    fn make_jobserver(&self, _dir: &Path) -> io::Result<Slots> {
        let name = format!(
            "ignite-jobserver-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        );
        let wide = wide(&name);
        // SAFETY: `wide` is a NUL-terminated string that outlives the call; no security
        // attributes means the default ones, and a handle that is not inherited.
        let handle = unsafe { CreateSemaphoreW(ptr::null(), 0, MOST, wide.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let semaphore = Semaphore(handle);
        // SAFETY: reads this thread's last error, which `CreateSemaphoreW` just set.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // Another process made a semaphore of this name, and its count is not ours.
            return Err(io::Error::other(format!(
                "a semaphore {name} exists already"
            )));
        }
        Ok(Slots {
            semaphore,
            auth: name,
        })
    }

    fn open_jobserver(&self, auth: &str) -> io::Result<Slots> {
        let wide = wide(auth);
        // SAFETY: `wide` is a NUL-terminated string that outlives the call.
        let handle = unsafe {
            OpenSemaphoreW(
                SEMAPHORE_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                0,
                wide.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Slots {
            semaphore: Semaphore(handle),
            auth: auth.to_owned(),
        })
    }

    fn put_slots(&self, slots: &mut Slots, count: u32) -> io::Result<()> {
        if count == 0 {
            // `ReleaseSemaphore` fails on a count of zero.
            return Ok(());
        }
        let count = i32::try_from(count).map_err(io::Error::other)?;
        // SAFETY: the handle is open with the right to modify the semaphore.
        if unsafe { ReleaseSemaphore(slots.semaphore.0, count, ptr::null_mut()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn take_slots(&self, slots: &mut Slots, count: u32) -> io::Result<u32> {
        let mut taken = 0;
        while taken < count {
            // SAFETY: the handle is open with the right to wait on it.
            match unsafe { WaitForSingleObject(slots.semaphore.0, 0) } {
                WAIT_OBJECT_0 => taken += 1,
                WAIT_FAILED => return Err(io::Error::last_os_error()),
                // `WAIT_TIMEOUT`: every slot is held by a job.
                _ => break,
            }
        }
        Ok(taken)
    }
}

/// `text` as a NUL-terminated UTF-16 string.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
