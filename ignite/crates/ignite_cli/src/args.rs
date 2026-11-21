//! Every verb and flag of the `ignite` command.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::values::{parse_every, parse_priority};

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
        /// The note dependent tickets will read; the landed commits' subjects when omitted.
        #[arg(short = 'm', long = "message")]
        note: Option<String>,
    },
    /// Give the ticket back unfinished: save the tree's work on the remote and release
    /// the tree.
    Release {
        /// The ticket's exact state, for whoever starts it next.
        #[arg(short = 'm', long = "message")]
        note: Option<String>,
        /// What the ticket needs: `person` (which needs a note) sets it aside for a person, an
        /// operating system such as `windows` records the system it needs, and a machine, as
        /// `<machine>` or `<machine>/<app>`, becomes its owner.
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
    /// Block until a running operation finishes, or with `emergency` until the emergency ends.
    Wait { handle: String },
    /// End a running command of this session's, such as a landing before its push begins.
    Stop {
        handle: String,
        /// End a command of any session's, or a daemon's internal verb, and its verb process if
        /// it does not stop when asked. Only for a justiciar or the operator.
        #[arg(long)]
        force: bool,
    },
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
        /// Seconds to wait for something to do. Run it in the background: a foreground tool
        /// call is cut off after 600 s.
        #[arg(long, default_value_t = ignite_core::protocol::client::DISPATCH_WAIT_S)]
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
        /// End the CLI worker's running turn and start the next one with the text, instead of
        /// waiting for the turn to end. Only for CLI workers.
        #[arg(long)]
        now: bool,
        /// The ticket or session, then the text; only the text with `--all`.
        #[arg(required = true, num_args = 1..=2)]
        words: Vec<String>,
    },
    /// Ask for a justiciar, for an emergency that stops work and no ordinary ticket fixes, or
    /// for a ticket that cannot be completed as written:
    /// `ignite justiciar [--here] "<what is broken, and the evidence>"`.
    Justiciar {
        /// Only this machine can work it, such as for its disk or its tools.
        #[arg(long)]
        here: bool,
        /// What is broken, and the evidence.
        #[arg(required = true)]
        reason: Vec<String>,
    },
    /// The emergency pause, which refuses heavy verbs on every machine while it is in force.
    Emergency {
        #[command(subcommand)]
        command: Option<crate::emergency::EmergencyCommand>,
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
    /// service does on the hub's machine, or restore its tickets from this machine's mirror.
    Hub {
        #[command(subcommand)]
        command: Option<HubCommand>,
    },
    /// Run one command as its supervisor.
    #[command(hide = true)]
    Supervise { spec: PathBuf },
    /// Cargo's compiler wrapper.
    #[command(hide = true, name = "rustc-wrap")]
    RustcWrap(Passthrough),
    /// Cargo's test runner.
    #[command(hide = true)]
    Runner(Passthrough),
    /// What a pool worker's `Stop` hook reports at the end of a turn: `pass`, `block` with the
    /// text to keep the worker going, or `released`.
    #[command(hide = true, name = "turn-ended")]
    TurnEnded {
        #[arg(long)]
        session: String,
        /// The hook already kept the worker going once this turn.
        #[arg(long)]
        again: bool,
        /// The worker's last message, the release note when it stops again.
        #[arg(long)]
        note: Option<String>,
        /// The session's own process, the hook's parent: a turn that ends with a process still
        /// running under it is waiting, not stopping.
        #[arg(long)]
        parent: Option<u32>,
    },
    /// Checks this executable on this machine, for the daemon that is about to restart into it.
    #[command(hide = true)]
    Internal {
        #[command(subcommand)]
        command: InternalCommand,
    },
}

