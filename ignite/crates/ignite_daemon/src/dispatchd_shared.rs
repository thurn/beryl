//! Shared failures: when the tip is broken, or a check is broken for everyone, the daemon
//! stops starting ordinary tickets on this machine.
//!
//! Before this, each worker found such a failure on its own, spent its ticket's time on a
//! failure that was not its own, and the pool kept starting more tickets into the same failure.
//!
//! A shared failure starts when either holds:
//!
//! - two different sessions on this machine had the same check fail the same way within
//!   [`SAME_WINDOW_S`] in `ignite test`, `check` or `land`. Two failures are the same when their
//!   messages have the same first line, or when they ran on the same inputs (a session that
//!   finds the failure in the result cache has no message of its own);
//! - a session gave its ticket back after its landing was refused because the check's result at
//!   the tip could not be found or run (the last case of `ignite guide blocked`).
//!
//! The outcomes come from each session's latest outcome of each check, which the result cache
//! records on this machine ([`Host::check_sightings`]). When `green` exists, a failure counts
//! only if the tested tree included that commit. A run on an older tree can finish after
//! `green` advances.
//!
//! While it lasts, the hub is asked only for tickets that hold `master` back, as it is while
//! commands wait long for cores (see `work`); `ignite status` and `ignite pool list` say why;
//! each running worker of the pool gets one message, sent the way a stall reminder is; and the
//! failure is reported once to the hub as a justiciar report, unless the check is already
//! assigned to an open regression ticket.
//!
//! It ends when the check passes for any session on this machine, when the `green` branch moves,
//! or after [`LAPSE_S`], whichever is first. Only failures seen after the end can start the next
//! one. The daemon persists the hold with the dispatch queue (see `store`), so it survives a restart.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use ignite_core::ids::Commit;
use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::time::Timestamp;
use ignite_store::local::rows::CheckSighting;

use super::{HeadlessDispatch, State, cli};
use crate::server::Context;

/// How close together two sessions' failures must be to count as one shared failure: 15
/// minutes.
pub const SAME_WINDOW_S: f64 = 15.0 * 60.0;
/// How long a shared failure lasts when nothing ends it sooner: 2 hours.
pub const LAPSE_S: f64 = 2.0 * 60.0 * 60.0;

/// The shared failure in force, if any, and when the last one ended.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Watch {
    pub(super) current: Option<SharedFailure>,
    /// Failures seen before this do not start another shared failure.
    pub(super) ended_at: Option<Timestamp>,
}

/// One check failing the same way for several sessions; see the module documentation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct SharedFailure {
    pub(super) check: String,
    /// The first line of the failure's output, when a session ran the check itself.
    pub(super) first_line: Option<String>,
    /// The sessions that failed it.
    pub(super) sessions: Vec<String>,
    /// It started because a session released its ticket after the check's result at the tip
    /// could not be found or run.
    pub(super) tip_unknown: bool,
    pub(super) since: Timestamp,
    /// Where `green` pointed when it started; `None` when there is no `green` branch.
    pub(super) green: Option<String>,
    /// The workers already sent the message.
    pub(super) messaged: BTreeSet<String>,
    /// Reported to the hub, or covered by an open regression ticket.
    pub(super) reported: bool,
}

impl SharedFailure {
    /// Why only tickets that hold `master` back start, for `ignite status`, `ignite pool list`
    /// and the reason the pool's idle workers got nothing.
    pub(super) fn reason(&self) -> String {
        if self.tip_unknown {
            format!(
                "a worker released its ticket because {}'s result at the tip could not be found \
                 or run, so only tickets that hold master back start",
                self.check
            )
        } else {
            format!(
                "{} workers failed {} the same way, so only tickets that hold master back start",
                self.sessions.len(),
                self.check
            )
        }
    }

