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
    },
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
    /// Give the ticket back without landing it, and release the tree.
    Release {
        #[arg(long)]
        reason: Option<String>,
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
        /// The command, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Block until a running operation finishes.
    Wait { handle: String },
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
    /// Measure the rebuild cost of a named edit.
    Rebuild {
        name: Option<String>,
        /// List the named edits.
        #[arg(long)]
        list: bool,
    },
    /// Rerun a sample of cache hits to check the cache.
    Audit {
        /// Run a sample now instead of in idle time.
        #[arg(long)]
        now: bool,
    },
    /// Import the old systems' data into the hub's database.
    Import {
        /// The old ticket service's database file.
        #[arg(long)]
        tickets: Option<PathBuf>,
        /// The old run log.
        #[arg(long)]
        run_log: Option<PathBuf>,
        /// The recorded per-crate compile timings.
        #[arg(long)]
        compile_timings: Option<PathBuf>,
    },
    /// Run the daemon.
    #[command(hide = true)]
    Daemon,
    /// Run one command as its supervisor.
    #[command(hide = true)]
    Supervise { spec: PathBuf },
    /// Cargo's compiler wrapper.
    #[command(hide = true, name = "rustc-wrap")]
    RustcWrap(Passthrough),
    /// Cargo's test runner.
    #[command(hide = true)]
    Runner(Passthrough),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AckResult {
    Ok,
    Fail,
}

#[derive(Debug, Subcommand)]
pub enum DispatchCommand {
    /// Report that an instruction was carried out.
    Ack { id: u64, result: AckResult },
}

#[derive(Debug, Subcommand)]
pub enum PoolCommand {
    /// List the pool's sessions and what each holds.
    List,
    /// Add sessions to the pool.
    Add {
        #[arg(required = true)]
        sessions: Vec<String>,
        /// `claude` or `codex`.
        #[arg(long)]
        app: String,
        /// Add them as the dispatcher instead of workers.
        #[arg(long)]
        dispatcher: bool,
    },
    /// Remove a session from the pool.
    Remove { session: String },
}

#[derive(Debug, Subcommand)]
pub enum PlanCommand {
    /// Report the graph's shape: chains, suspect edges, tickets that name no files.
    Check { epic: String },
}

#[derive(Debug, Subcommand)]
pub enum GateCommand {
    /// Recompute the threshold from the last days of history.
    Replay {
        #[arg(long, default_value_t = 14)]
        days: u32,
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
    /// Show a ticket.
    Show {
        id: String,
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
    /// Add or remove a dependency.
    Dep {
        #[command(subcommand)]
        command: DepCommand,
    },
    /// Block until a ticket reaches a status.
    Wait {
        id: String,
        #[arg(long)]
        status: String,
    },
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
            Command::Test { .. } => "test",
            Command::Check { .. } => "check",
            Command::Sync => "sync",
            Command::Land { .. } => "land",
            Command::Release { .. } => "release",
            Command::Run { .. } => "run",
            Command::Wait { .. } => "wait",
            Command::Ticket { .. } => "ticket",
            Command::Why { .. } => "why",
            Command::Status { .. } => "status",
            Command::Doctor => "doctor",
            Command::Prime => "prime",
            Command::Install => "install",
            Command::Dispatch { .. } => "dispatch",
            Command::Pool { .. } => "pool",
            Command::Plan { .. } => "plan",
            Command::Gate { .. } => "gate",
            Command::Rebuild { .. } => "rebuild",
            Command::Audit { .. } => "audit",
            Command::Import { .. } => "import",
            Command::Daemon => "daemon",
            Command::Supervise { .. } => "supervise",
            Command::RustcWrap(_) => "rustc-wrap",
            Command::Runner(_) => "runner",
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
        assert!(Cli::try_parse_from(["ignite", "ticket", "create", "x", "-p", "P7"]).is_err());
    }
}
