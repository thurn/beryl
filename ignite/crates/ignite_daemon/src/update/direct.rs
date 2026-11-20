//! Running the release build and its selftest as the daemon's own child processes, on the
//! machine's express cores, instead of through the scheduler.
//!
//! On 2026-11-19 an Ignite fix reached the hub's machine a median of 395 s after its landing
//! moved the release branch, over 15 releases. The release build asked the scheduler for cores
//! as an `agent` command and waited 0 to 451 s for them while the repository's other builds
//! held the machine, then got a single core and compiled for 11 to 235 s. Its selftest waited
//! up to 155 s more for one core.
//!
//! A release is the fix every session on the machine waits for, so [`ExpressLauncher`] runs
//! both commands right away as child process groups on the `express_cores` the scheduler never
//! hands out, as express landings do (see `crate::land::express`), and borrows whatever cores
//! are free when it starts, up to [`MOST_CORES`] in all. A machine with no express cores keeps
//! the scheduler's launcher.

use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ignite_core::protocol::Usage;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};
use ignite_os::proc::Processes;

use crate::land::express::{CoreLender, Loan, MOST_CORES};

/// How often a running command is looked at.
const LOOK_EVERY: Duration = Duration::from_millis(200);

/// Runs each command at once as a child process group on the express cores (see the module
/// documentation), its output in `log`, which each run replaces.
pub struct ExpressLauncher {
    /// The machine's `express_cores`.
    pub cores: u32,
    /// Where free cores are borrowed from, if anywhere.
    pub lender: Option<Arc<dyn CoreLender>>,
    pub log: PathBuf,
}

impl ExpressLauncher {
    /// The cores a command that asks for `wanted` runs on: up to the express cores, plus a loan
    /// of free cores up to [`MOST_CORES`] when it wants more.
    fn cores_for(&self, wanted: u32) -> (u32, Option<Loan>) {
        let own = wanted.min(self.cores).max(1);
        let more = wanted.min(MOST_CORES).saturating_sub(own);
        let loan = (more > 0)
            .then(|| self.lender.as_ref().and_then(|lender| lender.borrow(more)))
            .flatten();
        (own + loan.as_ref().map_or(0, |loan| loan.cores), loan)
    }
}

impl Launcher for ExpressLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        let (cores, loan) = self.cores_for(spec.cores);
        let result = run_group(spec, &self.log, cores);
        if let (Some(lender), Some(loan)) = (&self.lender, loan) {
            lender.give_back(&loan);
        }
        result
    }
}

/// Runs `spec` as a process group on `cores` cores, its output in `log`, until it exits or runs
/// past its hang limit.
fn run_group(spec: &CommandSpec, log: &std::path::Path, cores: u32) -> RunResult {
    let failed = |message: String| {
        let _ = std::fs::write(log, &message);
        RunResult {
            exit: Some(127),
            output: log.to_path_buf(),
            usage: Usage::default(),
            hung: false,
        }
    };
    if let Some(parent) = log.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return failed(format!("cannot create {}: {error}", parent.display()));
    }
    let (out, err) = match File::create(log).and_then(|out| Ok((out.try_clone()?, out))) {
        Ok((out, err)) => (out, err),
        Err(error) => return failed(format!("cannot write {}: {error}", log.display())),
    };
    let Some((program, args)) = spec.args.split_first() else {
        return failed("an empty command".to_owned());
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        // No jobserver of the daemon's is inherited: the command's own thread counts below say
        // how many cores it uses.
        .env_remove("CARGO_MAKEFLAGS")
        .env_remove("MAKEFLAGS")
        .env_remove("MFLAGS");
    let cores = cores.to_string();
    for name in ["CARGO_BUILD_JOBS", "IGNITE_CORES"] {
        command.env(name, &cores);
    }
    for (name, value) in &spec.env {
        command.env(name, value);
    }
    let native = ignite_os::Native;
    let mut group = match native.spawn_group(&mut command) {
        Ok(group) => group,
        Err(error) => return failed(format!("cannot start {program}: {error}")),
    };
    let limit = spec.hang_limit_s.map(Duration::from_secs);
    let began = Instant::now();
    let (exit, hung) = loop {
        match group.child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) => {}
            Err(_) => break (None, false),
        }
        if limit.is_some_and(|limit| began.elapsed() > limit) {
            let _ = native.end_group(&mut group);
            let _ = group.child.wait();
            break (None, true);
        }
        std::thread::sleep(LOOK_EVERY);
    };
    RunResult {
        exit,
        output: log.to_path_buf(),
        usage: Usage::default(),
        hung,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Lends what it is asked for, up to `free`, and records what it was asked.
    struct Lender {
        free: u32,
        asked: Mutex<Vec<u32>>,
        returned: Mutex<Vec<u64>>,
    }

    impl CoreLender for Lender {
        fn borrow(&self, wanted: u32) -> Option<Loan> {
            self.asked.lock().unwrap().push(wanted);
            (self.free > 0).then(|| Loan {
                grant: 7,
                cores: wanted.min(self.free),
            })
        }
        fn give_back(&self, loan: &Loan) {
            self.returned.lock().unwrap().push(loan.grant);
        }
    }

    fn launcher(free: u32) -> (ExpressLauncher, Arc<Lender>) {
        let lender = Arc::new(Lender {
            free,
            asked: Mutex::default(),
            returned: Mutex::default(),
        });
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-direct-{}", std::process::id()));
        let launcher = ExpressLauncher {
            cores: 3,
            lender: Some(lender.clone()),
            log: dir.join(format!("{free}.log")),
        };
        (launcher, lender)
    }

    #[test]
    fn a_command_wanting_the_machine_borrows_the_free_cores_beyond_the_express_ones() {
        let (launcher, lender) = launcher(4);
        let (cores, loan) = launcher.cores_for(16);
        assert_eq!(cores, 7);
        assert_eq!(*lender.asked.lock().unwrap(), vec![MOST_CORES - 3]);
        assert!(loan.is_some());
    }

    #[test]
    fn a_one_core_command_borrows_nothing() {
        let (launcher, lender) = launcher(4);
        let (cores, loan) = launcher.cores_for(1);
        assert_eq!(cores, 1);
        assert!(loan.is_none());
        assert!(lender.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn with_no_free_cores_the_command_runs_on_the_express_cores() {
        let (launcher, _) = launcher(0);
        assert_eq!(launcher.cores_for(16).0, 3);
    }

    #[cfg(unix)]
    #[test]
    fn a_command_runs_with_its_environment_and_the_loan_is_given_back() {
        let (launcher, lender) = launcher(2);
        let result = launcher.run(&CommandSpec {
            args: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "echo $CARGO_BUILD_JOBS $WHO; exit 3".to_owned(),
            ],
            cwd: ignite_core::test_files::temp_dir(),
            env: vec![("WHO".to_owned(), "release".to_owned())],
            class: ignite_core::protocol::Class::Agent,
            cores: 16,
            resources: Vec::new(),
            grows: true,
            tree: None,
            ticket: None,
            hang_limit_s: Some(60),
            reserve: None,
        });
        assert_eq!(result.exit, Some(3));
        assert!(!result.hung);
        let text = std::fs::read_to_string(&result.output).unwrap();
        assert_eq!(text.trim(), "5 release");
        assert_eq!(*lender.returned.lock().unwrap(), vec![7]);
    }
}
