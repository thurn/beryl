//! Sending a verb to the machine's daemon and returning its reply.
//!
//! Verbs that no ticket has built yet are listed in [`NOT_BUILT`] with the title of the ticket
//! that builds each; the client answers those itself, without the daemon. A ticket that
//! implements a verb deletes its row.

use std::path::PathBuf;

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{Exit, Reply, Request};

/// The verbs nobody has built yet, with the owning ticket's title.
pub const NOT_BUILT: &[(&str, &str)] = &[
    (
        "audit",
        "Idle work: exploration, cache audits and rebuild counts",
    ),
    (
        "daemon",
        "Assemble the daemon and prove one ticket end to end",
    ),
    ("gate", "Gate replay"),
    ("land", "Landing"),
    ("prime", "Command output and the workflow text"),
    (
        "rebuild",
        "Idle work: exploration, cache audits and rebuild counts",
    ),
    ("status", "Install, self-update, status and doctor"),
    ("sync", "Landing"),
];

/// How long the daemon may hold a reply before answering with a handle, below the agents'
/// tool timeout.
pub const DEFAULT_WAIT_S: u32 = 540;

/// Environment variables that name the asking session, first match wins: Ignite's own, then
/// the one the Claude desktop app gives its sessions.
pub const SESSION_VARIABLES: &[&str] = &["IGNITE_SESSION", "CLAUDE_CODE_HOST_SESSION_ID"];

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
    }
}

/// Sends the request to the machine's daemon over its socket and reads its reply. A daemon
/// that cannot be reached is a configuration error.
pub fn send(request: &Request) -> Reply {
    let socket = ignite_daemon::server::default_socket();
    ignite_daemon::server::send(&socket.to_string_lossy(), request).unwrap_or_else(|error| {
        Reply::text(
            Exit::Usage,
            format!(
                "failed   the daemon at {} did not answer: {error}\nnext     ignite doctor",
                socket.display()
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn unbuilt_verbs_name_their_ticket() {
        let reply = not_built("land").unwrap();
        assert_eq!(reply.exit, Exit::Usage);
        assert_eq!(reply.text, "not implemented: Landing");
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