    /// The message each running worker gets once.
    fn message(&self) -> String {
        let how = match &self.first_line {
            Some(line) => format!(" with `{line}`"),
            None => String::new(),
        };
        let what = if self.tip_unknown {
            format!(
                "`{}`'s result at the tip could not be found or run{how}",
                self.check
            )
        } else {
            format!(
                "`{}` fails{how} for {} workers on this machine",
                self.check,
                self.sessions.len()
            )
        };
        format!(
            "{what}. It is being handled: do not investigate it or work around it. Go on with \
             work that does not need that check, or run `ignite wait` on the ticket that holds \
             master back for it."
        )
    }
}

/// The first shared failure among `sightings`, each session's latest outcome of each check,
/// oldest first. `holders` are the sessions that hold a ticket tree now.
fn detect(
    ctx: &Context,
    sightings: &[CheckSighting],
    holders: &BTreeSet<String>,
    green: Option<&str>,
) -> Option<(String, Option<String>, Vec<String>, bool)> {
    let mut by_check: BTreeMap<&str, Vec<&CheckSighting>> = BTreeMap::new();
    let mut includes_green = BTreeMap::new();
    for sighting in sightings {
        let head = sighting.head.as_deref();
        if sighting.outcome == "fail"
            && !*includes_green
                .entry(head)
                .or_insert_with(|| tested_green(ctx, head, green))
        {
            continue;
        }
        by_check
            .entry(sighting.check_name.as_str())
            .or_default()
            .push(sighting);
    }
    for (check, seen) in by_check {
        // A pass after a failure says the failure is not everyone's.
        let last_pass = seen
            .iter()
            .filter(|s| s.outcome == "pass")
            .map(|s| s.at.as_str())
            .max();
        let fails: Vec<&CheckSighting> = seen
            .into_iter()
            .filter(|s| s.outcome == "fail" && last_pass.is_none_or(|pass| s.at.as_str() > pass))
            .collect();
        for fail in &fails {
            let same: Vec<&&CheckSighting> = fails
                .iter()
                .filter(|other| {
                    let line = fail.first_line.is_some() && other.first_line == fail.first_line;
                    let inputs = fail.hash.is_some() && other.hash == fail.hash;
                    line || inputs
                })
                .collect();
            let sessions: BTreeSet<&str> = same.iter().map(|s| s.session.as_str()).collect();
            if sessions.len() >= 2 {
                let first_line = same.iter().find_map(|s| s.first_line.clone());
                let sessions = sessions.into_iter().map(str::to_owned).collect();
                return Some((check.to_owned(), first_line, sessions, false));
            }
        }
        // A session whose landing could not learn the tip's result, and which then gave its
        // ticket back instead of landing (a landing that passed would have recorded a pass).
        if let Some(fail) = fails
            .iter()
            .find(|s| s.tip_unknown != 0 && !holders.contains(&s.session))
        {
            return Some((
                check.to_owned(),
                fail.first_line.clone(),
                vec![fail.session.clone()],
                true,
            ));
        }
    }
    None
}

fn tested_green(ctx: &Context, head: Option<&str>, green: Option<&str>) -> bool {
    let Some(green) = green else {
        return true;
    };
    let Some(head) = head else {
        return false;
    };
    if head == green {
        return true;
    }
    match (Commit::new(green), Commit::new(head)) {
        (Ok(green), Ok(head)) => ctx.git.is_ancestor(&green, &head).unwrap_or(false),
        _ => false,
    }
}

impl HeadlessDispatch {
    fn detect_shared(&self, ctx: &Context, state: &State, now: Timestamp) -> Option<SharedFailure> {
        let window = now.plus_seconds(-SAME_WINDOW_S);
        let since = match state.shared.ended_at {
            Some(ended) if ended.plus_seconds(0.001) > window => ended.plus_seconds(0.001),
            _ => window,
        };
        let sightings = self.host.check_sightings(since);
        if !sightings.iter().any(|s| s.outcome == "fail") {
            return None;
        }
        let holders: BTreeSet<String> = state.held.keys().map(ToString::to_string).collect();
        // Read the remote branch only when the locally known revision has a candidate.
        let known = self.host.known_green();
        let candidate = detect(ctx, &sightings, &holders, known.as_deref())?;
        let green = self.host.green().or_else(|| known.clone());
        let (check, first_line, sessions, tip_unknown) = if green == known {
            candidate
        } else {
            detect(ctx, &sightings, &holders, green.as_deref())?
        };
        eprintln!(
            "ignite: a shared failure of {check} started; only tickets that hold master back start"
        );
        Some(SharedFailure {
            check,
            first_line,
            sessions,
            tip_unknown,
            since: now,
            green,
            messaged: BTreeSet::new(),
            reported: false,
        })
    }

