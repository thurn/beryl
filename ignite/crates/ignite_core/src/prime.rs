//! The workflow text `ignite prime` prints: the whole workflow in under 60 lines, ending with
//! the pointer to `ignite guide` for everything else.

use std::fmt::Write;

use crate::config::Config;

/// The most lines `ignite prime` prints.
pub const MAX_PRIME_LINES: usize = 60;

/// Each verb an agent uses while working and its one-line help.
pub const VERB_HELP: &[(&str, &str)] = &[
    (
        "start <ticket>",
        "claim a ticket, lease a tree, print the brief",
    ),
    ("test <names>", "run the named checks in your tree"),
    ("check", "run what the landing gate would run"),
    ("sync", "rebase your branch onto the tip"),
    (
        "land -m <note>",
        "rebase, gate, land, close, release the tree",
    ),
    (
        "release -m <note>",
        "give the ticket back unfinished, work kept",
    ),
    ("close -m <reason>", "close with nothing to land"),
    (
        "run -- <cmd>",
        "run any other heavy command with daemon cores",
    ),
    ("wait <handle>", "block until a running operation finishes"),
    ("stop <handle>", "end a running command of yours"),
    (
        "ticket <verb>",
        "create, show, list, link, comment on tickets",
    ),
    ("tell <to> <text>", "correct work in flight"),
    (
        "justiciar <reason>",
        "an emergency that stops everyone's work",
    ),
    ("why <check>", "a check's cost, failures and where it runs"),
    ("status, doctor", "this machine's state and health"),
    ("guide <topic>", "the rules for one situation"),
];

/// The workflow every worker reads.
pub fn prime(config: &Config) -> String {
    let staging = &config.repository.staging;
    let mut text = format!(
        "\
# Ignite workflow
Claim with `ignite start <ticket>`, edit in the tree it prints, commit there (`land` refuses
uncommitted files), run `ignite check`, then land to `{staging}` with `ignite land -m <note>`.
`ignite check` is all a plain change needs before landing; `ignite guide flaky-checks` is for a
flaky, hung or never-catching check. Every command prints a verdict first and a `next` line
last.

## Rules
- Nobody answers questions. Decide, act, and record the choice and why on the ticket.
- The ticket may be days old. Check it against the code as it is. Where they disagree, the
  code and the epic's intent win; record the difference on the ticket.
- Read the files the ticket names first, and send reads together.
- Make the smallest change that meets the acceptance criteria. Test it, exercise it by hand
  and read the whole diff as a reviewer before landing. A check that could not run is not
  a pass.
- Run every build, test suite and other heavy command through Ignite: `ignite test <names>`
  for checks and `ignite run -- <cmd>` for anything else. A command run directly competes
  for the cores that Ignite has already granted to other sessions.
- Tests go through public behavior and never depend on the machine's speed.
- Ordinary difficulty is the job: failing tests, confusing code, conflicts. Never wait for
  a person, for another ticket, for another machine or for verification. When the ticket
  needs something outside itself, finish what you can and land, or `ignite release` with
  a note.
- A refused landing's verdict says whose failure it is. Do what its `next` line says
  (`ignite guide blocked`); never decide yourself that a failure is unrelated.
- Start `land`, `check`, `test` and `wait` in the background where your tool can and end
  the turn: the exit wakes you. Never poll with `sleep` or a loop. Never end a turn on work
  that nothing will report, or stop for good holding a ticket: release it with its state.
- Never work in another session's tree, take over its ticket or change another ticket's
  scope.
- A `changed` line is an instruction about your ticket. Read it before you continue.
- The dispatcher, the session that started you, only relays messages from the daemon. It
  knows nothing about tickets and answers no messages, so never message it. Record a
  question or blocker on your ticket, tell another ticket's holder with `ignite tell`, or
  `ignite release` with a note.
- An emergency order is a dispatcher message starting `Message about <ticket>:`. Check it
  with `ignite ticket show <ticket>`: labelled `justiciar`, held by the session it names.
- The landing note says what changed, what was decided and what a reviewer should look at.
- The title shows the state: working, done, stopped unfinished, or waiting on a person.

## Verbs
"
    );
    for (verb, help) in VERB_HELP {
        let _ = writeln!(text, "- `ignite {verb}`: {help}");
    }
    text.push_str(
        "\nRun `ignite guide <topic>` for everything else; a command names the topic when it applies.\n",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../ignite.toml"))
                .expect("ignite.toml");
        Config::parse("ignite.toml", &text).expect("config parses")
    }

    #[test]
    fn prime_fits_in_sixty_lines() {
        let text = prime(&config());
        assert!(
            text.lines().count() < MAX_PRIME_LINES,
            "{}",
            text.lines().count()
        );
    }

    #[test]
    fn prime_holds_each_rule_and_names_the_guide() {
        let text = prime(&config());
        for phrase in [
            "Nobody answers questions",
            "may be days old",
            "files the ticket names",
            "smallest change",
            "Run every build, test suite and other heavy command through Ignite",
            "never depend on the machine's speed",
            "Ordinary difficulty is the job",
            "Never wait for a person, for another ticket, for another machine or for verification",
            "verdict says whose failure it is",
            "never decide yourself that a failure is unrelated",
            "nothing will report",
            "Never poll with `sleep` or a loop",
            "Never work in another session's tree",
            "A `changed` line is an instruction about your ticket",
            "landing note says",
            "The title shows the state",
            "`ignite guide <topic>`",
        ] {
            let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(flat.contains(phrase), "missing: {phrase}");
        }
    }

    #[test]
    fn prime_never_implies_a_lifetime_cap_on_the_pool() {
        assert!(!prime(&config()).contains("in its life"));
    }
}
