//! Ending a held ticket: `ignite release`, `ignite close`, the stall procedure's release, and
//! freeing the tree afterwards.
//!
//! A ticket that was closed at the hub by hand (`ignite ticket close`) is no longer held there,
//! so the hub refuses its release and its close. This module frees the tree anyway when the
//! hub says the ticket is not held and the ticket is closed, and [`Claims::tick`] does the same
//! for a session that never tries.

use ignite_core::ids::{SessionId, TicketId, TreeId};
use ignite_core::protocol::client::{Reply, Request};
use ignite_core::protocol::hub::ReleaseCause;
use ignite_core::protocol::tickets::Status;
use ignite_core::traits::{Error, Result};
use ignite_store::local::{shown, trees as tree_rows};

use super::parse::{failure, parse_close, parse_release};
use super::{Claims, Held, ReleaseRequest, Saved, brief, lock};
use crate::server::Context;

/// What a release gave back.
pub(super) struct Given {
    pub saved: Saved,
    /// The reset's failure, if any.
    pub reset: Option<Error>,
    /// The ticket had already been closed at the hub, so the hub took no release.
    pub already_closed: bool,
}

impl Claims {
    pub(super) fn release(&self, ctx: &Context, req: &Request) -> Reply {
        let (note, needs) = match parse_release(req) {
            Ok(parsed) => parsed,
            Err(reply) => return reply,
        };
        let held = match self.held_for(req) {
            Ok(held) => held,
            Err(reply) => return reply,
        };
        let title = brief::title_of(held.brief.as_ref(), &held.ticket);
        match self.give_back(ctx, &held, |_| note, needs, req.pid, ReleaseCause::GaveUp) {
            Ok(Given {
                already_closed: true,
                reset,
                ..
            }) => brief::already_closed(&held.ticket, &held.tree, &title, reset),
            Ok(Given { saved, reset, .. }) => {
                brief::released(&held.ticket, &held.tree, &saved, &title, reset)
            }
            Err(error) => failure(&error, &format!("{} is still yours", held.ticket)),
        }
    }

    /// Saves the tree's work, releases the ticket at the hub with the note made from what was
    /// saved, and frees the tree. If the save or the release fails, the session keeps the ticket
    /// and the tree. The exception is a ticket that was closed at the hub by hand: the hub then
    /// refuses the release because it no longer holds the ticket, and the tree is freed anyway,
    /// after the save.
    pub(super) fn give_back(
        &self,
        ctx: &Context,
        held: &Held,
        note: impl FnOnce(&Saved) -> String,
        needs: Option<String>,
        caller: Option<u32>,
        cause: ReleaseCause,
    ) -> Result<Given> {
        let saved = self.trees.save(&held.tree, &held.ticket)?;
        let released = self.desk.release(&ReleaseRequest {
            ticket: held.ticket.clone(),
            machine: ctx.machine.clone(),
            session: held.session.clone(),
            note: note(&saved),
            needs,
            saved: saved.clone(),
            cause,
        });
        let mut already_closed = false;
        if let Err(error) = released {
            if !self.closed_by_hand(&held.ticket, &error) {
                return Err(error);
            }
            already_closed = true;
        }
        Ok(Given {
            saved,
            reset: self.free(held, caller),
            already_closed,
        })
    }

    pub(super) fn close(&self, ctx: &Context, req: &Request) -> Reply {
        let reason = match parse_close(req) {
            Ok(reason) => reason,
            Err(reply) => return reply,
        };
        let held = match self.held_for(req) {
            Ok(held) => held,
            Err(reply) => return reply,
        };
        let title = brief::title_of(held.brief.as_ref(), &held.ticket);
        let closed = self
            .desk
            .close(&held.ticket, &ctx.machine, &held.session, &reason);
        let already_closed = match closed {
            Ok(()) => false,
            Err(error) if self.closed_by_hand(&held.ticket, &error) => true,
            Err(error) => return failure(&error, &format!("{} is still open", held.ticket)),
        };
        let dropped = self.trees.drop_saved(&held.ticket).err();
        let reset = self.free(&held, req.pid);
        if already_closed {
            return brief::already_closed(&held.ticket, &held.tree, &title, reset);
        }
        brief::closed(&held.ticket, &held.tree, &reason, &title, dropped, reset)
    }

