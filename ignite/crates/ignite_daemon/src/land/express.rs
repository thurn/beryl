//! Express landings: a landing that changes only Ignite runs a few fixed commands on cores the
//! scheduler never hands out, instead of the gate (see `[express]` in `ignite.toml` and
//! "Express landings" in the design document).
//!
//! On 2026-11-18 every fix to Ignite's own scheduling waited 10 to 30 minutes in the queue it
//! was fixing: the gate's checks and builds asked the scheduler for cores, the scheduler had
//! granted every core to other work, and the bug being fixed was often why. Three of those fixes
//! were pushed to the release branch by hand instead. So a landing whose every changed file
//! matches `[express] paths` skips the gate. It checks the rebased commit out in one fixed
//! worktree, `[express] tree`, whose ignored files (Ignite's Cargo target directory) survive
//! from one landing to the next, and runs `[express] commands` there in order. They run as the
//! daemon's own child processes, never through a supervisor or the scheduler, with the
//! machine's `express_cores` as their thread count. The scheduler leaves those cores out of
//! what it hands out (see `crate::sched::Limits::for_machine`), so a saturated or broken
//! scheduler cannot hold them. One express run goes at a time on a machine: each holds an
//! exclusive lock on a file beside the tree, because each landing runs in a verb process of its
//! own.
//!
//! `ignite test` and `ignite check` use the same commands, cores and lock for a tree that changes
//! only Ignite (see `crate::checks::express`), but run them in the agent's own tree with no
//! checkout ([`ExpressLane::run_in`]). On 2026-11-18 `ignite test` of Ignite's own checks took 5
//! to 8.5 minutes while other builds held the scheduler's cores, against about a minute when it
//! got them, and that edit-and-test loop was most of a fix's wall time.
//!
//! The landing then takes the integration branch's lease and pushes as any landing does, and
//! moves the release branch on the commands' pass (see [`super::release`]). A machine whose
//! `express_cores` is zero has no express lane, and its landings all go through the gate.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;

use ignite_core::config::Config;
use ignite_core::ids::{Commit, TreeId};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_git::worktree;
use ignite_os::proc::Processes;

use super::env::GateTree;
use super::{Landing, line};
use crate::checks::verbs::render;
use crate::server::Context;
use crate::steps;

/// The branch the express tree keeps checked out.
const BRANCH: &str = "ignite-tree/express";

/// The tree's name in the landing's output.
const TREE_NAME: &str = "express";

/// A command still running after this long is taken to hang and is ended. Ignite's own test
/// build and run took 2 to 3 minutes on three cores after a change to the daemon, with the
/// target directory warm, and a cold one a few minutes.
const HANG_LIMIT: Duration = Duration::from_mins(30);

/// How often a running command is looked at, for its exit and for `ignite stop`.
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// How many lines from the end of the output a failure shows.
const TAIL_LINES: usize = 40;

/// The express tree and its commands, as a landing uses them. Tests replace it with a fake.
pub trait ExpressLane: Send + Sync {
    /// The fixed tree the commands run in.
    fn tree(&self) -> GateTree;
    /// Checks `commit` out in the tree and runs the commands there, waiting for any other
    /// express run on the machine to finish first.
    fn run(&self, commit: &Commit) -> ExpressRun;
    /// Runs the commands in `tree` as it is, with no checkout and no lock: a landing in its
    /// ticket tree, and `ignite test` and `ignite check` in an agent's tree.
    fn run_in(&self, tree: &Path) -> ExpressRun;
}

/// What an express run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpressRun {
    /// Each command that ran, with its seconds, in order.
    pub ran: Vec<(String, f64)>,
    /// The command that failed, or the step before the commands, with why: the end of its
    /// output. `None` when every command passed.
    pub failure: Option<(String, String)>,
    /// The threads each command was given.
    pub cores: u32,
}

impl ExpressRun {
    pub fn passed(&self) -> bool {
        self.failure.is_none()
    }

    /// The landing's `express` line.
    pub fn line(&self) -> (String, String) {
        self.line_in_place_of("the gate")
    }

    /// The `express` line, saying that the commands ran in place of `what`.
    pub fn line_in_place_of(&self, what: &str) -> (String, String) {
        let names: Vec<&str> = self
            .ran
            .iter()
            .map(|(command, _)| command.as_str())
            .collect();
        let seconds: f64 = self.ran.iter().map(|(_, s)| s).sum();
        line(
            "express",
            format!(
                "{} commands passed in {seconds:.0} s on {} reserved cores, in place of {what}: {}",
                self.ran.len(),
                self.cores,
                names.join("; ")
            ),
        )
    }
}

impl Landing {
    /// The express lane, when this landing may take it: the machine has one, the repository
    /// configures the express commands and a release branch to move, and every file the
    /// landing changes matches `[express] paths`.
    pub(super) fn express_for(
        &self,
        ctx: &Context,
        changed: &[String],
    ) -> Option<&dyn ExpressLane> {
        let lane = self.express.as_deref()?;
        let express = ctx.config.express.as_ref()?;
        let repository = &ctx.config.repository;
        let releases = repository.release.is_some() && repository.release_family.is_some();
        (releases && !express.commands.is_empty() && express.covers(changed)).then_some(lane)
    }
}

/// The reply to an express run that failed: exit code 2, the command and the end of its
/// output, and how to run the same command in the ticket tree.
pub(super) fn failed_express(ctx: &Context, run: &ExpressRun) -> Reply {
    let dir = ctx.config.express.as_ref().map_or("", |e| e.dir.as_str());
    let (command, output) = run.failure.clone().unwrap_or_default();
    let mut lines = vec![line(
        "failed",
        format!("the express command `{command}` failed; nothing landed"),
    )];
    lines.extend(output.lines().map(|text| line("", text.to_owned())));
    lines.push(line(
        "rerun",
        format!("cd {dir} && {command}, in your tree"),
    ));
    lines.push(line(
        "next",
        "fix it in your tree; nothing landed and the tree stays rebased",
    ));
    Reply {
        exit: Exit::CheckFailed,
        text: render(&lines),
        json: Some(json!({ "express": { "command": command, "output": output } })),
        handle: None,
    }
}

