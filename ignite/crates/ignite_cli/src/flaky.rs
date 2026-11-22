//! `ignite flaky` lists flaky checks, reads a ticket's saved evidence, and switches checks on
//! or off. It talks to the hub directly, so it works while the daemon is down. A switch needs
//! the machine's write token. The hub records who made it and comments on the check's ticket.

use std::fmt::Write as _;

use clap::Subcommand;
use serde_json::{Value, json};

use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::{CheckState, CheckStateChanged, FlakyChecksReply, FlakyEvidence};

use crate::client::session_from_env;
use crate::ticket::Hub;
use crate::ticket::http::{Failure, Result, usage};
use crate::ticket::ids::spelled;

#[derive(Debug, Subcommand)]
pub enum FlakyCommand {
    /// List the checks that are switched off, have flaked or were switched by hand.
    List,
    /// Show the saved reports and outputs for a flaky-check ticket.
    Evidence {
        /// The ticket that holds the reports.
        ticket: String,
    },
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
        FlakyCommand::Evidence { ticket } => {
            let value = hub
                .get(
                    &["checks", "flaky", "evidence"],
                    &[("ticket", ticket.clone())],
                )?
                .json()?;
            let reports: Vec<FlakyEvidence> =
                serde_json::from_value(value.clone()).map_err(usage)?;
            return Ok(answer(value, evidence_text(&reports)));
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

fn evidence_text(reports: &[FlakyEvidence]) -> String {
    if reports.is_empty() {
        return "this ticket has no saved flaky reports".to_owned();
    }
    let mut text = String::new();
    for report in reports {
        let _ = writeln!(
            text,
            "{} at {} on {} (commit {}, input {})",
            report.check, report.at, report.machine, report.commit, report.hash
        );
        if !report.command.is_empty() {
            let _ = writeln!(text, "command {}", report.command);
        }
        let output = if report.output.is_empty() {
            "(no output was reported)"
        } else {
            &report.output
        };
        let _ = writeln!(text, "{output}\n");
    }
    text
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
