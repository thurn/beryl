//! `ignite dispatch` when the daemon does not answer.
//!
//! The dispatcher is a session whose whole job is to call `ignite dispatch --wait` and carry out
//! what it returns, so it keeps calling while the daemon is down. Two things follow:
//!
//! - Each call that cannot reach the daemon waits before it answers, so the dispatcher does not
//!   spin on failures and spend its context on them.
//! - When the monitor has written `justiciar-needed.json` in the Ignite home (the daemon stayed
//!   down in spite of restarts), the call answers with a hand-off of a justiciar prompt to a
//!   worker of the pool, read from the daemon's database. The daemon is the one that would
//!   normally hand off a justiciar ticket, and the hub cannot file one without a daemon to
//!   report to, so this is the only way the machine gets a repair session without a person.
//!   The worker's acknowledgement comes back as `ignite dispatch ack <id>` with the fallback's
//!   own ID, answered here too. A refused hand-off moves on to the next worker.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{DispatchAction, DispatchInstruction, Exit, Reply, Request};
use ignite_core::time::Timestamp;

/// The ID of every instruction this module gives, which no daemon instruction uses.
pub const FALLBACK_ID: u64 = u64::MAX;

/// The file the monitor writes when it gives up restarting the daemon.
const NEEDED: &str = "justiciar-needed.json";

/// What this module remembers between calls.
const STATE: &str = "justiciar-handoff.json";

/// How long a call that cannot reach the daemon waits before it answers.
const WAIT_S: u64 = 60;

/// How long a hand-off may wait for its acknowledgement before the next worker is tried.
const ACK_WAIT_S: f64 = 180.0;

/// How long after an acknowledged hand-off no other is given for the same request.
const HANDED_S: f64 = 3_600.0;

/// The monitor's request for a repair.
#[derive(Clone, Debug, Deserialize)]
pub struct Needed {
    pub at: Timestamp,
    pub reason: String,
    #[serde(default)]
    pub evidence: String,
}

/// The hand-offs given for one request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// The request these hand-offs answer, by its time.
    pub needed_at: Option<Timestamp>,
    /// Workers already given the prompt, in order.
    pub tried: Vec<String>,
    pub handed_at: Option<Timestamp>,
    pub acked: bool,
}

/// What a call answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Hand the justiciar prompt to this worker.
    Handoff(String),
    /// Nothing to do; answer idle after the wait.
    Idle,
}

/// Decides what to do for `needed`, given the workers of the pool in order of preference.
pub fn decide(needed: &Needed, state: &mut State, workers: &[String], now: Timestamp) -> Decision {
    if state.needed_at != Some(needed.at) {
        *state = State {
            needed_at: Some(needed.at),
            ..State::default()
        };
    }
    if let Some(at) = state.handed_at {
        let waited = now.seconds_since(at);
        if (state.acked && waited < HANDED_S) || (!state.acked && waited < ACK_WAIT_S) {
            return Decision::Idle;
        }
    }
    let Some(next) = workers.iter().find(|w| !state.tried.contains(w)) else {
        return Decision::Idle;
    };
    state.tried.push(next.clone());
    state.handed_at = Some(now);
    state.acked = false;
    Decision::Handoff(next.clone())
}

/// Records an acknowledgement of the fallback's hand-off.
pub fn acknowledge(state: &mut State, ok: bool) {
    state.acked = ok;
    if !ok {
        state.handed_at = None;
    }
}

/// The prompt a justiciar gets when the daemon is down.
pub fn prompt(needed: &Needed, home: &Path) -> String {
    format!(
        "You are this machine's justiciar. The Ignite daemon is down and the monitor could not \
         restart it: {}.\n\nEvidence: {}\n\nRead `ignite guide justiciar`, then repair the \
         daemon. Its log is ~/Library/Logs/ignite.daemon.log (or the login service's log), its \
         crash records are in {}, and `ignite monitor` checks it again. You hold no ticket: \
         once the hub answers, record what you did with `ignite ticket create` and close it.",
        needed.reason,
        needed.evidence,
        home.join("crash").display()
    )
}

fn instruction(action: DispatchAction, session: Option<String>, prompt: Option<String>) -> Reply {
    let instruction = DispatchInstruction {
        id: FALLBACK_ID,
        action,
        session: session.and_then(|s| SessionId::new(s).ok()),
        clear: action == DispatchAction::Handoff,
        model: (action == DispatchAction::Handoff).then(|| "opus".to_owned()),
        effort: (action == DispatchAction::Handoff).then(|| "high".to_owned()),
        prompt,
    };
    let line = serde_json::to_string(&instruction).unwrap_or_default();
    Reply::text(
        Exit::Success,
        format!(
            "{line}\nThe daemon is down; this instruction comes from the client. Carry it out, \
             report `ignite dispatch ack {FALLBACK_ID} ok` (or `failed <why>`), then run \
             `ignite dispatch --wait 540` again. That loop is your whole job."
        ),
    )
}

