//! Every verb and flag of the `ignite` command.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Builds, checks and delivery for agents. Run `ignite prime` for the whole workflow.
#[derive(Debug, Parser)]
#[command(name = "ignite", version)]
pub struct Cli {
    /// Print the result as one JSON object.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Claim a ticket (or file one from a quoted summary), lease a build tree at the tip and
    /// print the ticket.
    Start {
        /// A ticket ID, or a summary for a new ticket.
        ticket: String,
        /// The code of the reservation a hand-off made for this session.
        #[arg(long)]
        reservation: Option<String>,
    },
    /// Print the ticket's brief again.
    Brief,
    /// Run the named checks in this tree.
    Test {
        #[arg(required = true)]
        names: Vec<String>,
        /// Run again even where the result cache has an answer.
        #[arg(long)]
        rerun: bool,
    },
    /// Run what the landing gate would run on the tree as it is.
    Check {
        /// Print which checks the gate would choose, and why, without running them.
        #[arg(long)]
        plan: bool,
        #[arg(long)]
        rerun: bool,
    },
    /// Rebase the tree's branch onto the tip.
    Sync,
    /// Rebase, run the gate, land the commits, close the ticket and release the tree.
    Land {
        /// The note dependent tickets will read.
        #[arg(short = 'm', long = "message")]
        note: String,
    },
    /// Give the ticket back unfinished: save the tree's work as `ignite/<ticket>` and release
    /// the tree.
    Release {
        /// The ticket's exact state, for whoever starts it next.
        #[arg(short = 'm', long = "message")]
        note: Option<String>,
        /// What the ticket needs: a machine's name, `macos`, or `person` (which needs a note).
        #[arg(long)]
        needs: Option<String>,
    },
    /// Close the ticket without landing anything, and release the tree.
    Close {
        /// Why it needs no landing: obsolete, already done, or no change to the repository.
        #[arg(short = 'm', long = "message")]
        reason: String,
    },
    /// Run any other heavy command with cores from the daemon, and record it.
    Run {
        #[arg(long, value_enum, default_value_t = ClassArg::Agent)]
        class: ClassArg,
        #[arg(long)]
        ticket: Option<String>,
        /// Cores wanted; the grant may be smaller.
        #[arg(long)]
        cores: Option<u32>,
        /// The command rereads the file `IGNITE_CORES_FILE` names while it runs, so its grant
        /// may grow as cores free up.
        #[arg(long)]
        grows: bool,
        /// The command, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Block until a running operation finishes.
    Wait { handle: String },
    /// End a running command of this session's, such as a landing before its push begins.
    Stop { handle: String },
    /// Create, show, list, link and comment on tickets.
    Ticket {
        #[command(subcommand)]
        command: Box<TicketCommand>,
    },
    /// Print a check's record: its cost, its failures, where it runs and why.
    Why {
        check: String,
        /// List the inputs its hash covers.
        #[arg(long)]
        inputs: bool,
    },
    /// Print the machine's state.
    Status {
        /// Show the cores admitted and the tickets waiting for them.
        #[arg(long)]
        admission: bool,
        /// Show each exploration family's finds and share.
        #[arg(long)]
        exploration: bool,
    },
    /// Check the machine's health and say what to fix.
    Doctor,
    /// Print the whole workflow.
    Prime,
    /// List the guide topics, or print one: the rules for one situation.
    Guide {
        /// The topic, such as `review`.
        topic: Option<String>,
    },
    /// Build the executable, register the machine with the hub and start the daemon at login.
    Install,
    /// Wait for the dispatcher's next instruction, or acknowledge one.
    #[command(args_conflicts_with_subcommands = true)]
    Dispatch {
        #[command(subcommand)]
        command: Option<DispatchCommand>,
        /// Seconds to wait for something to do.
        #[arg(long, default_value_t = 540)]
        wait: u32,
    },
    /// The machine's pool of worker sessions.
    Pool {
        #[command(subcommand)]
        command: PoolCommand,
    },
    /// Comment on a ticket and send the text to the session working it now:
    /// `ignite tell <ticket | session> "<text>"` or `ignite tell --all "<text>"`.
    Tell {
        /// Every ticket in progress, on every machine.
        #[arg(long)]
        all: bool,
        /// The ticket or session, then the text; only the text with `--all`.
        #[arg(required = true, num_args = 1..=2)]
        words: Vec<String>,
    },
    /// Ask for a justiciar, for an emergency that stops work and no ordinary ticket fixes:
    /// `ignite justiciar [--here] "<what is broken, and the evidence>"`.
    Justiciar {
        /// Only this machine can work it, such as for its disk or its tools.
        #[arg(long)]
        here: bool,
        /// What is broken, and the evidence.
        #[arg(required = true)]
        reason: Vec<String>,
    },
    /// Check an epic's ticket graph.
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
    /// The landing gate's values.
    Gate {
        #[command(subcommand)]
        command: GateCommand,
    },
    /// List flaky checks, or switch a check on or off by hand.
    Flaky {
        #[command(subcommand)]
        command: crate::flaky::FlakyCommand,
    },
    /// Measure the rebuild cost of a named edit.
    Rebuild {
        name: Option<String>,
        /// List the named edits.
        #[arg(long)]
        list: bool,
        /// Print the stored counts instead of measuring.
        #[arg(long)]
        last: bool,
    },
    /// Rerun a sample of cache hits to check the cache.
    Audit {
        /// Run a sample now instead of in idle time.
        #[arg(long)]
        now: bool,
    },
    /// Check the check manifests against the checkout.
    Manifest {
        #[command(subcommand)]
        command: ManifestCommand,
    },
    /// Import the old systems' data into the hub's database. Safe to run twice.
    Import {
        /// The hub's database file, created if it does not exist.
        #[arg(long)]
        hub: PathBuf,
        /// The old ticket service's database file, which is only read.
        #[arg(long)]
        tickets: Option<PathBuf>,
        /// The folder of the old run log's `*.jsonl` files. Verdicts are read from the
        /// tickets, so import those first or in the same command.
        #[arg(long, alias = "runlog")]
        run_log: Option<PathBuf>,
        /// The old efficiency store, whose `compile_timings` table holds per-crate timings.
        #[arg(long)]
        compile_timings: Option<PathBuf>,
        /// An old lane whose checks are exploration, left out of the check records; repeatable.
        #[arg(long)]
        exploration: Vec<String>,
        /// Report what would be written, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Check once that the daemon answers; kick it, or go back to the last-good build, if not.
    Monitor,
    /// Run the daemon.
    #[command(hide = true)]
    Daemon,
    /// Serve the hub at `IGNITE_HUB_LISTEN` (and `IGNITE_HUB_PUBLIC_LISTEN`), as the hub's login
    /// service does on the hub's machine.
    #[command(hide = true)]
    Hub,
    /// Run one command as its supervisor.
    #[command(hide = true)]
    Supervise { spec: PathBuf },
    /// Cargo's compiler wrapper.
    #[command(hide = true, name = "rustc-wrap")]
    RustcWrap(Passthrough),
    /// Cargo's test runner.
    #[command(hide = true)]
    Runner(Passthrough),
    /// Checks this executable on this machine, for the daemon that is about to restart into it.
    #[command(hide = true)]
    Internal {
        #[command(subcommand)]
        command: InternalCommand,
    },
}

/// The subcommands of `ignite internal`, which Ignite runs on itself.
#[derive(Debug, Subcommand)]
pub enum InternalCommand {
    /// Parse `ignite.toml` and the check manifests and migrate a copy of `daemon.db`, as a
    /// daemon starting now would; exit 0 if all of it works.
    Selftest,
    /// Run the verb of a running handle in this process, from start to finish (a verb process),
    /// and write its reply into the handle's row.
    Verb { handle: String },
    /// Make the running daemon restart into its own executable, as it does into a new release,
    /// and wait until the new daemon answers. A drill: refused to a session that holds a tree.
    Restart,
}

/// Arguments passed through untouched.
#[derive(Debug, Args)]
pub struct Passthrough {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ClassArg {
    Land,
    Agent,
    Tip,
    Batch,
    Idle,
}

/// How an instruction ended: `ok` or `failed` for a hand-off or message, and for a `check`
/// what the app said of the session (`unknown` when the lookup itself failed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AckResult {
    Ok,
    #[value(alias = "fail")]
    Failed,
    Running,
    Idle,
    Gone,
    Unknown,
}

#[derive(Debug, Subcommand)]
pub enum ManifestCommand {
    /// Print every input pattern that matches no tracked file and every `crate:` that names
    /// no workspace package; exit 1 if there is any.
    Lint,
}

#[derive(Debug, Subcommand)]
pub enum DispatchCommand {
    /// Report that an instruction was carried out.
    Ack {
        id: u64,
        result: AckResult,
        /// Why it failed.
        #[arg(trailing_var_arg = true)]
        reason: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum PoolCommand {
    /// List the pool's sessions and what each holds.
    List,
    /// Write the folder a person opens the dispatcher session in (default `~/ignite`).
    Init {
        /// The folder to write.
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
    },
    /// Add sessions to the pool. With `--dispatcher` and no session, adds the calling session.
    Add {
        sessions: Vec<String>,
        /// `claude` or `codex`.
        #[arg(long)]
        app: String,
        /// Add the session as this app's dispatcher on the machine, replacing any earlier one
        /// and removing the workers that one started.
        #[arg(long)]
        dispatcher: bool,
        /// The model tiers the app runs, comma-separated; by default `opus,sonnet,light` for
        /// `claude` and none for `codex`.
        #[arg(long, value_delimiter = ',')]
        models: Option<Vec<String>>,
    },
    /// Remove a session from the pool.
    Remove { session: String },
    /// Stop handing off new tickets; running workers finish.
    Pause {
        /// On every machine.
        #[arg(long)]
        all: bool,
    },
    /// Start handing off tickets again.
    Resume {
        /// On every machine.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlanCommand {
    /// Report the graph's shape: chains, suspect edges, tickets that name no files.
    Check { epic: String },
}

#[derive(Debug, Subcommand)]
pub enum GateCommand {
    /// Recompute the gate's threshold from the last days of history. Changes nothing.
    Replay {
        #[arg(long, default_value_t = 14)]
        days: u32,
        /// The hub's database file, which holds the check records, results, regressions and
        /// landings. It is opened read-only.
        #[arg(long)]
        hub: PathBuf,
    },
}

/// Fields shared by `ticket create` and `ticket update`.
#[derive(Debug, Args)]
pub struct TicketFieldArgs {
    /// P0 (highest) to P4.
    #[arg(short = 'p', long, value_parser = parse_priority)]
    pub priority: Option<u8>,
    /// task, epic, bug, feature, chore, decision, spike, story or milestone.
    #[arg(long = "type")]
    pub ticket_type: Option<String>,
    #[arg(long)]
    pub parent: Option<String>,
    /// Read the body from a file.
    #[arg(long)]
    pub body_file: Option<PathBuf>,
    #[arg(long)]
    pub acceptance: Option<String>,
    /// An epic's target number of workers.
    #[arg(long)]
    pub workers: Option<u32>,
    /// A hard cap on an epic's workers.
    #[arg(long)]
    pub max_workers: Option<u32>,
    /// The cores the ticket is expected to use.
    #[arg(long)]
    pub cores: Option<f64>,
}

#[derive(Debug, Subcommand)]
pub enum TicketCommand {
    /// File a ticket.
    Create {
        title: String,
        #[command(flatten)]
        fields: TicketFieldArgs,
        /// Comma-separated labels.
        #[arg(long, value_delimiter = ',')]
        labels: Vec<String>,
        /// Comma-separated tickets the new one depends on.
        #[arg(long, value_delimiter = ',')]
        deps: Vec<String>,
    },
    /// Show one or more tickets.
    Show {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long)]
        comments: bool,
        #[arg(long)]
        history: bool,
    },
    /// List tickets.
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        ancestor: Option<String>,
        #[arg(long)]
        label: Vec<String>,
        #[arg(long)]
        limit: Option<u32>,
        /// Include closed tickets.
        #[arg(long)]
        all: bool,
    },
    /// List ready tickets.
    Ready {
        #[arg(long)]
        ancestor: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        /// One line per ticket.
        #[arg(long)]
        brief: bool,
    },
    /// Change a ticket.
    Update {
        id: String,
        #[command(flatten)]
        fields: TicketFieldArgs,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        add_label: Vec<String>,
        #[arg(long)]
        remove_label: Vec<String>,
    },
    /// Close a ticket.
    Close {
        id: String,
        #[arg(long)]
        reason: String,
    },
    /// Reopen a closed ticket.
    Reopen { id: String },
    /// Defer a ticket until a time.
    Defer {
        id: String,
        /// An RFC 3339 time, such as 2026-11-20T09:00:00Z.
        #[arg(long)]
        until: String,
    },
    /// Comment on a ticket.
    Comment { id: String, text: String },
    /// Add, remove or list dependencies.
    Dep {
        #[command(subcommand)]
        command: DepCommand,
    },
    /// Block until a ticket reaches a status. Exits 5 when the wait runs out.
    Wait {
        id: String,
        #[arg(long)]
        status: String,
        /// Seconds to wait before giving up.
        #[arg(long, default_value_t = 540)]
        timeout: u32,
    },
    /// List a ticket's children, closed ones included.
    Children {
        id: String,
        /// Only the children that are not closed, each with its assignee, what it waits on and
        /// the first line of its last comment.
        #[arg(long)]
        open: bool,
    },
    /// Find tickets by words in their title or text.
    Search {
        #[arg(required = true)]
        words: Vec<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Print the dependency graph.
    Graph {
        /// Include closed tickets.
        #[arg(long)]
        all: bool,
    },
    /// Take a ticket, as the acting session.
    Claim { id: String },
    /// Print every ticket, with its edges, comments and history, as JSON lines.
    Export,
    /// Apply a JSON batch file in one request that succeeds or fails as a whole.
    Batch { file: PathBuf },
}

#[derive(Debug, Subcommand)]
pub enum DepCommand {
    /// `id` depends on `on`.
    Add {
        id: String,
        on: String,
        #[arg(long)]
        kind: Option<String>,
    },
    Remove {
        id: String,
        on: String,
    },
    /// What `id` waits on and what waits on it, with each one's status and title.
    #[command(visible_alias = "ls")]
    List {
        id: String,
    },
}

fn parse_priority(text: &str) -> Result<u8, String> {
    let digits = text.strip_prefix(['P', 'p']).unwrap_or(text);
    match digits.parse::<u8>() {
        Ok(priority) if priority <= 4 => Ok(priority),
        _ => Err(format!("{text:?} is not a priority from P0 to P4")),
    }
}

impl Command {
    /// The verb's name, as the client sends it to the daemon.
    pub fn verb(&self) -> &'static str {
        match self {
            Command::Start { .. } => "start",
            Command::Brief => "brief",
            Command::Close { .. } => "close",
            Command::Test { .. } => "test",
            Command::Check { .. } => "check",
            Command::Sync => "sync",
            Command::Land { .. } => "land",
            Command::Release { .. } => "release",
            Command::Run { .. } => "run",
            Command::Wait { .. } => "wait",
            Command::Stop { .. } => "stop",
            Command::Ticket { .. } => "ticket",
            Command::Why { .. } => "why",
            Command::Status { .. } => "status",
            Command::Doctor => "doctor",
            Command::Prime => "prime",
            Command::Guide { .. } => "guide",
            Command::Install => "install",
            Command::Dispatch { .. } => "dispatch",
            Command::Pool { .. } => "pool",
            Command::Tell { .. } => "tell",
            Command::Justiciar { .. } => "justiciar",
            Command::Plan { .. } => "plan",
            Command::Gate { .. } => "gate",
            Command::Flaky { .. } => "flaky",
            Command::Rebuild { .. } => "rebuild",
            Command::Audit { .. } => "audit",
            Command::Manifest { .. } => "manifest",
            Command::Import { .. } => "import",
            Command::Monitor => "monitor",
            Command::Daemon => "daemon",
            Command::Hub => "hub",
            Command::Supervise { .. } => "supervise",
            Command::RustcWrap(_) => "rustc-wrap",
            Command::Runner(_) => "runner",
            Command::Internal { .. } => "internal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn help_lists_every_agent_verb() {
        let help = Cli::command().render_help().to_string();
        for verb in [
            "start", "test", "check", "sync", "land", "run", "ticket", "why", "status", "doctor",
            "prime", "wait",
        ] {
            assert!(
                help.lines().any(|l| l.trim_start().starts_with(verb)),
                "{verb} in {help}"
            );
        }
        assert!(
            !help.contains("rustc-wrap"),
            "hidden subcommands stay hidden"
        );
    }

    #[test]
    fn parses_the_documented_commands() {
        let parse = |line: &str| {
            Cli::try_parse_from(line.split_whitespace())
                .unwrap()
                .command
        };
        assert!(matches!(parse("ignite land -m note"), Command::Land { note } if note == "note"));
        assert!(matches!(parse("ignite stop L-2"), Command::Stop { handle } if handle == "L-2"));
        assert!(matches!(
            parse("ignite start Tq44s.9.48 --reservation 7f3a"),
            Command::Start { ticket, reservation: Some(code) } if ticket == "Tq44s.9.48" && code == "7f3a"
        ));
        assert!(matches!(
            parse("ignite release -m blocked --needs person"),
            Command::Release { note: Some(_), needs: Some(needs) } if needs == "person"
        ));
        assert!(
            matches!(parse("ignite close -m obsolete"), Command::Close { reason } if reason == "obsolete")
        );
        let Command::Run {
            class,
            ticket,
            command,
            ..
        } = parse("ignite run --class batch --ticket Tix1t.12 -- just tournament --games 2000")
        else {
            panic!("not run");
        };
        assert_eq!(class, ClassArg::Batch);
        assert_eq!(ticket.as_deref(), Some("Tix1t.12"));
        assert_eq!(command, ["just", "tournament", "--games", "2000"]);
        let Command::Ticket { command } = parse(
            "ignite ticket create Split -p P2 --parent Tig9a --labels tier:opus --deps Tig9a.1",
        ) else {
            panic!("not ticket");
        };
        let TicketCommand::Create {
            fields,
            labels,
            deps,
            ..
        } = *command
        else {
            panic!("not ticket create");
        };
        assert_eq!(fields.priority, Some(2));
        assert_eq!(
            (labels, deps),
            (vec!["tier:opus".to_owned()], vec!["Tig9a.1".to_owned()])
        );
        assert!(matches!(
            parse("ignite dispatch ack 4812 ok"),
            Command::Dispatch {
                command: Some(DispatchCommand::Ack { id: 4812, .. }),
                ..
            }
        ));
        assert!(matches!(
            parse("ignite dispatch --wait 540"),
            Command::Dispatch { wait: 540, .. }
        ));
        let Command::Dispatch {
            command: Some(DispatchCommand::Ack { result, reason, .. }),
            ..
        } = parse("ignite dispatch ack 4813 failed clear refused")
        else {
            panic!("not ack");
        };
        assert_eq!(
            (result, reason.join(" ")),
            (AckResult::Failed, "clear refused".to_owned())
        );
        assert!(matches!(
            parse("ignite tell --all Skip"),
            Command::Tell { all: true, words } if words == ["Skip"]
        ));
        assert!(matches!(
            parse("ignite pool pause --all"),
            Command::Pool {
                command: PoolCommand::Pause { all: true }
            }
        ));
        assert!(matches!(
            parse("ignite status --admission"),
            Command::Status {
                admission: true,
                ..
            }
        ));
        assert!(matches!(
            parse("ignite rustc-wrap rustc --crate-name x"),
            Command::RustcWrap(Passthrough { args }) if args.len() == 3
        ));
        assert!(matches!(
            parse("ignite internal selftest"),
            Command::Internal {
                command: InternalCommand::Selftest
            }
        ));
        assert!(Cli::try_parse_from(["ignite", "ticket", "create", "x", "-p", "P7"]).is_err());
    }

    #[test]
    fn parses_the_verb_process_command() {
        let line = ["ignite", "internal", "verb", "L-6"];
        assert!(matches!(
            Cli::try_parse_from(line).unwrap().command,
            Command::Internal {
                command: InternalCommand::Verb { handle }
            } if handle == "L-6"
        ));
        assert!(matches!(
            Cli::try_parse_from(["ignite", "internal", "restart"])
                .unwrap()
                .command,
            Command::Internal {
                command: InternalCommand::Restart
            }
        ));
    }
}
