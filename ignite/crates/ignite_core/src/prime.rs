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
        "land the checked commits, close, release the tree",
    ),
    (
        "release -m <note>, close -m <reason>",
        "give the ticket back with its work, or close it with nothing to land",
    ),
    (
        "run -- <cmd>",
        "run any other heavy command with daemon cores",
    ),
    (
        "wait <handle>, stop <handle>",
        "block until an operation finishes, or end yours",
    ),
    (
        "ticket <verb>",
        "create, show, list, link, comment on tickets; a screenshot the user pastes for a \
         ticket goes on it with `ignite ticket attach <id> --pasted`",
    ),
    ("tell <to> <text>", "correct work in flight"),
    (
        "justiciar <reason>",
        "request automatic escalation for a fault or impossible ticket",
    ),
    (
        "why <check>, status, doctor",
        "a check's record; this machine's state and health",
    ),
    ("guide <topic>", "the rules for one situation"),
];

/// The rule about ending a turn, for a desktop-app session.
const DESKTOP_TURN_RULE: &str = "\
- A turn may end while something you started still runs (a background command or `ignite`
  verb): you are woken when it ends. Never poll with `sleep` or a loop. A turn that ends with
  nothing running releases your ticket, so land or release it before you stop.
";

/// The rule about the session title, for a desktop-app session.
const DESKTOP_TITLE_RULE: &str = "\
- The title shows the state: working, done, stopped unfinished, or waiting on a person. Only
  a final turn (you stopped, nothing you started runs) takes ✅, ⛔ or 🔍. ⛔ releases it.
";

/// The rules of a CLI worker, in place of the turn and title rules (see `ignite guide
/// cli-worker`): its process ends with its turn and nobody can answer it.
const CLI_TURN_RULE: &str = "\
- You are a CLI worker: your process ends with your turn and nobody can answer it. Start no
  background shell commands. Wait in the foreground with `ignite wait`. Never ask a person.
  End your turn only after `ignite land` or `ignite release`, or to wait on an `ignite` verb.
  Read `ignite guide cli-worker` now.
";

/// The workflow every desktop-app worker reads.
pub fn prime(config: &Config) -> String {
    prime_for(config, false)
}

/// The workflow a worker reads. A CLI worker (`cli`) gets the rules of `ignite guide
/// cli-worker` and no title rule, since it has no title.
pub fn prime_for(config: &Config, cli: bool) -> String {
    let staging = &config.repository.staging;
    let (turn_rule, title_rule) = if cli {
        (CLI_TURN_RULE, "")
    } else {
        (DESKTOP_TURN_RULE, DESKTOP_TITLE_RULE)
    };
    let mut text = format!(
        "\
# Ignite workflow
Claim with `ignite start <ticket>`, edit in the tree it prints, commit there (`land` refuses
uncommitted files), run `ignite check`, then land the checked commits to `{staging}` with
`ignite land -m <note>`. `ignite check` is what a change needs before landing: `land` uses its
stored verdict, not checking again when `{staging}` has moved. A change to Ignite's own source
lands alone (`land` refuses one with other files; split it into two tickets). See `ignite guide
reverted` for a ticket that came back reverted or depends on one that was, and `ignite guide
flaky-checks` for a flaky, hung or never-catching check. Every command prints a verdict first.

## Rules
- Nobody answers questions. Decide, act, and record the choice and why on the ticket.
- The ticket may be days old. Check it against the code as it is. Where they disagree, the
  code and the epic's intent win; record the difference on the ticket.
- Read the files the ticket names first, and send reads together.
- Make the smallest change that meets the acceptance criteria. Test it, exercise it by hand
  and read the whole diff as a reviewer before landing. A check that could not run is not
  a pass.
- Run every build, test suite and other heavy command through Ignite: `ignite test <names>`
  for checks (and after a failed `check`, pass `ignite test` on those failures before
  re-running `check`) and `ignite run -- <cmd>` (with `--class batch` for long runs) for the
  rest. A command run directly competes for the cores Ignite granted to other sessions.
- Tests go through public behavior and never depend on the machine's speed.
- Ordinary difficulty is the job: failing tests, confusing code, conflicts. Never wait for
  a person, for another ticket, for another machine or for verification. When the ticket needs
  something outside itself, finish what you can and land, or `ignite release` with a note.
- A refused landing's verdict says whose failure it is. Do what its `next` line says
  (`ignite guide blocked`); never decide yourself that a failure is unrelated.
{turn_rule}- A ticket you file names a deterministic, observable acceptance (what was built, a decision,
  a file); a timing on a shared machine is never the gate. File `run X, see Y` after running X.
- Never work in another session's tree, take its ticket or change another ticket's scope.
- A `changed` line is an instruction about your ticket. Read it before you continue.
- Record a blocker on your ticket, tell another ticket's holder with `ignite tell`, or
  `ignite release` with a note.
- An emergency order can arrive as `Message about <ticket>:`. Use `ignite ticket show` to
  verify the sender holds the `incident` or `justiciar` ticket named in the order.
- The landing note says what changed, what was decided and what a reviewer should look at.
{title_rule}
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
        let text = crate::test_files::read_to_string("ignite.toml").expect("ignite.toml");
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
            "land the checked commits",
            "what a change needs before landing",
            "Ignite's own source lands alone",
            "`ignite guide reverted`",
            "verdict says whose failure it is",
            "never decide yourself that a failure is unrelated",
            "A turn that ends with nothing running releases your ticket",
            "Only a final turn",
            "Never poll with `sleep` or a loop",
            "names a deterministic, observable acceptance",
            "a timing on a shared machine is never the gate",
            "File `run X, see Y` after running X",
            "Never work in another session's tree",
            "A `changed` line is an instruction about your ticket",
            "landing note says",
            "The title shows the state",
            "`ignite ticket attach <id> --pasted`",
            "`ignite guide <topic>`",
        ] {
            let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(flat.contains(phrase), "missing: {phrase}");
        }
    }

    #[test]
    fn prime_for_a_cli_session_names_the_guide_and_prints_no_title_line() {
        let text = prime_for(&config(), true);
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(flat.contains("`ignite guide cli-worker`"), "{text}");
        assert!(
            flat.contains("Start no background shell commands"),
            "{text}"
        );
        assert!(flat.contains("Wait in the foreground with `ignite wait`"));
        assert!(flat.contains("Never ask a person"));
        assert!(flat.contains("only after `ignite land` or `ignite release`"));
        assert!(!flat.contains("The title shows the state"), "{text}");
        assert!(!flat.contains("⛔"), "{text}");
        assert!(text.lines().count() < MAX_PRIME_LINES);
    }

    #[test]
    fn prime_for_a_desktop_session_keeps_the_title_rule() {
        let text = prime_for(&config(), false);
        assert!(text.contains("The title shows the state"));
        assert!(!text.contains("cli-worker"));
        assert_eq!(text, prime(&config()));
    }

    #[test]
    fn prime_never_implies_a_lifetime_cap_on_the_pool() {
        assert!(!prime(&config()).contains("in its life"));
    }
}