fn load<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The workers of the pool, idle ones first, from the daemon's database.
fn workers(home: &Path) -> Vec<String> {
    let Ok(conn) = ignite_store::local::open(&home.join("daemon.db")) else {
        return Vec::new();
    };
    let mut members: Vec<_> = ignite_store::local::pool::all(&conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.role == "worker" && m.state != "down")
        .collect();
    members.sort_by_key(|m| (m.state != "idle", m.added_at.clone()));
    members.into_iter().map(|m| m.session).collect()
}

/// Answers `ignite dispatch` (or its `ack`) for a daemon that did not answer, or `None` to let
/// the caller report the failure as usual.
pub fn dispatch(request: &Request, home: &Path, now: Timestamp) -> Option<Reply> {
    let state_path = home.join(STATE);
    let mut state: State = load(&state_path).unwrap_or_default();
    if request.args.first().map(String::as_str) == Some("ack") {
        if request.args.get(1).map(String::as_str) != Some(&FALLBACK_ID.to_string()) {
            return None;
        }
        acknowledge(
            &mut state,
            request.args.get(2).map(String::as_str) == Some("ok"),
        );
        let _ = std::fs::write(&state_path, serde_json::to_vec(&state).unwrap_or_default());
        return Some(Reply::text(Exit::Success, "acknowledged"));
    }
    let wait = u64::from(request.wait_s).min(WAIT_S);
    let Some(needed) = load::<Needed>(&home.join(NEEDED)) else {
        std::thread::sleep(Duration::from_secs(wait));
        return None;
    };
    let decision = decide(&needed, &mut state, &workers(home), now);
    let _ = std::fs::write(&state_path, serde_json::to_vec(&state).unwrap_or_default());
    Some(match decision {
        Decision::Handoff(session) => instruction(
            DispatchAction::Handoff,
            Some(session),
            Some(prompt(&needed, home)),
        ),
        Decision::Idle => {
            std::thread::sleep(Duration::from_secs(wait));
            instruction(DispatchAction::Idle, None, None)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: f64) -> Timestamp {
        Timestamp::parse("2026-11-18T00:00:00Z")
            .unwrap()
            .plus_seconds(seconds)
    }

    fn needed() -> Needed {
        Needed {
            at: at(0.0),
            reason: "the daemon does not answer".to_owned(),
            evidence: String::new(),
        }
    }

    fn workers() -> Vec<String> {
        vec!["local_a".to_owned(), "local_b".to_owned()]
    }

    #[test]
    fn a_request_hands_off_once_and_waits_for_the_acknowledgement() {
        let mut state = State::default();
        let first = decide(&needed(), &mut state, &workers(), at(10.0));
        assert_eq!(first, Decision::Handoff("local_a".to_owned()));
        assert_eq!(
            decide(&needed(), &mut state, &workers(), at(20.0)),
            Decision::Idle
        );
        acknowledge(&mut state, true);
        assert_eq!(
            decide(&needed(), &mut state, &workers(), at(1_000.0)),
            Decision::Idle
        );
    }

    #[test]
    fn a_refused_or_unanswered_handoff_moves_to_the_next_worker() {
        let mut state = State::default();
        decide(&needed(), &mut state, &workers(), at(10.0));
        acknowledge(&mut state, false);
        let next = decide(&needed(), &mut state, &workers(), at(11.0));
        assert_eq!(next, Decision::Handoff("local_b".to_owned()));
        // No answer at all: after the acknowledgement wait, every worker has been tried.
        assert_eq!(
            decide(&needed(), &mut state, &workers(), at(500.0)),
            Decision::Idle
        );
    }

    #[test]
    fn a_new_request_starts_over() {
        let mut state = State::default();
        decide(&needed(), &mut state, &workers(), at(10.0));
        acknowledge(&mut state, true);
        let later = Needed {
            at: at(7_200.0),
            ..needed()
        };
        let again = decide(&later, &mut state, &workers(), at(7_300.0));
        assert_eq!(again, Decision::Handoff("local_a".to_owned()));
    }

    #[test]
    fn the_prompt_names_the_guide_and_the_reason() {
        let text = prompt(&needed(), Path::new("/home/ignite"));
        assert!(text.contains("ignite guide justiciar"));
        assert!(text.contains("the daemon does not answer"));
        assert!(text.contains("/home/ignite/crash"));
    }
}
