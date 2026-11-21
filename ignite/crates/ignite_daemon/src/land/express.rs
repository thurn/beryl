//! Express landings: a landing that changes only Ignite runs a few fixed commands on cores the
//! scheduler never hands out, instead of the gate (see `[express]` in `ignite.toml` and
//! "Express landings" in the design document).
//!
//! On 2026-11-18 every fix to Ignite's own scheduling waited 10 to 30 minutes in the queue it
//! was fixing: the gate's checks and builds asked the scheduler for cores, the scheduler had
//! granted every core to other work, and the bug being fixed was often why. Three of those fixes
//! were pushed to the release ref by hand instead. So a landing whose every changed file
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
//! An express run that passes is remembered for an hour by the git tree ID of what it checked
//! and a digest of the `[express]` section (see [`PassMemory`]). Agents run `ignite check`
//! and then `ignite land`, and on 2026-11-18 that made each Ignite change pay for the same three
//! commands twice on a three-core lane: 17 checks averaging 213 s beside 30 landings. A run in a
//! tree with the same tree ID and section skips the commands and says so on its `express` line.
//! An edited section runs the commands. A dirty tree's tree ID is the one a commit of all its
//! files would have (see [`committed::snapshot`]): agents run `ignite test` before they commit,
//! and on 2026-11-19 two Ignite fixes each reran the commands once or twice after a passing
//! `ignite test`, waiting 220 s and 379 s on Ignite.
//!
//! A pass is also remembered with the tree it ran in, the commit it was at and the change's own
//! files. On 2026-11-18 a landing reran all four commands (56 s) a minute after `ignite check`
//! passed them, because a commit had landed meanwhile and the rebase gave the tree a new tree ID.
//! So when the tree ID misses, a landing reuses the tree's last pass under the rule that carries
//! a pass over a lost race (see [`Expressed`]): the change's own files are the same, and the
//! commits landed since that pass touch none of them and no build input.
//!
//! The landing then takes the integration branch's lease and pushes as any landing does, and
//! moves the release ref on the commands' pass (see [`super::release`]). A machine whose
//! `express_cores` is zero has no express lane, and its landings all go through the gate.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;

use ignite_core::config::Config;
use ignite_core::ids::{Commit, TreeId};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::traits::{Clock, Git};
use ignite_git::{committed, worktree};
use ignite_os::proc::Processes;
use ignite_store::local::express_passes;

use ignite_core::protocol::Class;

use super::env::GateTree;
use super::{Landing, line};
use crate::checks::verbs::render;
use crate::sched::{GrantRequest, Scheduler};
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

/// The most cores one express run uses in all: its reserved cores plus whatever the scheduler
/// has free when the run starts. An express run took about 164 s on 3 cores under load and
/// about 65 s on 12.
pub const MOST_CORES: u32 = 12;

/// How often a running command is looked at, for its exit and for `ignite stop`.
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// How many lines from the end of the output a failure shows.
const TAIL_LINES: usize = 40;

/// Cores borrowed from the scheduler for one express run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loan {
    /// What [`CoreLender::give_back`] needs to return the cores.
    pub grant: u64,
    pub cores: u32,
}

/// Where an express run borrows extra cores from: the scheduler itself in the daemon
/// ([`SchedulerLender`]), or the daemon over the host protocol in a verb process, where
/// landings run.
pub trait CoreLender: Send + Sync {
    /// Takes up to `wanted` of the cores that are free now without waiting; `None` when none
    /// are, or when the daemon cannot be asked.
    fn borrow(&self, wanted: u32) -> Option<Loan>;
    /// Returns the cores of a loan.
    fn give_back(&self, loan: &Loan);
}

/// [`CoreLender`] over the scheduler, for express runs in the daemon's own process.
pub struct SchedulerLender(pub Arc<Scheduler>);

impl CoreLender for SchedulerLender {
    fn borrow(&self, wanted: u32) -> Option<Loan> {
        let grant = self.0.try_borrow(&GrantRequest {
            class: Class::Land,
            cores: wanted,
            resources: Vec::new(),
            memory_mb: None,
            shrinks: false,
            grows: false,
            cores_file: None,
            reserve: None,
            priority_ticket: None,
        })?;
        Some(Loan {
            grant: grant.id,
            cores: grant.cores,
        })
    }

