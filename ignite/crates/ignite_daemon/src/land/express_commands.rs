//! Run the express commands in the caller's own build tree.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use ignite_core::protocol::host::ExpressBuildStage;
use ignite_os::proc::Processes as _;

use super::{ExpressRun, HANG_LIMIT, LOOK_EVERY, MachineExpress, Release, steps, tail};

impl MachineExpress {
    /// Runs the commands in order in `tree`'s commands directory, logging to `express.log` in
    /// its `target` directory, and records each in `run`; stops at the first failure.
    pub(super) fn run_commands(
        &self,
        tree: &Path,
        dir: &str,
        commands: &[String],
        run: &mut ExpressRun,
    ) {
        // A seed swaps the target directory in once no build holds Cargo's lock, which
        // cargo-nextest gives up while the tests run (see `trees::wait_for_seed_on_disk`).
        if let Err(why) = crate::trees::wait_for_seed_on_disk(tree, HANG_LIMIT) {
            run.failure = Some(("waiting for the tree's seed".to_owned(), why));
            return;
        }
        let target = Path::new(dir).join("target");
        for (index, command) in commands.iter().enumerate() {
            if let Some(builds) = &self.builds
                && let Err(error) = builds.express_build(tree, &target, ExpressBuildStage::Prepare)
            {
                run.failure = Some((
                    "preparing express build outputs".to_owned(),
                    error.to_string(),
                ));
                return;
            }
            let result = self.run_prepared(tree, dir, command, run, index == 0);
            if let Some(builds) = &self.builds {
                let stage = if result.is_ok() {
                    ExpressBuildStage::Passed
                } else {
                    ExpressBuildStage::Failed
                };
                if let Err(error) = builds.express_build(tree, &target, stage) {
                    eprintln!("ignite: cannot record express build outputs: {error}");
                }
            }
            if let Err(why) = result {
                run.failure = Some((command.clone(), why));
                return;
            }
        }
    }

    fn run_prepared(
        &self,
        tree: &Path,
        dir: &str,
        command: &str,
        run: &mut ExpressRun,
        first: bool,
    ) -> Result<(), String> {
        let tree_lock = crate::trees::command_lock(tree).map_err(|error| error.to_string())?;
        tree_lock.lock_shared().map_err(|error| error.to_string())?;
        let dir = tree.join(dir);
        let target = dir.join("target");
        let log = target.join("express.log");
        fs::create_dir_all(&target)
            .and_then(|()| {
                if first {
                    File::create(&log).map(|_| ())
                } else {
                    Ok(())
                }
            })
            .map_err(|error| format!("cannot write {}: {error}", log.display()))?;
        let borrowed = self.borrow();
        let cores = self.cores + borrowed.as_ref().map_or(0, |loan| loan.cores);
        run.cores = cores;
        let _release = borrowed.map(|loan| Release {
            lender: self.lender.clone().expect("a loan came from a lender"),
            loan,
        });
        steps::step(format!("running `{command}` on {cores} cores"));
        let began = Instant::now();
        let result = Self::run_one(command, &dir, &log, cores);
        run.ran
            .push((command.to_owned(), began.elapsed().as_secs_f64()));
        result
    }

    /// Runs one command in `dir`, its output appended to `log`; `Err` holds why it failed.
    fn run_one(command: &str, dir: &Path, log: &Path, cores: u32) -> Result<(), String> {
        let mut words = command.split_whitespace();
        let program = words.next().ok_or("an empty command")?;
        let mut out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| format!("cannot open {}: {e}", log.display()))?;
        let start = out.metadata().map_or(0, |m| m.len());
        let _ = writeln!(out, "$ {command}");
        let err = out.try_clone().map_err(|e| e.to_string())?;
        // Read back through this handle, not the path: a log moved away meanwhile, as a seed
        // once swapped a whole target directory, still holds what the command wrote.
        let written = File::open(log).map_err(|e| format!("cannot open {}: {e}", log.display()))?;
        let cores = cores.to_string();
        let mut process = Command::new(program);
        process
            .args(words)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            // The tree's own target directory is the one kept warm, whatever the daemon's
            // environment says, and no jobserver of the daemon's is inherited.
            .env("CARGO_LOG", "cargo::core::compiler::fingerprint=debug")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_MAKEFLAGS")
            .env_remove("MAKEFLAGS")
            .env_remove("MFLAGS");
        for name in [
            "CARGO_BUILD_JOBS",
            "RUST_TEST_THREADS",
            "NEXTEST_TEST_THREADS",
            "IGNITE_CORES",
        ] {
            process.env(name, &cores);
        }
        let native = ignite_os::Native;
        let mut group = native
            .spawn_group(&mut process)
            .map_err(|e| format!("cannot start `{command}`: {e}"))?;
        let began = Instant::now();
        let status = loop {
            match group.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => return Err(format!("cannot wait for `{command}`: {error}")),
            }
            let why = if steps::stop_requested() {
                Some("the run was stopped".to_owned())
            } else if began.elapsed() > HANG_LIMIT {
                Some(format!("it ran past {} s", HANG_LIMIT.as_secs()))
            } else {
                None
            };
            if let Some(why) = why {
                let _ = native.end_group(&mut group);
                let _ = group.child.wait();
                return Err(why);
            }
            std::thread::sleep(LOOK_EVERY);
        };
        if status.success() {
            Ok(())
        } else {
            Err(tail(written, start))
        }
    }
}