/// The subcommands of `ignite hub`. Without one, it serves the hub.
#[derive(Debug, Subcommand)]
pub enum HubCommand {
    /// Send the hub its tickets as this machine's ticket mirror recorded them: as of `--at`,
    /// replacing later copies, or the newest of every ticket to seed a new hub.
    Restore {
        /// Restore from this machine's ticket mirror (`mirror.db` in the Ignite home), the only
        /// source there is.
        #[arg(long, required = true)]
        from_mirror: bool,
        /// Roll each ticket back to how it stood at this time, `YYYY-MM-DDTHH:MM:SSZ` in UTC.
        #[arg(long)]
        at: Option<String>,
        /// Roll back only the tickets this actor changed after `--at`.
        #[arg(long)]
        actor: Option<String>,
        /// Say what would be sent, and send nothing.
        #[arg(long)]
        dry_run: bool,
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
    /// Take the turn lock of the CLI session `IGNITE_CLI_TURN` names, then become the CLI
    /// program given after `--`: one turn of a worker in CLI mode, run by its supervisor.
    CliTurn {
        /// The Ignite home, which holds the session's turn lock.
        #[arg(long)]
        home: std::path::PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Make the running daemon restart into its own executable, as it does into a new release,
    /// and wait until the new daemon answers. A drill: refused to a session that holds a tree.
    Restart,
    /// Run one headless justiciar turn for `ignite monitor`: hold `lock` while `command` runs,
    /// with its output in `log`.
    JusticiarTurn {
        lock: std::path::PathBuf,
        log: std::path::PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
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

/// How an instruction ended: `ok` or `failed` for a hand-off or message. `raw` carries what the
/// app's `get_session` returned, for a `check` or a hand-off's read-back, and the daemon decides
/// what it means. A Codex dispatcher answers a `check` itself (`unknown` when the lookup failed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AckResult {
    Ok,
    #[value(alias = "fail")]
    Failed,
    Raw,
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
        /// Why it failed, or the raw `get_session` result.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        reason: Vec<String>,
    },
    /// Exit 0 only when the current instruction calls for a session tool on a session. With no
    /// tool, reads a Claude Code hook's JSON from stdin and exits 2 to block the call.
    Allowed {
        /// The tool, such as `mcp__ccd_session_mgmt__clear_session`.
        tool: Option<String>,
        /// The session it acts on.
        session: Option<String>,
    },
    /// Report that a `clear_session` on a session succeeded. With no session, reads a Claude
    /// Code `PostToolUse` hook's JSON from stdin.
    Cleared {
        /// The session that was cleared.
        session: Option<String>,
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
        /// For a Codex dispatcher: how many tickets it may run at once, each in a new chat.
        /// 0 keeps the count the machine's Codex dispatcher already had.
        #[arg(long)]
        slots: Option<u32>,
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
    /// Choose how an app's workers run on this machine: `cli` (headless processes), `desktop`
    /// (a dispatcher session in the app) or `config` (remove the override so `ignite.toml`
    /// decides). A ticket in progress finishes as it started.
    Mode {
        /// `claude` or `codex`.
        app: String,
        #[arg(value_parser = ["cli", "desktop", "config"])]
        setting: String,
    },
    /// Print the newest sample of each Claude plan window, and the usage hold if one is in
    /// force.
    Usage,
    /// Print the digest of a CLI worker's transcript (what it said, its tool calls and how each
    /// turn ended) and follow it until interrupted.
    Watch {
        /// The ticket the worker holds or last held, or its session.
        target: String,
        /// Print what exists and exit.
        #[arg(long)]
        no_follow: bool,
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
    /// task, epic, bug, feature, chore, decision, spike, story, milestone or meta (a scheduled
    /// ticket; see `--every`).
    #[arg(long = "type")]
    pub ticket_type: Option<String>,
    #[arg(long)]
    pub parent: Option<String>,
    /// Read the body from a file.
    #[arg(long)]
    pub body_file: Option<PathBuf>,
    #[arg(long)]
    pub acceptance: Option<String>,
    /// Read the notes from a file, replacing any the ticket has. Notes are for what later
    /// readers of the ticket should know that the body does not say. A meta-ticket's notes
    /// are given to each copy when it is filed, so a copy can leave notes for the copies
    /// after it by writing them on the meta-ticket.
    #[arg(long)]
    pub notes_file: Option<PathBuf>,
    /// An epic's allocation: its weight when the pool divides workers between epics. 3 when an
    /// epic is filed without one, 0 to park it. `--workers` is its old name.
    #[arg(long, alias = "workers")]
    pub allocation: Option<u32>,
    /// A hard cap on an epic's workers.
    #[arg(long)]
    pub max_workers: Option<u32>,
    /// The cores the ticket is expected to use.
    #[arg(long)]
    pub cores: Option<f64>,
    /// Who works it: `<machine>/<app>` (such as `cygnus/codex`), `<machine>` for any app on that
    /// machine, or `none` to follow its epic's owner. A ticket with no owner in its line goes to
    /// `[pool] default_owner` in `ignite.toml`, whichever machine filed it.
    #[arg(long)]
    pub owner: Option<String>,
    /// Make it a meta-ticket, a template the hub copies into a new child ticket on this
    /// schedule: hours (`6` or `6h`) or days (`2d`). A copy is filed when the meta-ticket is
    /// created and then at each due time, never while an earlier copy is still open. On create
    /// it implies `--type meta`, whose ID starts with `M`.
    #[arg(long, value_parser = parse_every)]
    pub every: Option<u32>,
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
        /// Comma-separated tickets the new one depends on. The hub also adds a dependency on each
        /// unlanded ticket you hold under the same parent; see `--independent`.
        #[arg(long, value_delimiter = ',')]
        deps: Vec<String>,
        /// A follow-up that does not need your own unlanded change. Without it the hub makes the
        /// new ticket depend on every ticket you hold in progress, unlanded, under the same
        /// parent (or as its parent), so the pool does not hand it out before that change lands.
        #[arg(long)]
        independent: bool,
        /// File it released for implementation even if it is an epic or under a pending one.
        /// Without this flag an epic, and a ticket under a pending epic, are filed
        /// `pending_approval` and the pool is never offered them until a person releases them;
        /// any other ticket is filed `open`.
        #[arg(long, conflicts_with = "pending")]
        release: bool,
        /// File it `pending_approval` even if it is a single ticket under no epic, so that the
        /// pool is never offered it until a person or an agent reviewing it releases it.
        #[arg(long)]
        pending: bool,
        /// Filed by a program, such as a check or a script, not by a person or a session's own
        /// judgement: the ticket is owned by the machine that runs this command, unless
        /// `--owner` names another. A ticket filed without it, and with no owner in its line,
        /// goes to `[pool] default_owner`.
        #[arg(long)]
        automatic: bool,
        /// An image file to attach once the ticket is filed (repeatable). The text can show it
        /// with `![caption](attachment:<file name>)`.
        #[arg(long)]
        attach: Vec<PathBuf>,
    },
    /// Attach images to a ticket: files, an image pasted into this Claude Code session
    /// (`--pasted`) or the clipboard. Adds a comment showing them unless `--no-comment`.
    Attach(crate::ticket::attach::AttachArgs),
    /// Download or list a ticket's attachments.
    Attachment {
        #[command(subcommand)]
        command: crate::ticket::attach::AttachmentCommand,
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
        /// Release the ticket, and every descendant still pending approval, for
        /// implementation: each becomes `open`, so the pool may be offered it.
        #[arg(long, conflicts_with = "status")]
        release: bool,
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
            Command::Emergency { .. } => "emergency",
            Command::Plan { .. } => "plan",
            Command::Gate { .. } => "gate",
            Command::Flaky { .. } => "flaky",
            Command::Rebuild { .. } => "rebuild",
            Command::Audit { .. } => "audit",
            Command::Manifest { .. } => "manifest",
            Command::Import { .. } => "import",
            Command::Monitor => "monitor",
            Command::Daemon => "daemon",
            Command::Hub { .. } => "hub",
            Command::Supervise { .. } => "supervise",
            Command::RustcWrap(_) => "rustc-wrap",
            Command::Runner(_) => "runner",
            Command::Internal { .. } => "internal",
            Command::TurnEnded { .. } => "turn-ended",
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
    fn hub_serves_without_a_subcommand_and_restores_with_one() {
        let parse = |line: &str| {
            Cli::try_parse_from(line.split_whitespace())
                .unwrap()
                .command
        };
        assert!(matches!(
            parse("ignite hub"),
            Command::Hub { command: None }
        ));
        assert!(matches!(
            parse("ignite hub restore --from-mirror --at 2026-11-18T10:00:00Z --actor rogue"),
            Command::Hub {
                command: Some(HubCommand::Restore {
                    at: Some(_),
                    actor: Some(_),
                    ..
                })
            }
        ));
        assert!(Cli::try_parse_from(["ignite", "hub", "restore"]).is_err());
    }

    #[test]
    fn land_needs_no_message() {
        let parsed = Cli::try_parse_from(["ignite", "land"]).unwrap().command;
        assert!(matches!(parsed, Command::Land { note: None }));
        let parsed = Cli::try_parse_from(["ignite", "land", "-m", "n"])
            .unwrap()
            .command;
        assert!(matches!(parsed, Command::Land { note: Some(_) }));
    }

    #[test]
    fn a_raw_ack_carries_the_tool_result_as_given() {
        let line = r#"ignite dispatch ack 4814 raw {"sessionId":"local_a","isRunning":true}"#;
        let Ok(Cli {
            command:
                Command::Dispatch {
                    command: Some(DispatchCommand::Ack { result, reason, .. }),
                    ..
                },
            ..
        }) = Cli::try_parse_from(line.split_whitespace())
        else {
            panic!("not ack");
        };
        assert_eq!(
            (result, reason.join(" ")),
            (
                AckResult::Raw,
                r#"{"sessionId":"local_a","isRunning":true}"#.to_owned()
            )
        );
    }

    #[test]
    fn dispatch_waits_the_default_when_no_wait_is_given() {
        let command = Cli::try_parse_from(["ignite", "dispatch"]).unwrap().command;
        assert!(matches!(
            command,
            Command::Dispatch {
                wait: ignite_core::protocol::client::DISPATCH_WAIT_S,
                command: None,
            }
        ));
    }

    /// `ignite pool usage` reaches the daemon's `pool` verb, which prints the plan's usage.
    #[test]
    fn parses_pool_usage() {
        assert!(matches!(
            Cli::try_parse_from(["ignite", "pool", "usage"])
                .unwrap()
                .command,
            Command::Pool {
                command: PoolCommand::Usage
            }
        ));
    }

    #[test]
    fn parses_pool_mode() {
        assert!(matches!(
            Cli::try_parse_from(["ignite", "pool", "mode", "claude", "config"])
                .unwrap()
                .command,
            Command::Pool {
                command: PoolCommand::Mode { app, setting }
            } if app == "claude" && setting == "config"
        ));
    }

    #[test]
    fn parses_the_documented_commands() {
        let parse = |line: &str| {
            Cli::try_parse_from(line.split_whitespace())
                .unwrap()
                .command
        };
        assert!(
            matches!(parse("ignite stop L-2"), Command::Stop { handle, force: false } if handle == "L-2")
        );
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
            parse("ignite dispatch --wait 480"),
            Command::Dispatch { wait: 480, .. }
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
            Command::Tell { all: true, now: false, words } if words == ["Skip"]
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
    fn parses_a_forced_stop() {
        let line = ["ignite", "stop", "T-3", "--force"];
        assert!(matches!(
            Cli::try_parse_from(line).unwrap().command,
            Command::Stop { handle, force: true } if handle == "T-3"
        ));
    }

    #[test]
    fn parses_the_cli_turn_command_with_the_program_untouched() {
        let line = [
            "ignite", "internal", "cli-turn", "--home", "/h", "--", "claude", "-p", "--model", "m",
        ];
        assert!(matches!(
            Cli::try_parse_from(line).unwrap().command,
            Command::Internal {
                command: InternalCommand::CliTurn { home, command }
            } if home == std::path::Path::new("/h") && command == ["claude", "-p", "--model", "m"]
        ));
    }

    #[test]
    fn parses_the_verb_process_command() {
        let turn = Cli::try_parse_from([
            "ignite",
            "internal",
            "justiciar-turn",
            "/h/j.lock",
            "/h/j-1.log",
            "claude",
            "-p",
            "--model",
            "opus",
        ])
        .unwrap();
        let Command::Internal {
            command: InternalCommand::JusticiarTurn { lock, log, command },
        } = turn.command
        else {
            panic!("not a justiciar turn");
        };
        assert_eq!(
            (lock.to_str(), log.to_str()),
            (Some("/h/j.lock"), Some("/h/j-1.log"))
        );
        assert_eq!(command, ["claude", "-p", "--model", "opus"]);
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

    #[test]
    fn tell_takes_now() {
        let line = "ignite tell --now T.1 Stop";
        assert!(matches!(
            Cli::try_parse_from(line.split_whitespace()).unwrap().command,
            Command::Tell { all: false, now: true, words } if words == ["T.1", "Stop"]
        ));
    }
}
