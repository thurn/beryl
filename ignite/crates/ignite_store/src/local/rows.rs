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
    }

    /// The newest change to a leased tree's ticket that its session has been shown.
    Shown = "shown" {
        tree: String,
        ticket: String,
        comment: i64,
        history: i64,
    }

    PoolSetting = "pool_settings" { key: String, value: String }
}