/// The express lane of a machine: the fixed worktree under the checkout and the commands.
pub struct MachineExpress {
    /// The repository's main checkout.
    pub repo: PathBuf,
    /// The fixed worktree, absolute.
    pub tree: PathBuf,
    /// The directory the commands run in, relative to the tree.
    pub dir: String,
    pub commands: Vec<String>,
    /// The machine's `express_cores`.
    pub cores: u32,
}

impl MachineExpress {
    /// The machine's express lane, or `None` when the repository configures no express
    /// commands or the machine reserves no cores for them.
    pub fn for_config(config: &Config, repo: &Path, cores: u32) -> Option<Arc<dyn ExpressLane>> {
        let express = config.express.as_ref()?;
        if cores == 0 || express.commands.is_empty() {
            return None;
        }
        Some(Arc::new(MachineExpress {
            repo: repo.to_path_buf(),
            tree: repo.join(&express.tree),
            dir: express.dir.clone(),
            commands: express.commands.clone(),
            cores,
        }))
    }

    /// Takes the exclusive lock on the file beside the tree, held until the file is dropped,
    /// so that two express runs never share the tree or the cores. `Err` says why it was not taken.
    fn lock(&self) -> Result<File, String> {
        let path = self.tree.with_extension("lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let _waiting = steps::waiting("the express cores, which another express run is using");
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(e)) => {
                    return Err(format!("cannot lock {}: {e}", path.display()));
                }
            }
            if steps::stop_requested() {
                return Err("the run was stopped".to_owned());
            }
            std::thread::sleep(LOOK_EVERY);
        }
    }

    /// Puts the tree at `commit`, keeping the commands' directory's `target` (Cargo's build
    /// directory) so the next build in it is incremental.
    fn check_out(&self, commit: &Commit) -> Result<(), String> {
        let keep = [format!("{}/target", self.dir.trim_matches('/'))];
        let done = if self.tree.join(".git").exists() {
            worktree::reset(&self.tree, BRANCH, commit, &keep)
        } else {
            if let Some(parent) = self.tree.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            worktree::add(&self.repo, &self.tree, BRANCH, commit)
        };
        done.map_err(|error| error.to_string())
    }

    /// Runs the commands in order in `tree`'s commands directory, logging to `express.log` in
    /// its `target` directory, and records each in `run`; stops at the first failure.
    fn run_commands(&self, tree: &Path, run: &mut ExpressRun) {
        let dir = tree.join(&self.dir);
        let target = dir.join("target");
        let log = target.join("express.log");
        if let Err(error) = fs::create_dir_all(&target).and_then(|()| File::create(&log)) {
            run.failure = Some((
                "writing the express log".to_owned(),
                format!("{}: {error}", log.display()),
            ));
            return;
        }
        for command in &self.commands {
            steps::step(format!("running `{command}` on the express cores"));
            let began = Instant::now();
            let result = self.run_one(command, &dir, &log);
            run.ran
                .push((command.clone(), began.elapsed().as_secs_f64()));
            if let Err(why) = result {
                run.failure = Some((command.clone(), why));
                return;
            }
        }
    }

    /// Runs one command in `dir`, its output appended to `log`; `Err` holds why it failed.
    fn run_one(&self, command: &str, dir: &Path, log: &Path) -> Result<(), String> {
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
        let cores = self.cores.to_string();
        let mut process = Command::new(program);
        process
            .args(words)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            // The tree's own target directory is the one kept warm, whatever the daemon's
            // environment says, and no jobserver of the daemon's is inherited.
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
            Err(tail(log, start))
        }
    }
}

impl ExpressLane for MachineExpress {
    fn tree(&self) -> GateTree {
        GateTree {
            tree: TreeId::new(TREE_NAME).expect("`express` is a valid tree name"),
            path: self.tree.clone(),
        }
    }

    fn run(&self, commit: &Commit) -> ExpressRun {
        let mut run = ExpressRun {
            cores: self.cores,
            ..ExpressRun::default()
        };
        let _held = match self.lock() {
            Ok(file) => file,
            Err(why) => {
                run.failure = Some(("locking the express tree".to_owned(), why));
                return run;
            }
        };
        steps::step(format!("checking out {commit} in the express tree"));
        if let Err(why) = self.check_out(commit) {
            run.failure = Some(("checking out the express tree".to_owned(), why));
            return run;
        }
        self.run_commands(&self.tree, &mut run);
        run
    }

    fn run_in(&self, tree: &Path) -> ExpressRun {
        let mut run = ExpressRun {
            cores: self.cores,
            ..ExpressRun::default()
        };
        // No lock: each tree has its own target directory, so runs in different trees share
        // nothing, and a lock made every landing and check wait for every other one.
        self.run_commands(tree, &mut run);
        run
    }
}

/// The last [`TAIL_LINES`] lines `log` holds past byte `start`.
fn tail(log: &Path, start: u64) -> String {
    let mut text = String::new();
    if let Ok(mut file) = File::open(log) {
        let _ = file.seek(SeekFrom::Start(start));
        let _ = file.read_to_string(&mut text);
    }
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(TAIL_LINES);
    let shown = lines[from..].join("\n");
    if shown.trim().is_empty() {
        "it failed with no output".to_owned()
    } else {
        shown
    }
}

#[cfg(all(test, unix))]
#[path = "express_tests.rs"]
mod tests;