    /// Whether `error`, the hub's refusal of a release or close, only means that the ticket is
    /// not held any more because it was closed at the hub by hand (`ignite ticket close`).
    fn closed_by_hand(&self, ticket: &TicketId, error: &Error) -> bool {
        matches!(error, Error::Conflict(_)) && self.closed_at_hub(ticket)
    }

    /// Whether the hub says the ticket is closed. A hub that cannot be read says nothing.
    fn closed_at_hub(&self, ticket: &TicketId) -> bool {
        self.desk
            .detail(ticket)
            .is_ok_and(|detail| detail.ticket.status == Status::Closed)
    }

    /// Frees the trees of held tickets that were closed at the hub by hand, as
    /// `ignite ticket close` does without telling the daemon. The tree's work is saved first,
    /// like a release; if the save fails the ticket stays held and the next tick tries again.
    pub(super) fn free_closed_tickets(&self) {
        let held: Vec<Held> = lock(&self.state).held.values().cloned().collect();
        for held in held {
            if !self.closed_at_hub(&held.ticket) {
                continue;
            }
            if let Err(error) = self.trees.save(&held.tree, &held.ticket) {
                eprintln!(
                    "ignite: {} is closed but {} not saved: {error}",
                    held.ticket, held.tree
                );
                continue;
            }
            if let Some(error) = self.free(&held, None) {
                eprintln!(
                    "ignite: {} is closed but {} not reset: {error}",
                    held.ticket, held.tree
                );
            }
        }
    }

    /// Forgets a held ticket and resets its tree; returns the reset's failure, if any.
    pub(super) fn free(&self, held: &Held, caller: Option<u32>) -> Option<Error> {
        {
            let mut state = lock(&self.state);
            state.held.remove(&held.tree);
            state.dirty.remove(&held.ticket);
        }
        let _ = shown::clear(&lock(&self.db), held.tree.as_str());
        self.forget_activity(&held.tree);
        self.reset_tree(&held.tree, caller).err()
    }

    /// Resets and frees a tree. A failure is remembered so that [`Claims::tick`] retries it:
    /// the tree is then leased to a ticket that nothing on this machine holds any more.
    pub(super) fn reset_tree(&self, tree: &TreeId, caller: Option<u32>) -> Result<()> {
        let result = self.trees.reset(tree, caller);
        let mut state = lock(&self.state);
        if result.is_err() {
            state.reset_failed.insert(tree.clone());
        } else {
            state.reset_failed.remove(tree);
        }
        result
    }

    /// Retries the reset of every tree whose reset failed earlier.
    pub(super) fn retry_failed_resets(&self) {
        let failed: Vec<TreeId> = lock(&self.state).reset_failed.iter().cloned().collect();
        for tree in failed {
            let still_held = lock(&self.state).held.contains_key(&tree);
            if still_held {
                lock(&self.state).reset_failed.remove(&tree);
            } else if let Err(error) = self.reset_tree(&tree, None) {
                eprintln!("ignite: tree {tree} is still not reset: {error}");
            }
        }
    }

    /// Drops the held tickets whose tree is no longer leased to the same session. A landing
    /// frees its ticket's tree through the build trees, not through this module, so the entry
    /// would otherwise outlive the lease: it would still answer for the tree's path, and the
    /// stall check would try to release a ticket that is already closed.
    pub(super) fn forget_released(&self) {
        let trees: Vec<(TreeId, SessionId)> = lock(&self.state)
            .held
            .values()
            .map(|h| (h.tree.clone(), h.session.clone()))
            .collect();
        for (tree, session) in trees {
            let row = tree_rows::get(&lock(&self.db), tree.as_str());
            // A row that cannot be read, or that does not exist, proves nothing, so the entry
            // stays.
            let Ok(Some(row)) = row else { continue };
            let leased = row.state == tree_rows::State::Leased.as_str()
                && row.session.as_deref() == Some(session.as_str());
            if !leased {
                let mut state = lock(&self.state);
                if let Some(held) = state.held.remove(&tree) {
                    state.dirty.remove(&held.ticket);
                }
                drop(state);
                let _ = shown::clear(&lock(&self.db), tree.as_str());
                self.forget_activity(&tree);
            }
        }
    }
}
