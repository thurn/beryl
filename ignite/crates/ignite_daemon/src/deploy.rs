//! Whether a release of Ignite has reached the machines: the `release` section of
//! `ignite status` and `ignite wait deploy-<commit>`.
//!
//! When `ignite land` moves the release branch, each daemon finds the new tip at its next check
//! of the branch, builds it, runs its selftest and restarts into it, and the daemon on the hub's
//! machine restarts the hub first (see `main/selfupdate.rs`). None of that is reported to the
//! session that landed. This module shows it:
//!
//! - [`ReleaseSection`] prints, in `ignite status`, the release branch's tip, what this daemon
//!   runs and what it is doing about the difference (waiting for its next check of the branch,
//!   building, self-testing, restarting, failed to build, or rolled back with the reason from
//!   `bin/rollbacks.log`), the hub's version and each machine's.
//! - [`DeployWaiter`] answers `ignite wait deploy-<commit>`, the handle `ignite land` prints when
//!   it moves the release. It asks this daemon to check the branch at its next round instead of
//!   waiting for its regular check, then waits until this daemon, the hub and every machine the
//!   hub heard from in the last [`ACTIVE_WITHIN_S`] run that commit or a later one (exit 0), or
//!   the commit was rolled back or failed to build here (exit 2). A wait that ends first answers
//!   exit 5, or 4 when the hub could not be reached. The wait keeps no state, so a client whose
//!   connection dropped because this daemon restarted into the release sends it again.
//!
//! A machine other than this one that rolls the release back stays on its old build, which the
//! hub reports; the wait then names it as behind until it ends.

use std::sync::{Arc, OnceLock};

use serde_json::json;

use ignite_core::ids::Commit;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::fleet::FleetVersions;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Git, Hub, Result};

use crate::doctor::StatusSection;
use crate::loop_health::span;

/// What a deploy handle starts with; the rest is the commit, as in `deploy-<commit>`.
pub const HANDLE_PREFIX: &str = "deploy-";

/// A machine the hub has not heard from for longer than this is left out of a deploy: it is
/// switched off or cut off, and a wait on it would never end.
pub const ACTIVE_WITHIN_S: f64 = 600.0;

/// How long a deploy wait pauses between two looks.
pub const LOOK_EVERY_S: f64 = 2.0;

/// The handle `ignite wait` takes to wait for `commit` to reach every machine.
pub fn handle(commit: &Commit) -> String {
    format!("{HANDLE_PREFIX}{commit}")
}

/// The text of a landing's `release` line when it moved the release branch `branch` to
/// `commit`.
pub fn moved(branch: &str, commit: &Commit) -> String {
    format!("{branch} moved to {}", short(commit))
}

/// The handle `ignite wait` takes to wait for the release, when a landing's `release` line
/// `release` says that it moved the release branch, `branch` (none when the repository names
/// no release branch), to `commit`; `None` when it did not move.
pub fn landed_handle(branch: Option<&str>, release: &str, commit: &Commit) -> Option<String> {
    (release == moved(branch?, commit)).then(|| handle(commit))
}

/// The `next` line of a landing whose `release` line, if it had one, is `release`, and the
/// deploy handle when that line says the release branch `branch` moved to the landed `commit`.
pub fn landed_next(
    branch: Option<&str>,
    release: Option<&str>,
    commit: &Commit,
) -> (String, Option<String>) {
    match release.and_then(|release| landed_handle(branch, release, commit)) {
        Some(handle) => (
            format!(
                "ignite wait {handle} until every machine runs the release; then wait for your \
                 next ticket"
            ),
            Some(handle),
        ),
        None => ("nothing; wait for your next ticket".to_owned(), None),
    }
}

/// The commit a deploy handle names, or `None` for any other handle.
pub fn parse_handle(name: &str) -> Option<Commit> {
    Commit::new(name.strip_prefix(HANDLE_PREFIX)?).ok()
}

/// What a check of the release branch found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    /// The daemon runs the tip.
    Current,
    /// The tip changed nothing under Ignite's source, so the daemon stays.
    Unchanged,
    /// The tip does not contain the daemon's own commit, so going to it would be a downgrade.
    Ahead,
    /// The tip was rolled back or failed its selftest here.
    Rejected,
    /// The tip is to be built and restarted into.
    Build,
}

