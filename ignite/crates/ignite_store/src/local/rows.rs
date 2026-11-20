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
}
