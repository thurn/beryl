//! The jobserver a supervisor runs its command under, so that every Cargo build the command
//! starts follows the grant as the grant shrinks and grows.
//!
//! Cargo takes its job slots from a FIFO in the GNU make jobserver protocol: one byte per slot
//! after the first, which every process holds without a byte. The supervisor makes the FIFO in
//! the run directory with one byte per granted core after the first, and names it in
//! `CARGO_MAKEFLAGS`. Every command gets one, whatever its first word: a Cargo run that `uv`,
//! `just` or a script starts inherits the variable and follows the grant too. About every
//! [`FOLLOW_EVERY`] the supervisor rereads the grant's cores file, which the scheduler rewrites
//! when the grant changes. When the size rises it writes a byte for each new slot. When it falls
//! it reads back bytes, but only those in the pipe, which no compile job holds; a slot a job
//! holds comes back when the job ends and is read back on a later pass. Cargo then runs fewer
//! jobs, and a waiting landing gets a long build's cores within seconds.
//!
//! Cargo takes its slots from an inherited jobserver whatever `CARGO_BUILD_JOBS` says (measured
//! with Cargo 1.98: a build with `CARGO_BUILD_JOBS=1` ran six build scripts at once on six
//! bytes). The supervisor still sets it to the most one command may hold, because Cargo passes
//! it to build scripts as `NUM_JOBS`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ignite_os::Native;
use ignite_os::fifo::Fifos;

/// How often the supervisor rereads the cores file. The scheduler takes one core per step from a
/// shrinking grant, about once a second, so this keeps up with it.
pub(super) const FOLLOW_EVERY: Duration = Duration::from_millis(250);

/// The FIFO's name in the run directory.
const FIFO: &str = "jobserver";

/// A jobserver FIFO and the slots this process has put in it beyond those it took back.
pub(super) struct Jobserver {
    fifo: File,
    path: PathBuf,
    /// Bytes written and not read back: the job slots the build may use beyond its first, less
    /// any it holds that a smaller grant has not yet taken back.
    slots: u32,
}

impl Jobserver {
    /// Makes the FIFO in run directory `dir` with room for `cores` jobs.
    pub(super) fn open(dir: &Path, cores: u32) -> io::Result<Self> {
        let path = dir.join(FIFO);
        let fifo = Native.open_fifo(&path)?;
        let mut jobserver = Self {
            fifo,
            path,
            slots: 0,
        };
        jobserver.resize(cores)?;
        Ok(jobserver)
    }

    /// The variable that points Cargo at the FIFO.
    pub(super) fn env(&self) -> (String, String) {
        (
            "CARGO_MAKEFLAGS".to_owned(),
            format!("-j --jobserver-auth=fifo:{}", self.path.display()),
        )
    }