    fn give_back(&self, loan: &Loan) {
        self.0.release_id(loan.grant);
    }
}

/// The express tree and its commands, as a landing uses them. Tests replace it with a fake.
pub trait ExpressLane: Send + Sync {
    /// The fixed tree the commands run in.
    fn tree(&self) -> GateTree;
    /// Checks `commit` out in the tree and runs the commands there, waiting for any other
    /// express run on the machine to finish first.
    fn run(&self, commit: &Commit) -> ExpressRun;
    /// Runs the commands in `tree` as it is, with no checkout and no lock: a landing in its
    /// ticket tree, and `ignite test` and `ignite check` in an agent's tree. It runs the
    /// commands and directory of `express`, the section of that tree's own `ignite.toml`
    /// (see [`section_in`]), not the lane's own.
    /// `changed` is the change's own files, kept with a pass so that a later landing can
    /// judge whether the pass still holds after a rebase (see [`reused`]).
    fn run_in(
        &self,
        tree: &Path,
        express: &ignite_core::config::Express,
        changed: &[String],
    ) -> ExpressRun;
    /// The last pass of the commands of `express` remembered for `tree` within the hour, with
    /// where it ran; `None` when there is none or the lane keeps no memory.
    fn earlier_pass(
        &self,
        _tree: &Path,
        _express: &ignite_core::config::Express,
    ) -> Option<EarlierPass> {
        None
    }
}

/// An express pass remembered for a working tree: the commit it ran on, the change's own files
/// there and how many seconds ago it passed.
#[derive(Clone, Debug, PartialEq)]
pub struct EarlierPass {
    pub head: Commit,
    pub changed: Vec<String>,
    pub ago_s: f64,
}

/// What an express run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpressRun {
    /// Each command that ran, with its seconds, in order.
    pub ran: Vec<(String, f64)>,
    /// The command that failed, or the step before the commands, with why: the end of its
    /// output. `None` when every command passed.
    pub failure: Option<(String, String)>,
    /// The threads each command was given: the reserved cores plus any borrowed.
    pub cores: u32,
    /// The run passed on an earlier pass of the landing and was carried over to this one,
    /// because the commits landed since touch nothing it depends on (see [`Expressed`]).
    pub carried: bool,
    /// When the commands were not run because they had passed for this same tree, how many
    /// seconds ago that was.
    pub reused_after_s: Option<f64>,
    /// When the pass reused was on another commit of the tree, that commit: the commits landed
    /// since touch nothing the pass depends on (see [`reused`]).
    pub reused_on: Option<Commit>,
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
        if let (Some(ago), Some(on)) = (self.reused_after_s, &self.reused_on) {
            return line(
                "express",
                format!(
                    "passed for this tree {} ago on {}; the commits landed since touch none of \
                     its files, so the commands did not run again, in place of {what}",
                    ago_text(ago),
                    super::short(on)
                ),
            );
        }
        if let Some(ago) = self.reused_after_s {
            return line(
                "express",
                format!(
                    "passed for this tree {} ago, so the commands did not run again, in place \
                     of {what}",
                    ago_text(ago)
                ),
            );
        }
        let names: Vec<&str> = self
            .ran
            .iter()
            .map(|(command, _)| command.as_str())
            .collect();
        let seconds: f64 = self.ran.iter().map(|(_, s)| s).sum();
        let when = if self.carried {
            ", on an earlier pass"
        } else {
            ""
        };
        line(
            "express",
            format!(
                "{} commands passed in {seconds:.0} s on {} cores{when}, in place of \
                 {what}: {}",
                self.ran.len(),
                self.cores,
                names.join("; ")
            ),
        )
    }
}

/// `seconds` as `45 s` or `4 min`.
fn ago_text(seconds: f64) -> String {
    if seconds < 90.0 {
        format!("{seconds:.0} s")
    } else {
        format!("{:.0} min", seconds / 60.0)
    }
}