/// What the daemon is doing to get onto a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Work {
    Building(Commit),
    SelfTesting(Commit),
    /// Built and tested; the daemon restarts into it once its restart window allows.
    Restarting {
        commit: Commit,
        /// When the restart goes ahead if no newer release appears and no landing holds it.
        /// `None`, or a moment already past, means at the next heartbeat.
        due: Option<Timestamp>,
        /// The latest the restart can be held to, a push in progress aside.
        cap: Option<Timestamp>,
    },
    /// The build failed, for the reason given; it is not tried again until the branch moves.
    Failed(Commit, String),
}

/// One daemon's view of its own release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalRelease {
    /// The commit the daemon runs, or `None` for an unstored executable, which never updates.
    pub own: Option<Commit>,
    /// The branch releases come from.
    pub branch: String,
    /// When the daemon checks the branch next.
    pub next_check: Option<Timestamp>,
    pub work: Option<Work>,
}

/// The daemon's self-update, as this module reads it.
pub trait DeployState: Send + Sync {
    fn local(&self) -> LocalRelease;
    /// Fetches the release branch and says what the daemon makes of its tip, without acting
    /// on it.
    fn check(&self) -> Result<(Commit, Checked)>;
    /// Makes the daemon check the release branch at its next round, within seconds, instead of
    /// at its regular check.
    fn check_soon(&self);
    /// Whether `version` is `target` or a later commit that contains it.
    fn reached(&self, version: &Commit, target: &Commit) -> bool;
    /// Why `commit` was rolled back or failed its selftest on this machine, if it was.
    fn rejection(&self, commit: &Commit) -> Option<String>;
}

/// Says what this daemon runs and does about the release tip `tip`, which a check found to be
/// `checked`, if one was made.
fn local_line(
    state: &dyn DeployState,
    local: &LocalRelease,
    tip: &Commit,
    checked: Option<Checked>,
    now: Timestamp,
) -> String {
    let Some(own) = &local.own else {
        return "this daemon runs an unstored build, which never updates itself".to_owned();
    };
    let runs = format!("this daemon runs {}", short(own));
    if state.reached(own, tip) {
        return format!("{runs}, the release");
    }
    let doing = match &local.work {
        Some(Work::Building(commit)) => Some(format!("building {}", short(commit))),
        Some(Work::SelfTesting(commit)) => Some(format!("self-testing {}", short(commit))),
        Some(Work::Restarting { commit, due, cap }) => Some(restarting(commit, *due, *cap, now)),
        Some(Work::Failed(commit, reason)) if commit == tip => {
            Some(format!("{} failed to build: {reason}", short(commit)))
        }
        _ => None,
    };
    if let Some(doing) = doing {
        return format!("{runs}; {doing}");
    }
    if let Some(reason) = state.rejection(tip) {
        return format!("{runs}; rolled back {}: {reason}", short(tip));
    }
    match checked {
        Some(Checked::Unchanged) => format!(
            "{runs}; {} changed nothing under Ignite's source, so it stays",
            short(tip)
        ),
        Some(Checked::Ahead) => {
            format!("{runs}, which {} does not contain, so it stays", short(tip))
        }
        _ => {
            let due = local.next_check.map_or_else(
                || "soon".to_owned(),
                |at| {
                    let left = at.seconds_since(now);
                    if left <= 0.0 {
                        "now".to_owned()
                    } else {
                        format!("in {}", span(left))
                    }
                },
            );
            format!("{runs}; pending: it checks {} next {due}", local.branch)
        }
    }
}

/// Says when the daemon restarts into `commit`: at its next heartbeat once `due` has passed,
/// else when `due` comes, unless a newer release or a landing holds it, and at the latest at
/// `cap`.
fn restarting(
    commit: &Commit,
    due: Option<Timestamp>,
    cap: Option<Timestamp>,
    now: Timestamp,
) -> String {
    let short = short(commit);
    match due.map(|due| due.seconds_since(now)) {
        Some(left) if left > 0.0 => {
            let latest = cap.map_or_else(String::new, |cap| {
                format!(", at the latest in {}", span(cap.seconds_since(now)))
            });
            format!(
                "restarting into {short} in {} unless a newer release or a landing holds it{latest}",
                span(left)
            )
        }
        _ => format!("restarting into {short} at its next heartbeat"),
    }
}

fn short(commit: &Commit) -> &str {
    let text = commit.as_str();
    &text[..text.len().min(7)]
}

/// Whether a machine reports a version that has reached `target`, and how to say it.
fn version_line(state: &dyn DeployState, version: &str, target: &Commit) -> (bool, String) {
    match Commit::new(version) {
        Err(_) => (true, "an unstored build".to_owned()),
        Ok(version) if state.reached(&version, target) => {
            (true, format!("{}, the release", short(&version)))
        }
        Ok(version) => (false, format!("{}, behind the release", short(&version))),
    }
}

