//! `ignite dispatch allowed [<tool> [<session>]]`: the check behind the hook that `ignite pool
//! init` writes into the dispatcher's folder, which runs before each call of a session tool that
//! starts, clears, reconfigures or messages a session.
//!
//! Given a tool, it asks the daemon and prints its answer. Given none, it is the hook itself: it
//! reads the tool call Claude Code passes on stdin as JSON (`tool_name`, and `tool_input` with
//! the target's `session_id`), and when the call is not allowed it prints why on stderr and
//! exits 2, which makes Claude Code block the call and show the model that text. Every failure
//! blocks, including a daemon that cannot answer: the hook exists to stop calls nobody asked for,
//! so it never lets one through because it could not ask. The one exception is the client's own
//! hand-off of a justiciar prompt while the daemon is down (see `fallback`).

use std::io::Read as _;

use ignite_core::protocol::client::{Exit, Reply};

use crate::{client, fallback};

/// The tool name and target session of a hook's JSON, or `None` when it names no tool.
fn from_hook(json: &str) -> Option<(String, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let tool = value.get("tool_name")?.as_str()?.to_owned();
    let session = value
        .get("tool_input")
        .and_then(|input| input.get("session_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Some((tool, session))
}

/// Asks the daemon, falling back on the client's own hand-off while it is down.
fn ask(tool: &str, session: Option<&str>) -> Reply {
    let mut args = vec!["allowed".to_owned(), tool.to_owned()];
    args.extend(session.map(str::to_owned));
    let raw: Vec<String> = ["ignite", "dispatch"]
        .into_iter()
        .map(str::to_owned)
        .chain(args)
        .collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let asking = client::session_from_env(|name| std::env::var(name).ok());
    let request = client::request("dispatch", &raw, cwd, asking);
    client::send_or(&request, |_| {
        let home = ignite_daemon::update::Home::from_env().root;
        let now = ignite_core::traits::Clock::now(&ignite_core::time::SystemClock);
        let starts = tool.ends_with("start_session");
        Some(if fallback::allows(&home, starts, session, now) {
            Reply::text(
                Exit::Success,
                "allowed  by the client's hand-off; the daemon is down",
            )
        } else {
            Reply::text(
                Exit::CheckFailed,
                format!(
                    "blocked  {tool}: the daemon did not answer, and no hand-off from the client \
                     calls for it. Run `ignite dispatch --wait 540` and carry out only what it \
                     prints."
                ),
            )
        })
    })
}

pub fn run(tool: Option<&str>, session: Option<&str>) -> Reply {
    if let Some(tool) = tool {
        return ask(tool, session);
    }
    let mut input = String::new();
    let reply = match std::io::stdin().read_to_string(&mut input) {
        Ok(_) => match from_hook(&input) {
            Some((tool, session)) => ask(&tool, session.as_deref()),
            None => Reply::text(Exit::CheckFailed, "blocked  the hook's input named no tool"),
        },
        Err(error) => Reply::text(
            Exit::CheckFailed,
            format!("blocked  could not read the hook's input: {error}"),
        ),
    };
    if reply.exit == Exit::Success {
        return Reply::text(Exit::Success, "");
    }
    eprintln!("{}", reply.text.trim_end());
    Reply::text(Exit::CheckFailed, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_tool_and_its_target_from_a_hooks_input() {
        let json = r#"{"session_id": "local_dispatcher", "hook_event_name": "PreToolUse",
            "tool_name": "mcp__ccd_session_mgmt__clear_session",
            "tool_input": {"session_id": "local_w1"}}"#;
        assert_eq!(
            from_hook(json),
            Some((
                "mcp__ccd_session_mgmt__clear_session".to_owned(),
                Some("local_w1".to_owned())
            ))
        );
        let start = r#"{"tool_name": "mcp__ccd_session__start_session", "tool_input": {}}"#;
        assert_eq!(
            from_hook(start),
            Some(("mcp__ccd_session__start_session".to_owned(), None))
        );
        assert_eq!(from_hook("not json"), None);
    }
}