impl Landing {
    /// The express lane, when this landing may take it: the machine has one, the repository
    /// configures the express commands and a release ref to move, and every file the
    /// landing changes matches `[express] paths`.
    ///
    /// The section judged is that of the `ignite.toml` in `tree`, the ticket tree the rebase
    /// has put at the commit that would land, so the paths and commands are those that land
    /// with it. It falls back to the daemon's when the tree has no readable one.
    pub(super) fn express_for(
        &self,
        ctx: &Context,
        tree: &Path,
        changed: &[String],
    ) -> Option<(&dyn ExpressLane, ignite_core::config::Express)> {
        let lane = self.express.as_deref()?;
        let express = section_in(tree, ctx.config.express.as_ref())?;
        let repository = &ctx.config.repository;
        let releases = repository.release.is_some() && repository.release_family.is_some();
        (releases && !express.commands.is_empty() && express.covers(changed))
            .then_some((lane, express))
    }
}

/// The `[express]` section of the `ignite.toml` in `tree`, or `fallback` (the daemon's section)
/// when the tree has no such file, it does not parse or it has no `[express]`.
///
/// On 2026-11-18 a tree started before a commit that changed the express commands and added a
/// file they read failed every `ignite check` with `profile 'express' not found`: the daemon
/// had loaded the newer `ignite.toml` but the commands ran in the older tree.
pub fn section_in(
    tree: &Path,
    fallback: Option<&ignite_core::config::Express>,
) -> Option<ignite_core::config::Express> {
    fs::read_to_string(tree.join("ignite.toml"))
        .ok()
        .and_then(|text| Config::express_section(&text))
        .or_else(|| fallback.cloned())
}

/// Files that make a passing express run stale when another change lands them, even though
/// the change's own files are untouched: the manifests and lock file of the workspace the
/// commands build, and the toolchain and Cargo configuration it builds with.
const BUILD_INPUTS: [&str; 4] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
];

/// The last express run of a landing that passed, and what it ran on.
///
/// On 2026-11-18, after express landings began to run side by side, they raced each other for
/// the tip: L-371 took 4 passes and L-372 3, and each pass ran every command again (70 to 230 s)
/// because another express landing had pushed meanwhile. A pass that loses the race now runs the
/// commands again only when the commits landed since touch one of the change's own files or a
/// build input ([`BUILD_INPUTS`], or a `.cargo` directory) in the commands' directory, and
/// otherwise pushes the rebased commit on the earlier run's pass. The risk is a semantic conflict
/// between two Ignite changes that touch different files (one renames a function the other
/// calls): both land, and tip verification of the `ignite` family catches it.
pub(super) struct Expressed {
    /// The rebased commit the commands passed on.
    head: Commit,
    /// The change's own files on that commit.
    changed: Vec<String>,
    run: ExpressRun,
}

impl Expressed {
    pub(super) fn new(head: Commit, changed: Vec<String>, run: ExpressRun) -> Self {
        Self { head, changed, run }
    }

    /// The run the pass at `head`, whose own files are `changed`, may push on without running
    /// the commands, when the earlier run [`still_holds`] there.
    pub(super) fn carried(
        &self,
        ctx: &Context,
        head: &Commit,
        changed: &[String],
    ) -> ignite_core::traits::Result<Option<ExpressRun>> {
        if !still_holds(
            ctx.git.as_ref(),
            ctx.config.express.as_ref(),
            (&self.head, &self.changed),
            head,
            changed,
        )? {
            return Ok(None);
        }
        Ok(Some(ExpressRun {
            carried: true,
            ..self.run.clone()
        }))
    }
}

/// Whether an express pass on the commit `earlier.0`, whose change's own files were
/// `earlier.1`, still holds for `head`, whose own files are `changed`. It does not when the
/// files differ, or when the commits between the two touch one of them or a build input of the
/// workspace in the commands' directory of `express` (see [`is_build_input`]). The order of the
/// files does not matter.
///
/// A file of the change that the landed commits touch too does not count when it matches the
/// section's `inert` patterns: no command builds it, or a command only checks its formatting,
/// and the landing formats it after its first rebase. The rebase merged the two edits without a
/// conflict, so the commands would build the same code as before. Almost every Ignite change
/// edits the design document and the efficiency log, so without this rule any landing that
/// rebased over another Ignite change ran all four commands again (65 to 71 s on 2026-11-19).
/// A merge of two well formatted Markdown edits that a later race brings in is not formatted
/// again, and tip verification's lint family would report one that came out wrong.
fn still_holds(
    git: &dyn Git,
    express: Option<&ignite_core::config::Express>,
    earlier: (&Commit, &[String]),
    head: &Commit,
    changed: &[String],
) -> ignite_core::traits::Result<bool> {
    let sorted = |files: &[String]| {
        let mut files = files.to_vec();
        files.sort();
        files
    };
    if sorted(earlier.1) != sorted(changed) {
        return Ok(false);
    }
    let dir = express.map_or("", |express| express.dir.trim_matches('/'));
    let inert = |file: &str| express.is_some_and(|express| express.is_inert(file));
    let moved = git.changed(earlier.0, head)?;
    Ok(!moved
        .iter()
        .any(|file| (changed.contains(file) && !inert(file)) || is_build_input(file, dir)))
}

