//! Keeping each held ticket's silence clock across daemon restarts. Without it, a session that
//! has been quiet for 25 minutes looks freshly active to a restarted daemon, so with a restart
//! every half hour the stall procedure (see `watch`) would never reach its 30 minutes.
//!
//! The clock is stored per tree in the local database's `claim_activity` table: when the
//! session's last command ended (`active_at`) and how far the stall procedure has gone. A `check`
//! that was queued is not known to the dispatcher after a restart, so it is restored as a failed
//! lookup, which is asked again at the next tick.

use ignite_core::ids::TreeId;
use ignite_core::time::Timestamp;
use ignite_store::local::{activity, rows};

use super::{Claims, Held, Stall, lock};

/// The row that records `held`'s clock.
fn row_of(held: &Held) -> rows::ClaimActivity {
    let (stage, checked_at, messaged_at, second) = match held.stall {
        Stall::Active => ("active", None, None, false),
        Stall::Asked { at, second, .. } => ("asked", Some(at), None, second),
        Stall::Messaged { at } => ("messaged", None, Some(at), false),
        Stall::Retry { at, second } => ("retry", Some(at), None, second),
    };
    rows::ClaimActivity {
        tree: held.tree.to_string(),
        ticket: held.ticket.to_string(),
        active_at: held.active_at.to_string(),
        stage: stage.to_owned(),
        checked_at: checked_at.map(|at| at.to_string()),
        messaged_at: messaged_at.map(|at| at.to_string()),
        second,
    }
}

/// Sets `held`'s clock from `row`. A row that cannot be read leaves the clock as it is.
pub(super) fn restore(held: &mut Held, row: &rows::ClaimActivity) {
    let parse = |text: &Option<String>| text.as_deref().and_then(|t| Timestamp::parse(t).ok());
    let Ok(active_at) = Timestamp::parse(&row.active_at) else {
        return;
    };
    held.active_at = active_at;
    held.stall = match (
        row.stage.as_str(),
        parse(&row.checked_at),
        parse(&row.messaged_at),
    ) {
        ("asked" | "retry", Some(at), _) => Stall::Retry {
            at,
            second: row.second,
        },
        ("messaged", _, Some(at)) => Stall::Messaged { at },
        _ => Stall::Active,
    };
}

/// The recorded clocks of `trees`, for [`Claims::new`] to restore.
pub(super) fn recorded(conn: &rusqlite::Connection) -> Vec<rows::ClaimActivity> {
    activity::all(conn).unwrap_or_default()
}

impl Claims {
    /// Writes the clock of each of `trees` that differs from what was last written.
    pub(super) fn save_activity<'a>(&self, trees: impl IntoIterator<Item = &'a TreeId>) {
        let changed: Vec<rows::ClaimActivity> = {
            let mut state = lock(&self.state);
            let mut changed = Vec::new();
            for tree in trees {
                let Some(held) = state.held.get(tree) else {
                    continue;
                };
                let row = row_of(held);
                if state.saved.get(tree) != Some(&row) {
                    state.saved.insert(tree.clone(), row.clone());
                    changed.push(row);
                }
            }
            changed
        };
        let db = lock(&self.db);
        for row in &changed {
            // A failed write only means a restart forgets a little of the clock.
            let _ = activity::set(&db, row);
        }
    }

    /// Forgets the clock of a tree whose ticket is no longer held.
    pub(super) fn forget_activity(&self, tree: &TreeId) {
        lock(&self.state).saved.remove(tree);
        let _ = activity::clear(&lock(&self.db), tree.as_str());
    }
}
