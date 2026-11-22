//! The guide topics `ignite guide <topic>` prints: the rules for one situation each, read only
//! when a command names the topic.
//!
//! Each topic's text is a plain-text file beside this module, under [`MAX_LINES`] lines of at
//! most [`MAX_COLUMNS`] columns, so that a worker reads it whole at the moment it applies. The
//! texts name nothing of any one project: where a rule is the project's, they say so.

use std::fmt::Write;

use crate::protocol::client::{Exit, Reply};

/// The most lines a topic may print.
pub const MAX_LINES: usize = 49;
/// The widest line a topic may print, in characters.
pub const MAX_COLUMNS: usize = 80;

/// One guide topic.
#[derive(Clone, Copy, Debug)]
pub struct Topic {
    /// The name `ignite guide` takes, such as `review`.
    pub name: &'static str,
    /// The line `ignite guide` lists it with.
    pub summary: &'static str,
    pub text: &'static str,
}

/// Every topic, in the order `ignite guide` lists them.
pub const TOPICS: &[Topic] = &[
    Topic {
        name: "follow-ups",
        summary: "filing a ticket while you hold one",
        text: include_str!("follow-ups.txt"),
    },
    Topic {
        name: "release",
        summary: "giving a ticket back, or setting it aside",
        text: include_str!("release.txt"),
    },
    Topic {
        name: "turn-end",
        summary: "ending a turn while you hold a ticket",
        text: include_str!("turn-end.txt"),
    },
    Topic {
        name: "cli-worker",
        summary: "working as a headless CLI worker, one process per turn",
        text: include_str!("cli-worker.txt"),
    },
    Topic {
        name: "blocked",
        summary: "a landing that was refused, or landed past a failing check",
        text: include_str!("blocked.txt"),
    },
    Topic {
        name: "review",
        summary: "reviewing landed work, and review:inline before landing",
        text: include_str!("review.txt"),
    },
    Topic {
        name: "regression",
        summary: "fixing a check that passed and now fails at the tip",
        text: include_str!("regression.txt"),
    },
    Topic {
        name: "reverted",
        summary: "a ticket whose landing was reverted, or that depends on one",
        text: include_str!("reverted.txt"),
    },
    Topic {
        name: "flaky-checks",
        summary: "a flaky, hung or never-catching check",
        text: include_str!("checks.txt"),
    },
    Topic {
        name: "audit",
        summary: "auditing the architecture or the process, and what to file",
        text: include_str!("audit.txt"),
    },
    Topic {
        name: "docs",
        summary: "keeping the documentation true",
        text: include_str!("docs.txt"),
    },
    Topic {
        name: "machine",
        summary: "a fault that breaks the machine for everyone on it",
        text: include_str!("machine.txt"),
    },
    Topic {
        name: "justiciar",
        summary: "automatic escalation for a fault or impossible ticket",
        text: include_str!("justiciar.txt"),
    },
    Topic {
        name: "incident",
        summary: "commanding the fleet during an operator-declared emergency",
        text: include_str!("incident.txt"),
    },
];

/// Names that older instructions still use for a topic, as (old name, current name).
const ALIASES: &[(&str, &str)] = &[("checks", "flaky-checks")];

/// The topic called `name`, or whose alias in [`ALIASES`] is `name`.
pub fn topic(name: &str) -> Option<&'static Topic> {
    let name = ALIASES
        .iter()
        .find(|(old, _)| *old == name)
        .map_or(name, |(_, current)| current);
    TOPICS.iter().find(|topic| topic.name == name)
}

/// The list `ignite guide` prints with no topic: one line per topic.
pub fn list() -> String {
    let width = TOPICS.iter().map(|t| t.name.len()).max().unwrap_or(0);
    let mut text = String::new();
    for topic in TOPICS {
        // Writing to a `String` cannot fail.
        let _ = writeln!(text, "{:<width$}  {}", topic.name, topic.summary);
    }
    text.push_str("\nignite guide <topic> prints one.\n");
    text
}

/// The reply to `ignite guide [topic]`.
pub fn reply(name: Option<&str>) -> Reply {
    match name {
        None => Reply::text(Exit::Success, list()),
        Some(name) => match topic(name) {
            Some(topic) => Reply::text(Exit::Success, topic.text),
            None => Reply::text(
                Exit::Usage,
                format!("no guide topic {name:?}. The topics:\n\n{}", list()),
            ),
        },
    }
}

#[cfg(test)]
mod tests;
