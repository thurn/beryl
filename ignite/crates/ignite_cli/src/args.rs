//! Every verb and flag of the `ignite` command.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::fleet::FleetCommand;
use crate::values::{parse_every, parse_priority, parse_saving};

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
        /// Land even though the ticket holds regression checks the hub has switched off, which
        /// no gate runs. Only for justiciars: a worker moves such a check to its follow-up
        /// ticket with `ignite ticket regression move`.
        #[arg(long)]
        allow_off_checks: bool,
    },
    /// Give the ticket back unfinished: save the tree's work on the remote and release
    /// the tree.
    Release {
        /// The ticket's exact state, for whoever starts it next.
        #[arg(short = 'm', long = "message")]
        note: Option<String>,
        /// What the ticket needs: `person` (which needs a note) sets it aside for a person, an
        /// operating system such as `windows` records the required system. A machine name sets
        /// the `machine:<name>` label, replacing any machine pin on the ticket.
        #[arg(long)]
        needs: Option<String>,
    },
    /// Close the ticket without landing anything, and release the tree.
    Close {
        /// Why it needs no landing: obsolete, already done, or no change to the repository.
        #[arg(short = 'm', long = "message")]
        reason: String,
        /// Close even though the ticket holds regression checks the hub has switched off. Only
        /// for justiciars, as for `ignite land`.
        #[arg(long)]
        allow_off_checks: bool,
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
        /// it does not stop when asked. Only for incident commanders, justiciars and the operator.
        #[arg(long)]
        force: bool,
    },
    /// Create, show, list, link and comment on tickets.
    Ticket {
        #[command(subcommand)]
        command: Box<TicketCommand>,
    },
    /// Show one or more tickets, like `ticket show`. For a check's record use `why`.
    Show {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long)]
        comments: bool,
        #[arg(long)]
        history: bool,
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
    /// View or edit the fleet's machine and provider settings.
    Fleet {
        #[command(subcommand)]
        command: Option<FleetCommand>,
    },
    /// Manage where this machine keeps its build trees.
    Trees {
        #[command(subcommand)]
        command: TreesCommand,
    },
    /// The machine's pool of worker sessions.
    #[command(disable_help_subcommand = true)]
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
    /// Pause new landings or all new heavy work across the fleet during an emergency.
    Emergency {
        #[command(subcommand)]
        command: Option<crate::emergency::EmergencyCommand>,
    },
    /// For the operator's `/rush` skill only: put this session's work ahead of every class but
    /// tip builds and emergencies until its ticket lands or is released.
    #[command(hide = true)]
    Rush {
        #[command(subcommand)]
        command: Option<RushCommand>,
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
    /// Probe one trees root for `ignite monitor`: hold `lock` while reading `root`'s metadata.
    ProbeStorage {
        lock: std::path::PathBuf,
        root: std::path::PathBuf,
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

#[derive(Debug, Subcommand)]
pub enum ManifestCommand {
    /// Print every input pattern that matches no tracked file and every `crate:` that names
    /// no workspace package; exit 1 if there is any.
    Lint,
}

#[derive(Debug, Subcommand)]
pub enum PoolCommand {
    /// List the pool's sessions and what each holds.
    List,
    /// Stop handing off new tickets; running workers finish.
    Pause {
        /// On every machine.
        #[arg(long)]
        all: bool,
        /// With `--all`: still hand off the tickets that hold master back (regressions and
        /// justiciar tickets), one at a time per machine.
        #[arg(long, requires = "all")]
        except_holding_master: bool,
    },
    /// Start handing off tickets again. An emergency in force goes on; `ignite emergency end`
    /// ends it.
    Resume {
        /// On every machine.
        #[arg(long)]
        all: bool,
    },
    /// Print the newest sample of each Claude plan window, and the usage hold if one is in
    /// force.
    Usage,
    /// Take back the newest build tickets of a machine that holds more than its limit, without
    /// the workers' help: their work is saved and the tickets are open again. For the operator,
    /// incident commanders and justiciars.
    Shed {
        /// The machine to shed on, by name; this machine when left out.
        #[arg(long)]
        machine: Option<String>,
        /// The number of build tickets to keep; the machine's limit when left out.
        #[arg(long)]
        to: Option<u32>,
    },
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
    /// Report the epic's graph and fail on blocking dependencies outside the plan.
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
    /// An epic's worker share or a ticket's queue weight. Zero parks work.
    #[arg(long, alias = "workers")]
    pub allocation: Option<u32>,
    /// A hard cap on an epic's workers.
    #[arg(long)]
    pub max_workers: Option<u32>,
    /// The cores the ticket is expected to use.
    #[arg(long)]
    pub cores: Option<f64>,
    /// The operating system the work requires. Use `any` to clear the requirement.
    #[arg(long, value_parser = ["macos", "windows", "any"])]
    pub needs_os: Option<String>,
    /// Estimated agent minutes for this ticket's own work. Use 0 to clear it.
    #[arg(long)]
    pub estimate_minutes: Option<u32>,
    /// Estimated agent minutes for work that has yet to be filed. Use 0 to clear it.
    #[arg(long)]
    pub unfiled_minutes: Option<u32>,
    /// Make it a meta-ticket, a template the hub copies into a new child ticket on this
    /// schedule: hours (`6` or `6h`) or days (`2d`). A copy is filed when the meta-ticket is
    /// created and then at each due time, never while an earlier copy is still open. On create
    /// it implies `--type meta`, whose ID starts with `M`.
    #[arg(long, value_parser = parse_every)]
    pub every: Option<u32>,
    /// On create, your estimate of what the ticket would save a week, as `<unit>:<amount>` with
    /// a unit of `dollars`, `agent-hours` or `core-hours`, such as `agent-hours:12`. It makes
    /// the ticket an improvement, which takes the improvement share of hand-offs ahead of the
    /// epics' order, so the body must show the measurement the estimate comes from.
    #[arg(long, value_parser = parse_saving)]
    pub saving: Option<(f64, String)>,
}

#[derive(Debug, Subcommand)]
pub enum TicketCommand {
    /// File a ticket.
    Create {
        title: String,
        #[command(flatten)]
        fields: TicketFieldArgs,
        /// Comma-separated labels. Use `machine:<name>` and `provider:<name>` to pin work.
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
        /// File from a program such as a check or script. Adds a `machine:<name>` label for
        /// the machine running this command if the ticket has no machine pin.
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
    /// Change which ticket holds a failing check's regression.
    Regression {
        #[command(subcommand)]
        command: RegressionCommand,
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
        /// Allow `on` to lie outside `id`'s outermost epic. Use only when the user has approved
        /// this edge: the epic's workers do not reach `on`, so the epic can stall on it.
        #[arg(long)]
        external: bool,
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

#[derive(Debug, Subcommand)]
pub enum RegressionCommand {
    /// Move `check` off the regression ticket the hub grouped it on and onto `ticket`, so that
    /// its later failures reopen `ticket` and not the old one.
    Move { check: String, ticket: String },
}

#[derive(Debug, Subcommand)]
pub enum TreesCommand {
    /// Delete one tree's build output to free its build slot, for incident commanders,
    /// justiciars and the operator. The lease stays with its holder, whose ticket is told.
    Evict {
        /// The tree, such as `tree-05`.
        tree: String,
        /// Stop the commands running in the tree first, instead of refusing.
        #[arg(long)]
        force: bool,
    },
    /// Provision an APFS volume with matching reserve and quota.
    Volume {
        /// Use the volume for Ignite's databases and services instead of build trees.
        #[arg(long)]
        state: bool,
        /// Reserve and quota in decimal gigabytes.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        size_gb: u32,
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
            // Answered by the same code as `ticket show`.
            Command::Ticket { .. } | Command::Show { .. } => "ticket",
            Command::Why { .. } => "why",
            Command::Status { .. } => "status",
            Command::Doctor => "doctor",
            Command::Prime => "prime",
            Command::Guide { .. } => "guide",
            Command::Install => "install",
            Command::Fleet { .. } => "fleet",
            Command::Trees { .. } => "trees",
            Command::Pool { .. } => "pool",
            Command::Tell { .. } => "tell",
            Command::Justiciar { .. } => "justiciar",
            Command::Emergency { .. } => "emergency",
            Command::Rush { .. } => "rush",
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

/// The subcommands of `ignite rush`, which the daemon's `rush` verb answers (see
/// `ignite_daemon::rush`).
#[derive(Debug, Subcommand)]
pub enum RushCommand {
    /// End this session's rush before its ticket lands.
    End,
}

#[cfg(test)]
mod tests;