/// The run a landing at `head`, whose own files are `changed`, may push on because the commands
/// of `express` passed in its tree `tree` on another commit: usually `ignite check` before a
/// commit landed and the rebase moved the tree. `None` when the tree has no such pass, when the
/// pass was on `head` itself (then [`ExpressLane::run_in`] finds it by tree ID), or when it no
/// longer [`still_holds`].
pub(super) fn reused(
    lane: &dyn ExpressLane,
    git: &dyn Git,
    express: &ignite_core::config::Express,
    tree: &Path,
    head: &Commit,
    changed: &[String],
) -> ignite_core::traits::Result<Option<ExpressRun>> {
    let Some(earlier) = lane.earlier_pass(tree, express) else {
        return Ok(None);
    };
    if earlier.head == *head
        || !still_holds(
            git,
            Some(express),
            (&earlier.head, &earlier.changed),
            head,
            changed,
        )?
    {
        return Ok(None);
    }
    Ok(Some(ExpressRun {
        reused_after_s: Some(earlier.ago_s),
        reused_on: Some(earlier.head),
        ..ExpressRun::default()
    }))
}

/// Whether `file` is a build input of the workspace in `dir`: a manifest or lock file in it, or
/// a toolchain file or Cargo configuration anywhere, since Cargo also reads those from the
/// directories above the workspace.
pub(super) fn is_build_input(file: &str, dir: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    let in_dir = dir.is_empty() || file.starts_with(&format!("{dir}/"));
    let anywhere =
        name.starts_with("rust-toolchain") || file.split('/').any(|part| part == ".cargo");
    anywhere || (in_dir && BUILD_INPUTS.contains(&name))
}

/// The reply to an express run that failed: exit code 2, the command and the end of its
/// output, and how to run the same command in the ticket tree.
pub(super) fn failed_express(dir: &str, run: &ExpressRun) -> Reply {
    let (command, output) = run.failure.clone().unwrap_or_default();
    let rerun = rerun_command(&command, &output);
    let mut lines = vec![line(
        "failed",
        format!("the express command `{command}` failed; nothing landed"),
    )];
    lines.extend(output.lines().map(|text| line("", text.to_owned())));
    lines.push(line("rerun", format!("cd {dir} && {rerun}, in your tree")));
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
    /// The runs that passed, when the daemon has a database to keep them in.
    pub memory: Option<PassMemory>,
    /// Where to ask for extra cores when a run starts, if anywhere.
    pub lender: Option<Arc<dyn CoreLender>>,
}