/// The machines the hub heard from in the last [`ACTIVE_WITHIN_S`], each with whether it runs
/// `target` and a line saying what it runs.
fn machine_lines(
    state: &dyn DeployState,
    fleet: &FleetVersions,
    target: &Commit,
    now: Timestamp,
) -> Vec<(bool, String)> {
    fleet
        .machines
        .iter()
        .map(|machine| {
            let silent = now.seconds_since(machine.last_seen);
            if silent > ACTIVE_WITHIN_S {
                return (
                    true,
                    format!(
                        "{} {}, not heard from for {}",
                        machine.name,
                        short_text(&machine.version),
                        span(silent)
                    ),
                );
            }
            let (reached, what) = version_line(state, &machine.version, target);
            (reached, format!("{} {what}", machine.name))
        })
        .collect()
}

fn short_text(version: &str) -> &str {
    &version[..version.len().min(7)]
}

/// `ignite status`'s `release` section.
pub struct ReleaseSection {
    pub state: Arc<dyn DeployState>,
    pub git: Arc<dyn Git>,
    /// The branch landings go to, which the release branch must not run ahead of.
    pub staging: String,
    pub hub: Arc<dyn Hub>,
    pub clock: Arc<dyn Clock>,
}

impl ReleaseSection {
    /// A warning when the release branch's tip `tip` holds changes to Ignite's source that the
    /// staging tip lacks, as a fix pushed straight to the release branch does. A landing moves
    /// the release branch only over a commit whose changes the landed commit holds, so while
    /// the fix stays off staging, no later landing can move the release. `None` when staging
    /// holds the tip or either branch cannot be read.
    fn stranded_line(&self, tip: &Commit) -> Option<String> {
        let staging = self.git.known_tip(&self.staging).ok()?;
        let held = self.git.is_ancestor(tip, &staging).ok()?
            || self
                .git
                .holds_changes(&staging, tip, crate::update::SOURCE)
                .ok()?;
        (!held).then(|| {
            format!(
                "warning: {} holds {}, which {} (at {}) lacks; no landing can move the release \
                 until it is pushed to {} too",
                self.state.local().branch,
                short(tip),
                self.staging,
                short(&staging),
                self.staging
            )
        })
    }
}

impl StatusSection for ReleaseSection {
    fn title(&self) -> &'static str {
        "release"
    }

    fn lines(&self) -> Vec<String> {
        let local = self.state.local();
        let now = self.clock.now();
        let (tip, checked) = match self.state.check() {
            Ok(checked) => checked,
            Err(error) => {
                let own = local.own.as_ref().map_or("an unstored build", short);
                return vec![
                    format!("{} cannot be checked: {error}", local.branch),
                    format!("this daemon runs {own}"),
                ];
            }
        };
        let mut lines = vec![
            format!("{} at {}", local.branch, short(&tip)),
            local_line(self.state.as_ref(), &local, &tip, Some(checked), now),
        ];
        lines.extend(self.stranded_line(&tip));
        match self.hub.fleet_versions() {
            Ok(fleet) => {
                lines.push(format!(
                    "the hub runs {}",
                    version_line(self.state.as_ref(), &fleet.hub_version, &tip).1
                ));
                for (_, line) in machine_lines(self.state.as_ref(), &fleet, &tip, now) {
                    lines.push(line);
                }
            }
            Err(error) => lines.push(format!("the hub cannot be reached: {error}")),
        }
        lines
    }
}

/// What a look at the machines found.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Look {
    /// Every machine runs the release; a line for each.
    Deployed(Vec<String>),
    /// The release was rolled back or failed to build here, for this reason.
    RolledBack(String),
    /// Lines for the machines that do not run it yet.
    Pending(Vec<String>),
    Unreachable(String),
}

/// `ignite wait deploy-<commit>`.
pub struct DeployWaiter {
    pub state: Arc<dyn DeployState>,
    pub hub: Arc<dyn Hub>,
    pub clock: Arc<dyn Clock>,
    /// Waits this many seconds between looks: a sleep in the daemon, a move of the fake clock
    /// in tests.
    pub pause: Arc<dyn Fn(f64) + Send + Sync>,
}

