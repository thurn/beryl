//! A `SIGTERM` handler that only writes one byte to a pipe, which is all a signal handler may
//! safely do, and a thread that waits on the pipe and runs the last step.

use std::io;
use std::sync::atomic::{AtomicI32, Ordering};

use super::LastStep;

/// The write end of the pipe, or -1 before [`on_terminate`] made it.
static WRITE_END: AtomicI32 = AtomicI32::new(-1);

extern "C" fn handle(_signal: libc::c_int) {
    let fd = WRITE_END.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte = 1u8;
        // SAFETY: `write` is async-signal-safe and the buffer lives for the call.
        unsafe { libc::write(fd, (&raw const byte).cast(), 1) };
    }
}

pub fn on_terminate(last: LastStep) -> io::Result<()> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for the two descriptors `pipe` writes.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        // Programs the daemon starts do not inherit the pipe.
        // SAFETY: `fd` is a descriptor `pipe` just returned.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    let [read_end, write_end] = fds;
    std::thread::Builder::new()
        .name("ignite-terminate".to_owned())
        .spawn(move || {
            if wait_for_byte(read_end) {
                last();
                end_with_sigterm();
            }
        })?;
    WRITE_END.store(write_end, Ordering::Relaxed);
    // SAFETY: a zeroed `sigaction` is a valid value to fill in.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handle as extern "C" fn(libc::c_int) as libc::sighandler_t;
    action.sa_flags = libc::SA_RESTART;
    // SAFETY: `action` and its mask are valid for the calls.
    let installed = unsafe {
        libc::sigemptyset(&raw mut action.sa_mask);
        libc::sigaction(libc::SIGTERM, &raw const action, std::ptr::null_mut())
    };
    if installed != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Blocks until the handler writes its byte. Returns `false` if the pipe fails instead.
fn wait_for_byte(fd: libc::c_int) -> bool {
    loop {
        let mut byte = 0u8;
        // SAFETY: the buffer has room for the one byte asked for.
        let read = unsafe { libc::read(fd, (&raw mut byte).cast(), 1) };
        if read == 1 {
            return true;
        }
        if read < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return false;
    }
}

/// Ends the process the way `SIGTERM` would have without a handler.
fn end_with_sigterm() -> ! {
    // SAFETY: restoring the default action and raising the signal on this thread end the
    // process; neither touches memory.
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
        libc::raise(libc::SIGTERM);
    }
    std::process::exit(128 + libc::SIGTERM);
}