impl MachineExpress {
    /// The machine's express lane, or `None` when the repository configures no express
    /// commands or the machine reserves no cores for them.
    pub fn for_config(
        config: &Config,
        repo: &Path,
        cores: u32,
        db: Option<Arc<Mutex<Connection>>>,
        clock: &Arc<dyn Clock>,
        lender: Option<Arc<dyn CoreLender>>,
    ) -> Option<Arc<dyn ExpressLane>> {
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
            memory: db.map(|db| PassMemory::new(db, clock.clone())),
            lender,
        }))
    }

    /// Asks the lender, without waiting, for the cores that are free now, up to [`MOST_CORES`]
    /// less the reserved ones. `None` when it has none or the lane has no lender: the reserved
    /// cores are the floor, so a saturated or broken scheduler cannot hold the run.
    fn borrow(&self) -> Option<Loan> {
        let wanted = MOST_CORES.saturating_sub(self.cores);
        if wanted == 0 {
            return None;
        }
        self.lender.as_ref()?.borrow(wanted)
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
    fn run_commands(&self, tree: &Path, dir: &str, commands: &[String], run: &mut ExpressRun) {
        // A seed swaps the target directory in once no build holds Cargo's lock, which
        // cargo-nextest gives up while the tests run (see `trees::wait_for_seed_on_disk`).
        if let Err(why) = crate::trees::wait_for_seed_on_disk(tree, HANG_LIMIT) {
            run.failure = Some(("waiting for the tree's seed".to_owned(), why));
            return;
        }
        let dir = tree.join(dir);
        let target = dir.join("target");
        let log = target.join("express.log");
        if let Err(error) = fs::create_dir_all(&target).and_then(|()| File::create(&log)) {
            run.failure = Some((
                "writing the express log".to_owned(),
                format!("{}: {error}", log.display()),
            ));
            return;
        }
        // The grant is held for the whole run and released when it ends, however it ends.
        let borrowed = self.borrow();
        let cores = self.cores + borrowed.as_ref().map_or(0, |loan| loan.cores);
        run.cores = cores;
        let _release = borrowed.map(|loan| Release {
            lender: self.lender.clone().expect("a loan came from a lender"),
            loan,
        });
        for command in commands {
            steps::step(format!("running `{command}` on {cores} cores"));
            let began = Instant::now();
            let result = Self::run_one(command, &dir, &log, cores);
            run.ran
                .push((command.clone(), began.elapsed().as_secs_f64()));
            if let Err(why) = result {
                run.failure = Some((command.clone(), why));
                return;
            }
        }
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

/// Gives borrowed cores back to the lender when dropped.
struct Release {
    lender: Arc<dyn CoreLender>,
    loan: Loan,
}

impl Drop for Release {
    fn drop(&mut self) {
        self.lender.give_back(&self.loan);
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
        self.run_commands(&self.tree, &self.dir, &self.commands, &mut run);
        run
    }

    fn run_in(
        &self,
        tree: &Path,
        express: &ignite_core::config::Express,
        changed: &[String],
    ) -> ExpressRun {
        let mut run = ExpressRun {
            cores: self.cores,
            ..ExpressRun::default()
        };
        // The tree ID of what is on disk, committed or not, says exactly what was checked.
        let snapshot = self
            .memory
            .as_ref()
            .and_then(|_| committed::snapshot(tree).ok());
        let section = PassMemory::digest(express, self.cores);
        if let (Some(memory), Some(snapshot)) = (&self.memory, &snapshot)
            && let Some(ago) = memory.passed_ago(&snapshot.tree_id, &section)
        {
            run.reused_after_s = Some(ago);
            return run;
        }
        // No lock: each tree has its own target directory, so runs in different trees share
        // nothing, and a lock made every landing and check wait for every other one.
        self.run_commands(tree, &express.dir, &express.commands, &mut run);
        if let (Some(memory), Some(snapshot)) = (&self.memory, &snapshot)
            && run.passed()
        {
            // A dirty tree's commit holds what was on disk, for a landing after a rebase.
            let place = express_passes::Place {
                tree_path: tree.to_string_lossy().into_owned(),
                head: snapshot.commit.as_str().to_owned(),
                changed: changed.to_vec(),
            };
            memory.remember(&snapshot.tree_id, &section, Some(&place));
        }
        run
    }

    fn earlier_pass(
        &self,
        tree: &Path,
        express: &ignite_core::config::Express,
    ) -> Option<EarlierPass> {
        let section = PassMemory::digest(express, self.cores);
        self.memory.as_ref()?.last_in(tree, &section)
    }
}

/// The last [`TAIL_LINES`] lines the log `file` holds past byte `start`.
fn tail(mut file: File, start: u64) -> String {
    let mut text = String::new();
    let _ = file.seek(SeekFrom::Start(start));
    let _ = file.read_to_string(&mut text);
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(TAIL_LINES);
    let shown = lines[from..].join("\n");
    if shown.trim().is_empty() {
        "it failed with no output".to_owned()
    } else {
        shown
    }
}

#[path = "express_memory.rs"]
mod memory;
pub use memory::PassMemory;

#[path = "express_rerun.rs"]
mod rerun;
pub use rerun::rerun_command;

#[cfg(all(test, unix))]
#[path = "express_tests.rs"]
mod tests;