impl DeployWaiter {
    fn look(&self, target: &Commit) -> Look {
        let state = self.state.as_ref();
        if let Some(reason) = state.rejection(target) {
            return Look::RolledBack(reason);
        }
        let local = state.local();
        if let Some(Work::Failed(commit, reason)) = &local.work
            && commit == target
        {
            return Look::RolledBack(format!("{} failed to build: {reason}", short(commit)));
        }
        let now = self.clock.now();
        let fleet = match self.hub.fleet_versions() {
            Ok(fleet) => fleet,
            Err(error) => return Look::Unreachable(error.to_string()),
        };
        let mut done = Vec::new();
        let mut pending = Vec::new();
        let line = local_line(state, &local, target, None, now);
        match &local.own {
            Some(own) if !state.reached(own, target) => pending.push(line),
            _ => done.push(line),
        }
        let (reached, hub) = version_line(state, &fleet.hub_version, target);
        let hub = format!("the hub runs {hub}");
        if reached {
            done.push(hub);
        } else {
            pending.push(hub);
        }
        for (reached, line) in machine_lines(state, &fleet, target, now) {
            if reached {
                done.push(line);
            } else {
                pending.push(line);
            }
        }
        if pending.is_empty() {
            Look::Deployed(done)
        } else {
            Look::Pending(pending)
        }
    }

    /// Waits up to `wait_s` seconds for `target` to reach every machine, passing each new state
    /// to `progress`. `left` reports that the client has gone, which ends the wait early.
    pub fn wait(
        &self,
        target: &Commit,
        wait_s: u32,
        mut progress: Option<&mut dyn FnMut(&str) -> bool>,
        left: &dyn Fn() -> bool,
    ) -> Reply {
        self.state.check_soon();
        let deadline = self.clock.now().plus_seconds(f64::from(wait_s));
        let handle = handle(target);
        let mut said: Option<Look> = None;
        loop {
            let look = self.look(target);
            match look {
                Look::Deployed(lines) => return deployed(target, &lines),
                Look::RolledBack(reason) => return rolled_back(target, &reason),
                Look::Pending(_) | Look::Unreachable(_) => {}
            }
            if said.as_ref() != Some(&look)
                && let Some(progress) = progress.as_mut()
            {
                for line in pending_lines(&look) {
                    progress(&line);
                }
            }
            if self.clock.now() >= deadline || left() {
                return still_pending(target, &handle, &look);
            }
            said = Some(look);
            (self.pause)(LOOK_EVERY_S);
        }
    }
}

fn pending_lines(look: &Look) -> Vec<String> {
    match look {
        Look::Pending(lines) => lines.clone(),
        Look::Unreachable(error) => vec![format!("the hub cannot be reached: {error}")],
        Look::Deployed(_) | Look::RolledBack(_) => Vec::new(),
    }
}

fn rows(label: &str, lines: &[String]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:<8} {line}", if i == 0 { label } else { "" }))
        .collect::<Vec<_>>()
        .join("\n")
}

fn deployed(target: &Commit, lines: &[String]) -> Reply {
    Reply {
        exit: Exit::Success,
        text: format!(
            "deployed {} runs on every machine\n{}",
            short(target),
            rows("", lines)
        ),
        json: Some(json!({ "commit": target, "deployed": true, "machines": lines })),
        handle: None,
    }
}

fn rolled_back(target: &Commit, reason: &str) -> Reply {
    Reply {
        exit: Exit::CheckFailed,
        text: format!(
            "rolled back {} on this machine: {reason}\nnext     fix the cause with a \
             regression test and land again; ignite status shows what runs",
            short(target)
        ),
        json: Some(json!({ "commit": target, "deployed": false, "rolled_back": reason })),
        handle: None,
    }
}

fn still_pending(target: &Commit, handle: &str, look: &Look) -> Reply {
    let (exit, lines) = match look {
        Look::Unreachable(_) => (Exit::Unreachable, pending_lines(look)),
        _ => (Exit::Running, pending_lines(look)),
    };
    Reply {
        exit,
        text: format!(
            "waiting  {} has not reached every machine\n{}\nnext     ignite wait {handle}",
            short(target),
            rows("", &lines)
        ),
        json: Some(json!({ "commit": target, "deployed": false, "pending": lines })),
        handle: ignite_core::ids::Handle::new(handle).ok(),
    }
}

/// The daemon's deploy waiter, set once when it starts; `ignite wait` hands deploy handles to it.
static WAITER: OnceLock<Arc<DeployWaiter>> = OnceLock::new();

/// Sets the waiter `ignite wait deploy-<commit>` uses. Called once when the daemon starts.
pub fn install(waiter: Arc<DeployWaiter>) {
    let _ = WAITER.set(waiter);
}

/// Where `ignite wait` finds the deploy waiter.
pub type Slot = &'static OnceLock<Arc<DeployWaiter>>;

/// The daemon's slot, which [`install`] fills.
pub fn slot() -> Slot {
    &WAITER
}

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod tests;