    /// Moves toward room for `cores` jobs: writes the slots a larger grant adds, or reads back
    /// as many of those a smaller one removes as no job holds now.
    pub(super) fn resize(&mut self, cores: u32) -> io::Result<()> {
        let target = cores.max(1) - 1;
        if target > self.slots {
            let add = target - self.slots;
            self.fifo.write_all(&vec![b'+'; add as usize])?;
            self.slots = target;
        } else if target < self.slots {
            let mut buffer = vec![0; (self.slots - target) as usize];
            match self.fifo.read(&mut buffer) {
                Ok(read) => self.slots -= read as u32,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

/// Keeps `jobserver` at the size in `cores_file`, rereading it every [`FOLLOW_EVERY`], until the
/// returned sender is dropped. The thread then ends.
pub(super) fn follow(jobserver: Jobserver, cores_file: PathBuf) -> mpsc::SyncSender<()> {
    let (wake, woken) = mpsc::sync_channel(0);
    thread::spawn(move || keep(jobserver, &cores_file, &woken, FOLLOW_EVERY));
    wake
}

/// Rereads `cores_file` and resizes `jobserver` each time `every` passes or `woken` receives,
/// until every sender of `woken` is dropped, and returns the jobserver. A test wakes it instead
/// of waiting for the time to pass.
fn keep(
    mut jobserver: Jobserver,
    cores_file: &Path,
    woken: &mpsc::Receiver<()>,
    every: Duration,
) -> Jobserver {
    while let Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) = woken.recv_timeout(every) {
        if let Some(cores) = read_cores(cores_file)
            && let Err(error) = jobserver.resize(cores)
        {
            eprintln!("ignite: cannot resize the jobserver: {error}");
        }
    }
    jobserver
}

/// The grant size in a cores file, when it holds a number.
pub(super) fn read_cores(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Whether `args` run a Cargo command that only compiles, whose jobs end within seconds, so its
/// grant can shrink while it runs. Every command runs under the jobserver, but only these give
/// cores back: `cargo test`, `nextest`, `run` and any other command may run tests or a program
/// that take as many threads as they started with (`NEXTEST_TEST_THREADS` and the like), which
/// the jobserver does not control.
pub(super) fn compiles_only(args: &[String]) -> bool {
    const COMPILES: [&str; 5] = ["build", "check", "clippy", "rustc", "doc"];
    // `env NAME=value ... cargo ...` runs the same Cargo command: the release test build of the
    // fuzzers starts that way, and as a non-compiling command it never gave its cores back.
    let args = match args.split_first() {
        Some((program, rest)) if program == "env" => {
            let skip = rest.iter().take_while(|arg| arg.contains('=')).count();
            &rest[skip..]
        }
        _ => args,
    };
    let Some((program, rest)) = args.split_first() else {
        return false;
    };
    let name = Path::new(program).file_stem().and_then(|s| s.to_str());
    if name != Some("cargo") {
        return false;
    }
    // The subcommand is the first argument that is not a flag or a `+toolchain`. A test run
    // with `--no-run` only builds the test binaries.
    let no_run = rest.iter().any(|arg| arg == "--no-run");
    rest.iter()
        .find(|arg| !arg.starts_with(['-', '+']))
        .is_some_and(|sub| {
            COMPILES.contains(&sub.as_str())
                || (no_run && ["test", "nextest"].contains(&sub.as_str()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn only_cargo_commands_that_compile_shrink() {
        for args in [
            &["cargo", "build", "--workspace"][..],
            &["cargo", "+nightly", "--locked", "check"],
            &["/Users/me/.cargo/bin/cargo", "clippy"],
            &["cargo", "doc"],
            &["cargo", "rustc"],
            &[
                "env",
                "A=1",
                "B=2",
                "cargo",
                "test",
                "--release",
                "--no-run",
            ],
            &["cargo", "nextest", "run", "--no-run", "--workspace"],
        ] {
            assert!(compiles_only(&strings(args)), "{args:?}");
        }
        for args in [
            &["cargo", "test"][..],
            &["cargo", "nextest", "run"],
            &["cargo", "run", "--", "build"],
            &["cargo", "--version"],
            &["make", "build"],
            &["env", "A=1", "cargo", "test"],
            &["env", "A=1"],
            &[],
        ] {
            assert!(!compiles_only(&strings(args)), "{args:?}");
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ignite-jobserver-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Whether the FIFO holds no byte, read through the jobserver's own descriptor, which does
    /// not block.
    fn empty(jobserver: &mut Jobserver) -> bool {
        let mut byte = [0];
        matches!(jobserver.fifo.read(&mut byte), Err(e) if e.kind() == io::ErrorKind::WouldBlock)
    }

    /// A smaller grant takes back only the slots in the pipe. The one a job holds comes back on
    /// a later resize, once the job has ended and returned it.
    #[test]
    fn shrinking_takes_back_only_the_slots_no_job_holds() {
        let dir = scratch("shrink");
        let mut jobserver = Jobserver::open(&dir, 5).unwrap();
        let mut job = File::open(dir.join(FIFO)).unwrap();
        let mut held = [0];
        job.read_exact(&mut held).unwrap();

        jobserver.resize(1).unwrap();
        // Three of the four slots were in the pipe; the job's is still out.
        assert_eq!(jobserver.slots, 1);
        assert!(empty(&mut jobserver));

        File::options()
            .write(true)
            .open(dir.join(FIFO))
            .unwrap()
            .write_all(&held)
            .unwrap();
        jobserver.resize(1).unwrap();
        assert_eq!(jobserver.slots, 0);
        assert!(empty(&mut jobserver));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The FIFO gains a byte for each core the cores file adds, and gives the bytes back when the
    /// file falls.
    #[test]
    fn the_fifo_follows_the_cores_file() {
        let dir = scratch("follow");
        let cores = dir.join("cores");
        fs::write(&cores, "1\n").unwrap();
        let jobserver = Jobserver::open(&dir, 1).unwrap();
        let (wake, woken) = mpsc::sync_channel(0);
        let keeper = {
            let cores = cores.clone();
            thread::spawn(move || keep(jobserver, &cores, &woken, Duration::from_secs(3600)))
        };
        // A rendezvous channel: the second send returns once the first pass has finished.
        let pass = || {
            wake.send(()).unwrap();
            wake.send(()).unwrap();
        };

        fs::write(&cores, "3\n").unwrap();
        pass();
        let mut job = File::open(dir.join(FIFO)).unwrap();
        let mut slots = [0; 2];
        job.read_exact(&mut slots).unwrap();
        File::options()
            .write(true)
            .open(dir.join(FIFO))
            .unwrap()
            .write_all(&slots)
            .unwrap();

        fs::write(&cores, "1\n").unwrap();
        pass();
        drop(wake);
        let mut jobserver = keeper.join().unwrap();
        assert_eq!(jobserver.slots, 0);
        assert!(empty(&mut jobserver));
        fs::remove_dir_all(&dir).unwrap();
    }
}
