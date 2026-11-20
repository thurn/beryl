//! One row struct per table of the hub's database, with the columns as they are stored: text
//! for IDs, times (RFC 3339) and JSON, integers and reals for numbers. Converting to the
//! protocol's types is the job of each table's module.

crate::tables! {
    Ticket = "tickets" {
        id: String,
        parent: Option<String>,
        next_child: i64,
        title: String,
        body: String,
        acceptance: String,
        design: String,
        notes: String,
        r#type: String,
        status: String,
        priority: i64,
        assignee: Option<String>,
        last: i64,
        /// JSON array of strings.
        labels: String,
        /// JSON object.
        metadata: String,
        defer_until: Option<String>,
        close_reason: Option<String>,
        created_at: String,
        created_by: String,
        updated_at: String,
        closed_at: Option<String>,
        version: i64,
        workers: Option<i64>,
        max_workers: Option<i64>,
        cores: Option<f64>,
        machine: Option<String>,
        session: Option<String>,
        tree: Option<String>,
        claimed_at: Option<String>,
        landed_at: Option<String>,
        /// JSON: commits, files, note.
        landed: Option<String>,
        /// JSON: rule, subject, unit, amount.
        saving: Option<String>,
        /// `<machine>` or `<machine>/<app>`.
        owner: Option<String>,
        needs_os: Option<String>,
        fallback: bool,
        every_hours: Option<i64>,
    }

    Edge = "edges" { src: String, dst: String, kind: String }

    Comment = "comments" {
        ticket: String,
        n: i64,
        author: String,
        at: String,
        text: String,
        legacy_id: Option<String>,
    }

    History = "history" { ticket: String, seq: i64, actor: String, at: String, change: String }

    Lease = "leases" {
        name: String,
        holder: String,
        machine: String,
        expires_at: String,
        payload: String,
        waiters: String,
    }

    Alias = "aliases" { alias: String, id: String }

    Machine = "machines" {
        id: String,
        name: String,
        os: String,
        cores: i64,
        memory_mb: i64,
        families: String,
        version: String,
        last_seen: String,
    }

    Worker = "workers" {
        machine: String,
        session: String,
        app: String,
        role: String,
        state: String,
        ticket: Option<String>,
        files: String,
        command: Option<String>,
        command_since: Option<String>,
        command_running: bool,
        state_since: Option<String>,
    }

    Reservation = "reservations" {
        ticket: String,
        machine: String,
        session: String,
        expires_at: String,
        code: String,
    }

    Handoff = "handoffs" { seq: i64, machine: String, ticket: String, kind: String, at: String }

    /// A machine's ID, or `*` for every machine.
    Pause = "pauses" { machine: String, at: String }

    CachedResult = "results" {
        check_name: String,
        hash: String,
        /// Empty for a portable check.
        platform: String,
        outcome: String,
        machine: String,
        record: i64,
        at: String,
        last_lookup: String,
        output: Option<String>,
    }

    Check = "checks" {
        name: String,
        family: String,
        kind: String,
        state: String,
        check_group: String,
    }

    CheckStat = "check_stats" { check_name: String, area: String, runs: i64, failures: i64 }

    CheckCost = "check_costs" {
        check_name: String,
        /// JSON: the last 20 costs.
        costs: String,
        median: Option<f64>,
        unit: String,
    }

    Flaky = "flaky" {
        check_name: String,
        hash: String,
        commit_sha: String,
        machine: String,
        at: String,
        always: i64,
        command: String,
        output: String,
        /// The ticket filed for this record; a filed record no longer counts.
        filed: Option<String>,
    }

    Regression = "regressions" {
        check_name: String,
        ticket: String,
        first_bad: String,
        last_good: String,
        state: String,
    }

    Tip = "tips" { commit_sha: String, seen: String, verified: Option<String>, state: String }

    TipClaim = "tip_claims" {
        commit_sha: String,
        family: String,
        batch: i64,
        machine: String,
        expires_at: String,
    }

    Record = "records" {
        id: i64,
        machine: String,
        ticket: Option<String>,
        session: Option<String>,
        tree: Option<String>,
        verb: String,
        class: String,
        family: Option<String>,
        queued_s: f64,
        wall_s: f64,
        cpu_s: f64,
        instructions: Option<f64>,
        peak_mb: i64,
        outcome: Option<String>,
        commit_sha: Option<String>,
        ignite: String,
        at: String,
    }

    CompileUnit = "compile_units" {
        record: i64,
        crate_name: String,
        mode: String,
        instructions: Option<f64>,
        cpu_s: f64,
        rebuilt: i64,
    }

    Finding = "findings" {
        rule: String,
        subject: String,
        unit: String,
        cost: f64,
        saving: f64,
        ticket: Option<String>,
        closed_at: Option<String>,
    }

    Explore = "explore" { family: String, day: String, bugs: i64, instructions: f64 }

    Rebuild = "rebuilds" {
        name: String,
        commit_sha: String,
        instructions: f64,
        /// JSON: instructions by crate.
        crates: String,
    }

    RejectedCommand = "rejected_commands" {
        id: i64,
        machine: String,
        session: Option<String>,
        ticket: Option<String>,
        verb: String,
        /// JSON array of strings.
        args: String,
        kind: String,
        suggestion: Option<String>,
        at: String,
    }

    /// A check switched on or off by hand.
    CheckStateChange = "check_state_changes" {
        id: i64,
        check_name: String,
        state: String,
        previous: String,
        actor: String,
        machine: String,
        note: String,
        ticket: Option<String>,
        at: String,
    }

    /// A check family's listing, by family and listing hash, as one machine made it.
    Listing = "listings" {
        family: String,
        hash: String,
        listing: String,
        machine: String,
        at: String,
    }

    /// A verdict, by tree, base and gate settings, as the machine that passed the gate sent it.
    Verdict = "verdicts" {
        tree: String,
        base: String,
        settings: String,
        verdict: String,
        passed_at: String,
    }

    /// The check costs a tip round cuts one family's batches by, frozen at the first request.
    TipCostRow = "tip_costs" {
        commit_sha: String,
        family: String,
        claim_seconds: f64,
        /// JSON object of check name to CPU seconds.
        costs: String,
        at: String,
    }

    /// One finished tip verification round and where its time went, in seconds.
    TipRoundRow = "tip_rounds" {
        id: i64,
        commit_sha: String,
        machine: String,
        at: String,
        wall_s: f64,
        ran: i64,
        cached: i64,
        checks_s: f64,
        listing_s: f64,
        building_s: f64,
        waiting_s: f64,
        /// JSON `RoundBudget`, NULL from a daemon built before the tip budget.
        budget: Option<String>,
        /// Listing with nothing else running, NULL from a daemon built before it was measured.
        listing_alone_s: Option<f64>,
    }

    /// A check a landing found failing at a tip, which tip verification had missed.
    TipMiss = "tip_misses" {
        check_name: String,
        family: String,
        tip: String,
        /// `stale-pass`, `skipped` or `not-run`.
        kind: String,
        reason: String,
        ticket: String,
        at: String,
    }

    /// Each class's rate of check commands for one day, in billions of instructions per second.
    ClassRate = "class_rates" { day: String, class: String, rate: Option<f64> }

    /// `kind` is the export table the row came from, NULL for a header or a findings report.
    Metric = "metrics" { machine: String, source: String, data: String, at: String, kind: Option<String> }

    /// One ticket window's time and dollar segments, as a machine posted them.
    TicketSpend = "ticket_spend" { machine: String, window: String, ticket: String, start: String, end: String, data: String, at: String }

    /// The latest title a Claude desktop session reported.
    SessionTitle = "session_titles" { session: String, title: String, updated_at: String }

    /// One change in the feed's journal, which keeps only the most recent positions.
    FeedChange = "feed" { id: i64, seq: i64, kind: String, ticket: Option<String>, name: Option<String> }
}
