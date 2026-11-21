//! One row struct per table of a daemon's local database, with the columns as they are stored.

crate::tables! {
    Tree = "trees" {
        id: String,
        path: String,
        kind: String,
        ticket: Option<String>,
        session: Option<String>,
        branch: Option<String>,
        built_commit: Option<String>,
        leased_at: Option<String>,
        last_used: Option<String>,
        /// `free`, `prebuilding`, `leased`, `landing` or `resetting`.
        state: String,
        /// The handle of the verb that leased a landing tree, such as `L-6`; `None` for a
        /// holder without a handle and for every other state.
        owner: Option<String>,
    }

    Grant = "grants" {
        id: i64,
        handle: Option<String>,
        class: String,
        cores: i64,
        /// JSON array of resource names.
        resources: String,
        tree: Option<String>,
        ticket: Option<String>,
        pid: Option<i64>,
        granted_at: String,
    }

    Handle = "handles" {
        id: String,
        verb: String,
        session: Option<String>,
        tree: Option<String>,
        state: String,
        exit: Option<i64>,
        /// JSON: the reply, once done.
        reply: Option<String>,
        started_at: String,
        finished_at: Option<String>,
        /// The process ID of the verb process running the verb; `None` for a verb the daemon
        /// runs itself.
        pid: Option<i64>,
        /// The Ignite commit of the verb process's executable.
        executable: Option<String>,
        /// The host protocol version the verb process speaks.
        protocol: Option<i64>,
    }

    OutboxEntry = "outbox" { seq: i64, kind: String, body: String, at: String }

    CachedResult = "results" {
        check_name: String,
        hash: String,
        platform: String,
        outcome: String,
        record: Option<i64>,
        at: String,
        output: Option<String>,
        /// The machine that produced it; `None` for a row stored before machines were kept.
        machine: Option<String>,
    }

    /// A check family's listing, by family and listing hash.
    Listing = "listings" { family: String, hash: String, listing: String, at: String }

    Pass = "passes" {
        family: String,
        /// `list` or `run`.
        command: String,
        wall_s: f64,
        at: String,
    }

    PoolMember = "pool" {
        session: String,
        app: String,
        role: String,
        state: String,
        ticket: Option<String>,
        /// JSON: model tiers its app can run.
        models: String,
        added_at: String,
        /// The dispatcher session that added it.
        parent: Option<String>,
        /// JSON: the files its last ticket touched.
        files: String,
        /// A Codex dispatcher's worker slots; 0 for every other member.
        slots: i64,
        /// 1 for a Codex chat opened for one ticket, which leaves the pool when it ends.
        single_use: i64,
    }

    /// The newest change to a leased tree's ticket that its session has been shown.
    Shown = "shown" {
        tree: String,
        ticket: String,
        comment: i64,
        history: i64,
    }

    /// When a held tree's session last ran a command, and its place in the stall procedure.
    ClaimActivity = "claim_activity" {
        tree: String,
        ticket: String,
        active_at: String,
        /// `active`, `asked`, `messaged` or `retry`.
        stage: String,
        checked_at: Option<String>,
        messaged_at: Option<String>,
        /// Whether the check is the one after the message.
        second: bool,
    }

    /// One entry of the dispatcher's queue, as JSON.
    DispatchEntry = "dispatch_queue" {
        /// `pending`, `given`, `reserved`, `orphan`, `refused`, `stall` or `meta`.
        kind: String,
        key: String,
        body: String,
    }

    PoolSetting = "pool_settings" { key: String, value: String }

    /// One window of one sample of the Claude plan's usage, as the app's `get_usage` reported
    /// it through the dispatcher.
    PlanUsage = "plan_usage" {
        id: i64,
        /// When the sample was taken.
        at: String,
        machine: String,
        /// `get_usage`'s `plan.status`: `ok`, `not_applicable`, `unavailable`, or `unreadable`
        /// when the dispatcher's report was not `get_usage`'s JSON.
        status: String,
        /// The window's label, such as `Weekly · all models`; `None` for a sample with no
        /// windows.
        label: Option<String>,
        percent_used: Option<f64>,
        resets_at: Option<String>,
        /// Why a sample has no windows, when the status is not `ok`.
        note: Option<String>,
        /// 1 once the row reached the hub.
        posted: i64,
        provider: String,
    }

    /// What the build manager knows about one build product in one tree.
    Build = "builds" {
        tree: String,
        product: String,
        /// JSON: the package names, sorted; empty for the whole workspace.
        packages: String,
        /// The input hash the product was last built or last failed at.
        hash: String,
        /// `held` or `failed`.
        outcome: String,
        /// The failed build's output file.
        output: Option<String>,
        updated_at: String,
    }

    /// The tip verifier's memory between rounds: one row, `id` always 1.
    TipMemory = "tip_state" {
        id: i64,
        /// JSON; see `ignite_daemon`'s `tip::state`.
        body: String,
        updated_at: String,
    }

    /// The stop state of a verb that runs in a process of its own.
    HandleStop = "handle_stops" {
        handle: String,
        /// A stop was asked.
        asked: bool,
        /// Why stopping is refused, while it is.
        refused: Option<String>,
        at: String,
    }

    /// An express run that passed, by the tree it checked and the `[express]` section it ran.
    ExpressPass = "express_passes" {
        tree_id: String,
        /// A digest of the `[express]` section.
        config: String,
        at: String,
        /// The working tree the commands ran in.
        tree_path: Option<String>,
        /// The commit that tree had checked out.
        head: Option<String>,
        /// The change's own files on that commit, one per line.
        changed: Option<String>,
    }

    /// The checks a change's last passing gate chose by score.
    GateChoice = "gate_choices" {
        /// The change's own files, sorted, one per line.
        change: String,
        /// The checks, one per line.
        checks: String,
        /// Milliseconds since the Unix epoch.
        at: i64,
    }

    /// The rate the landing gates' check commands achieved lately.
    GateRate = "gate_rate" {
        id: i64,
        /// Milliseconds since the Unix epoch.
        at: i64,
        /// Billions of instructions per second of running checks.
        rate: f64,
    }

    /// A verdict this daemon stored, by tree, base and gate settings.
    Verdict = "verdicts" {
        tree: String,
        base: String,
        settings: String,
        /// The verdict as JSON.
        verdict: String,
        passed_at: String,
    }

    /// One CLI worker slot of one app (see `schema_20.sql`).
    CliSlot = "cli_slots" {
        app: String,
        /// The slot's number, from 1.
        slot: i64,
        /// The Ignite session, `cli-<app>-<8 hex>`; `None` while free.
        session: Option<String>,
        ticket: Option<String>,
        /// `starting`, `running`, `waiting_verb`, `backing_off`, `parked` or `free`.
        state: String,
        /// The latest turn's number; 0 before the first.
        turn: i64,
        /// The CLI's own transcript ID, from the latest turn's first line.
        transcript: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        /// When the slot entered its state.
        since: Option<String>,
        /// The hub's reservation code the slot was handed.
        reservation: Option<String>,
        /// The prompt of the next turn, while the slot waits to start it.
        next_prompt: Option<String>,
        /// The `ignite` verb's handle a `waiting_verb` slot waits on.
        wait_handle: Option<String>,
        /// When a `backing_off` slot starts its next turn.
        resume_at: Option<String>,
        /// Why a `parked` slot is parked.
        park_reason: Option<String>,
        /// Interrupted ends in a row with no `ignite` command between them.
        interrupted_in_row: i64,
        /// 1 when the last clean end was answered with the turn-ended block text.
        block_sent: i64,
        /// 1 when the last end was an API error answered with a back-off.
        backed_off: i64,
        /// While the slot's first turn resumes another session's transcript, the prompt that
        /// starts the ticket again on a new transcript if that turn does not end cleanly.
        fallback_prompt: Option<String>,
        handoff_note: Option<String>,
    }

    /// One turn of a CLI session.
    CliTurn = "cli_turns" {
        session: String,
        turn: i64,
        app: String,
        slot: i64,
        ticket: Option<String>,
        prompt: String,
        /// `starting`, `running` or `ended`.
        state: String,
        /// The supervised run's directory, once found.
        run: Option<String>,
        started_at: Option<String>,
        ended_at: Option<String>,
        exit: Option<i64>,
        /// JSON: the turn's end, as `ignite_core::cli::TurnEnd`.
        end_kind: Option<String>,
        transcript: Option<String>,
        model: Option<String>,
        cli_version: Option<String>,
        cost_usd: Option<f64>,
        /// Lines of `output.log` whose effects were applied.
        lines_applied: i64,
    }

    /// A message waiting for, or delivered in, a CLI session's turn.
    CliMessage = "cli_messages" {
        id: i64,
        session: String,
        ticket: Option<String>,
        text: String,
        /// 1 for `tell --now` and orders from a justiciar, which end the running turn.
        urgent: i64,
        queued_at: Option<String>,
        /// The turn whose prompt carried it; `None` while it waits.
        delivered_turn: Option<i64>,
    }

    /// An app's CLI hold.
    CliHold = "cli_holds" {
        app: String,
        /// `ignite_core::cli::HoldReason`'s stored name.
        reason: String,
        since: Option<String>,
        cli_version: Option<String>,
        /// The first line of the error.
        message: Option<String>,
        probed_at: Option<String>,
        /// 1 once the hold's notification and justiciar report were sent.
        reported: i64,
    }

    /// The latest outcome of one check for one session (see `results::note_sighting`).
    CheckSighting = "check_sightings" {
        session: String,
        check_name: String,
        /// `test`, `check` or `land`.
        verb: String,
        /// `pass` or `fail`.
        outcome: String,
        hash: Option<String>,
        /// The first line of a failure's message; `None` for a cached failure.
        first_line: Option<String>,
        /// 1 when a landing was refused because the check's result at the tip was unknown.
        tip_unknown: i64,
        at: String,
    }

    /// A ticket that landed from a CLI slot, with the transcript its last turn named, which a
    /// regression it caused resumes for 24 hours.
    CliLanded = "cli_landed" {
        ticket: String,
        app: String,
        transcript: Option<String>,
        model: Option<String>,
        landed_at: Option<String>,
    }
}
