//! Sending a verb to the machine's daemon and returning its reply.
//!
//! Verbs that no ticket has built yet are listed in [`NOT_BUILT`] with the title of the ticket
//! that builds each; the client answers those itself, without the daemon. A ticket that
//! implements a verb deletes its row.

use std::path::PathBuf;
use std::time::Instant;

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_daemon::server::Cut;

use crate::reattach::SocketDaemon;

/// The verbs nobody has built yet, with the owning ticket's title.
pub const NOT_BUILT: &[(&str, &str)] = &[];

/// How long the daemon may hold a reply before answering with a handle, below the agents'
/// tool timeout.
pub const DEFAULT_WAIT_S: u32 = 540;

/// Seconds a `dispatch` request waits beyond its `--wait`. The daemon answers idle a few
/// seconds before the request's own wait ends (`CALLER_MARGIN_S` in the daemon's dispatch
/// verb), so the request has to outlast the wait it asks for.
pub const DISPATCH_MARGIN_S: u32 = 10;

/// The request's own wait for `ignite dispatch --wait <wait>`. Without it, every request waits
/// [`DEFAULT_WAIT_S`], and the daemon would cut a longer dispatch wait short.
pub fn dispatch_request_wait_s(wait: u32) -> u32 {
    wait.saturating_add(DISPATCH_MARGIN_S)
}

/// Environment variables that name the asking session, first match wins: Ignite's own, then
/// the one the Claude desktop app gives its sessions.
pub const SESSION_VARIABLES: &[&str] = &["IGNITE_SESSION", "CLAUDE_CODE_HOST_SESSION_ID"];

/// Verbs sent without the asking session, which names its session in its arguments instead.
/// The daemon records each session's current command, and `turn-ended`, which a session's
/// `Stop` hook runs, would replace a running background `land` there and look like a command
/// that ended.
pub const UNRECORDED_VERBS: &[&str] = &["turn-ended"];

/// The reply for a verb nobody has built, if `verb` is one.
pub fn not_built(verb: &str) -> Option<Reply> {
    NOT_BUILT
        .iter()
        .find(|(name, _)| *name == verb)
        .map(|(_, owner)| Reply::text(Exit::Usage, format!("not implemented: {owner}")))
}

/// The session named by the environment.
pub fn session_from_env(lookup: impl Fn(&str) -> Option<String>) -> Option<SessionId> {
    SESSION_VARIABLES
        .iter()
        .filter_map(|name| lookup(name))
        .find_map(|value| SessionId::new(value).ok())
}

/// The arguments typed after the verb, without the global `--json` flag.
pub fn args_after_verb(raw: &[String], verb: &str) -> Vec<String> {
    let start = raw
        .iter()
        .position(|arg| arg == verb)
        .map_or(raw.len(), |i| i + 1);
    raw[start..]
        .iter()
        .filter(|arg| *arg != "--json")
        .cloned()
        .collect()
}

/// The request the client sends for a verb.
pub fn request(verb: &str, raw: &[String], cwd: PathBuf, session: Option<SessionId>) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args_after_verb(raw, verb),
        cwd,
        session,
        wait_s: DEFAULT_WAIT_S,
        pid: Some(std::process::id()),
    }
}

/// Sends the request to the machine's daemon over its socket and reads its reply, keeping it
/// across a restart of the daemon (see [`crate::reattach`]). A daemon that cannot be reached is
/// a configuration error.
pub fn send(request: &Request) -> Reply {
    deliver(request, None, true).unwrap_or_else(|(socket, cut)| unreachable_daemon(&socket, &cut))
}

/// The verbs whose client prints each step as it starts, before the reply.
pub const SHOWS_STEPS: &[&str] = &["land"];

/// [`send`], printing each step of the verb on its own `step` line as the daemon reports it.
/// Once a line cannot be written (the reader of a pipe has gone), no more are printed, and the
/// reply is still awaited.
pub fn send_showing_steps(request: &Request) -> Reply {
    use std::io::Write as _;
    let mut open = true;
    let mut show = |text: &str| {
        if open {
            let mut out = std::io::stdout().lock();
            open = writeln!(out, "step     {text}")
                .and_then(|()| out.flush())
                .is_ok();
        }
    };
    deliver(request, Some(&mut show), true)
        .unwrap_or_else(|(socket, cut)| unreachable_daemon(&socket, &cut))
}

/// Sends `request` to the machine's daemon through [`crate::reattach::send`]. `Err` names the
/// socket and the failure to reach a daemon.
fn deliver(
    request: &Request,
    progress: Option<&mut dyn FnMut(&str)>,
    retry_unsent: bool,
) -> Result<Reply, (PathBuf, Cut)> {
    let socket = ignite_daemon::server::default_socket();
    let mut daemon = SocketDaemon {
        socket: socket.to_string_lossy().into_owned(),
        started: Instant::now(),
    };
    crate::reattach::send(&mut daemon, request, progress, retry_unsent).map_err(|cut| (socket, cut))
}

fn unreachable_daemon(socket: &std::path::Path, cut: &Cut) -> Reply {
    Reply::text(
        Exit::Usage,
        format!(
            "failed   the daemon at {} did not answer: {}\nnext     ignite doctor",
            socket.display(),
            cut.error
        ),
    )
}

/// [`send`], but a daemon that cannot be reached is first offered to `fallback`, whose reply,
/// if it gives one, is the answer. A daemon that refuses the connection goes to the fallback at
/// once, which waits by itself; one whose connection drops during the request is tried again.
pub fn send_or(request: &Request, fallback: impl FnOnce(&Request) -> Option<Reply>) -> Reply {
    match deliver(request, None, false) {
        Ok(reply) => reply,
        Err((socket, cut)) => {
            fallback(request).unwrap_or_else(|| unreachable_daemon(&socket, &cut))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn every_verb_is_built() {
        assert!(not_built("gate").is_none());
        assert!(not_built("nonsense").is_none());
    }

    #[test]
    fn the_table_is_sorted_and_unique() {
        let names: Vec<_> = NOT_BUILT.iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
    }

    #[test]
    fn forwards_the_arguments_after_the_verb() {
        let raw = strings(&["ignite", "--json", "land", "-m", "a note", "--json"]);
        assert_eq!(args_after_verb(&raw, "land"), ["-m", "a note"]);
        let request = request("land", &raw, PathBuf::from("/t"), None);
        assert_eq!(request.wait_s, DEFAULT_WAIT_S);
    }

    #[test]
    fn a_dispatch_request_outlasts_its_wait() {
        assert_eq!(dispatch_request_wait_s(3600), 3610);
        assert_eq!(dispatch_request_wait_s(u32::MAX), u32::MAX);
    }

    #[test]
    fn the_session_comes_from_the_first_variable_set() {
        let env = |name: &str| match name {
            "CLAUDE_CODE_HOST_SESSION_ID" => Some("local_abc".to_owned()),
            _ => None,
        };
        assert_eq!(session_from_env(env).unwrap().as_str(), "local_abc");
        let both = |name: &str| Some(format!("{name}-value"));
        assert_eq!(
            session_from_env(both).unwrap().as_str(),
            "IGNITE_SESSION-value"
        );
        assert!(session_from_env(|_| None).is_none());
    }
}
