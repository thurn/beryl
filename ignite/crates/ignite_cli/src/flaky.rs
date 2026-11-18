//! `ignite flaky`: list the checks that are switched off or have flaked, and switch one on or
//! off by hand. It talks to the hub directly, like the ticket verbs, so it works while the daemon
//! is down. A switch needs the machine's write token; the hub records who made it and comments
//! on the check's flaky ticket if it has one.

use std::fmt::Write as _;

use clap::Subcommand;
use serde_json::{Value, json};

use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::{CheckState, CheckStateChanged, FlakyChecksReply};

use crate::client::session_from_env;
use crate::ticket::Hub;
use crate::ticket::http::{Failure, Result, usage};

#[derive(Debug, Subcommand)]
pub enum FlakyCommand {
    /// List the checks that are switched off, have flaked or were switched by hand.
    List,
    /// Switch a check back on, such as after replacing what made it flaky.
    On {
        /// The check, such as `ui:hand-chooser`.
        check: String,
        /// Why, recorded on the check's flaky ticket.
        #[arg(short = 'm', long = "note")]
        note: Option<String>,
    },
    /// Switch a check off on every machine.
    Off {
        check: String,
        #[arg(short = 'm', long = "note")]
        note: Option<String>,
    },
}

pub fn run(command: &FlakyCommand) -> Reply {
    let env = |name: &str| std::env::var(name).ok();
    let actor = session_from_env(env).map_or_else(|| "ignite".to_owned(), |s| s.to_string());
    let cwd = std::env::current_dir().unwrap_or_default();
    Hub::load(&cwd, actor, env)
        .and_then(|hub| execute(&hub, command))
        .unwrap_or_else(Failure::into_reply)
}

pub fn execute(hub: &Hub, command: &FlakyCommand) -> Result<Reply> {
    let (check, state, note) = match command {
        FlakyCommand::List => {
            let value = hub.get(&["checks", "flaky"], &[])?.json()?;
            let reply: FlakyChecksReply = serde_json::from_value(value.clone()).map_err(usage)?;
            return Ok(answer(value, list_text(&reply)));
        }
        FlakyCommand::On { check, note } => (check, CheckState::On, note),
        FlakyCommand::Off { check, note } => (check, CheckState::Off, note),
    };
    let body = json!({
        "check": check,
        "state": state,
        "note": note.as_deref().unwrap_or_default(),
    });
    // Setting a state twice is the same as setting it once, so a replay does no harm.
    let value = hub.post(&["checks", "state"], &body, true)?.json()?;
    let changed: CheckStateChanged = serde_json::from_value(value.clone()).map_err(usage)?;
    Ok(answer(value, switched_text(&changed)))
}

fn answer(json: Value, text: String) -> Reply {
    Reply {
        exit: Exit::Success,
        text,
        json: Some(json),
        handle: None,
    }
}

fn word(state: CheckState) -> &'static str {
    match state {
        CheckState::On => "on",
        CheckState::Off => "off",
        CheckState::Uncached => "uncached",
    }
}

/// A ticket ID as the commands print it, with its `T`.
fn spelled(ticket: &str) -> String {
    if ticket.starts_with('T') {
        ticket.to_owned()
    } else {
        format!("T{ticket}")
    }
}

fn switched_text(changed: &CheckStateChanged) -> String {
    if changed.previous == changed.state {
        return format!("{} was already {}", changed.check, word(changed.state));
    }
    let mut text = format!(
        "switched {} {} (it was {})",
        changed.check,
        word(changed.state),
        word(changed.previous)
    );
    if let Some(ticket) = &changed.ticket {
        let _ = write!(text, "\nrecorded on {}", spelled(ticket));
    }
    text
}

fn list_text(reply: &FlakyChecksReply) -> String {
    if reply.checks.is_empty() {
        return "no check is switched off or has flaked".to_owned();
    }
    let mut text = String::new();
    for check in &reply.checks {
        let _ = write!(
            text,
            "{:<8} {}  flaky {}",
            word(check.state),
            check.check,
            check.flaky_records
        );
        if let Some(at) = &check.last_flaky {
            let _ = write!(text, ", last {at}");
        }
        if let Some(ticket) = &check.ticket {
            let _ = write!(text, "  {}", spelled(ticket));
        }
        if let Some(switched) = &check.switched {
            let _ = write!(
                text,
                "  switched {} by {} at {}",
                word(switched.state),
                switched.actor,
                switched.at
            );
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
#[path = "flaky_tests.rs"]
mod tests;
