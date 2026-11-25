//! The job slots of a jobserver, which Cargo and `make` take from to decide how many jobs to run
//! at once. Every process holds one job without a slot; each slot lets one more job run, and a
//! job puts its slot back when it ends. The supervisor of a command keeps one slot in the
//! jobserver for each core its grant has beyond the first.
//!
//! On macOS the jobserver is a FIFO in the GNU make protocol, holding one byte per slot, and
//! Cargo finds it through `--jobserver-auth=fifo:<path>`. On Windows it is a named semaphore whose
//! count is the number of slots, and Cargo finds it through `--jobserver-auth=<name>`: the
//! `jobserver` crate that Cargo uses opens it with `OpenSemaphore`, takes a slot by waiting on it
//! and puts one back with `ReleaseSemaphore`.

use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Whether this system has a jobserver that a supervisor can resize while its command runs.
pub const AVAILABLE: bool = cfg!(any(target_os = "macos", windows));

/// An open jobserver: the slots it holds, and how another process finds it.
pub struct Slots {
    #[cfg(target_os = "macos")]
    fifo: std::fs::File,
    #[cfg(windows)]
    semaphore: windows::Semaphore,
    /// The value of `--jobserver-auth=` in `CARGO_MAKEFLAGS` that names this jobserver.
    auth: String,
}

impl Slots {
    /// The value of `--jobserver-auth=` that points Cargo at this jobserver.
    pub fn auth(&self) -> &str {
        &self.auth
    }
}

pub trait Jobservers {
    /// Makes a new jobserver that holds no slots, named after run directory `dir` (on macOS the
    /// FIFO is made there), readable and writable by this user only.
    fn make_jobserver(&self, dir: &Path) -> io::Result<Slots>;

    /// Opens the jobserver that `auth` names, as a job that Cargo starts would.
    fn open_jobserver(&self, auth: &str) -> io::Result<Slots>;

    /// Puts `count` slots into the jobserver.
    fn put_slots(&self, slots: &mut Slots, count: u32) -> io::Result<()>;

    /// Takes up to `count` slots out of the jobserver without waiting, and returns how many it
    /// took: only the slots no job holds now can be taken.
    fn take_slots(&self, slots: &mut Slots, count: u32) -> io::Result<u32>;
}