    /// Ends the shared failure in force when its check passed, `green` moved or it lapsed;
    /// starts one when the sightings show it; and, while one lasts, sends its message to each
    /// running worker that has not had it.
    pub(super) fn watch_shared(&self, ctx: &Context, state: &mut State, now: Timestamp) {
        if let Some(current) = &state.shared.current {
            let lapsed = now.seconds_since(current.since) >= LAPSE_S;
            let green = self.host.green().or_else(|| self.host.known_green());
            let moved = green
                .as_deref()
                .is_some_and(|green| current.green.as_deref() != Some(green));
            let passed = !lapsed
                && !moved
                && self
                    .host
                    .check_sightings(current.since)
                    .iter()
                    .any(|s| s.check_name == current.check && s.outcome == "pass");
            if lapsed || moved || passed {
                state.shared = Watch {
                    current: None,
                    ended_at: Some(now),
                };
            }
        }
        if state.shared.current.is_none() {
            state.shared.current = self.detect_shared(ctx, state, now);
        }
        let Some(current) = &state.shared.current else {
            return;
        };
        let workers: Vec<_> = state
            .held
            .keys()
            .filter(|session| cli::is_cli_session(session))
            .filter(|session| !current.messaged.contains(session.as_str()))
            .cloned()
            .collect();
        if workers.is_empty() {
            return;
        }
        let message = current.message();
        for session in workers {
            state.cli_calls.push(cli::Call::Message {
                session: session.clone(),
                text: message.clone(),
                urgent: false,
            });
            if let Some(current) = &mut state.shared.current {
                current.messaged.insert(session.to_string());
            }
        }
    }

    /// Why only tickets that hold `master` back start while a shared failure lasts.
    pub fn shared_failure(&self) -> Option<String> {
        self.state()
            .shared
            .current
            .as_ref()
            .map(SharedFailure::reason)
    }

    /// Reports the shared failure in force to the hub as a justiciar report, once, unless its
    /// check is already assigned to an open regression ticket. Runs in the sender loop after a
    /// heartbeat that reached the hub (see `justiciar::Reporter`). The hub is asked with the
    /// state unlocked.
    pub fn report_shared(&self, ctx: &Context, host: &str) {
        let Some(current) = self.state().shared.current.clone().filter(|c| !c.reported) else {
            return;
        };
        let open = match ctx.hub.open_regressions() {
            Ok(open) => open,
            Err(error) => {
                eprintln!(
                    "ignite: could not look up open regressions for {}: {error}",
                    current.check
                );
                return;
            }
        };
        if !open
            .iter()
            .any(|entry| entry.check.as_str() == current.check)
        {
            let report = JusticiarReport {
                subject: format!("shared:{}", current.check),
                title: format!(
                    "{} fails the same way for every worker on {host}",
                    current.check
                ),
                details: format!(
                    "{}.\n\nSessions: {}. First line: {}.\n\nSince {} the daemon on {host} \
                     starts only tickets that hold master back. That ends when the check passes \
                     for a session on {host}, when green moves, or two hours after it started.",
                    current.reason(),
                    current.sessions.join(", "),
                    current.first_line.as_deref().unwrap_or("(none recorded)"),
                    current.since,
                ),
                evidence: current.since.to_string(),
                machine: ctx.machine.clone(),
                host: host.to_owned(),
                on_machine: true,
            };
            if let Err(error) = ctx.hub.justiciar(&report) {
                eprintln!("ignite: shared failure not reported: {error}");
                return;
            }
        }
        if let Some(still) = self.state().shared.current.as_mut()
            && still.check == current.check
            && still.since == current.since
        {
            still.reported = true;
        }
        self.save();
    }
}
