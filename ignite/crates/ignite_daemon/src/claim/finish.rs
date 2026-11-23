//! Ending a held ticket: `ignite release`, `ignite close`, the stall procedure's release, and
//! freeing the tree afterwards.
//!
//! The hub can stop holding a ticket for its session without the daemon hearing of it: a ticket
//! closed by hand (`ignite ticket close`), a regression that upkeep reopens without an assignee,
//! or a ticket released or reassigned from the dashboard. The hub then refuses the session's
//! release and close. This module frees the tree anyway, after saving its work, when the hub
//! says the session no longer holds the ticket, and [`Claims::tick`] does the same for a session
//! that never tries. Until it did, a reopened regression stayed held by its old tree, and every
//! other worker's `ignite start` of it was refused as held by another session.

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
    /// How the hub took the release.
    pub ended: Ended,
}

/// How the hub took a release or close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// The hub took it.
    Taken,
    /// The ticket had already been closed at the hub, so the hub took nothing.
    AlreadyClosed,
    /// The ticket is open but the hub no longer names the session as its holder, so the hub
    /// took nothing.
    NotHeld,
}

/// What the hub says of a ticket that this machine holds for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AtHub {
    /// The session holds it there too.
    Held,
    Closed,
    /// It is open and has no assignee, or another session's.
    Lost,
    /// The hub could not be read, which proves nothing.
    Unknown,
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
        let given = self
            .prepare_release(ctx, &held, |_| note, needs, ReleaseCause::GaveUp)
            .and_then(|release| self.give_back(&held, release, req.pid));
        match given {
            Ok(Given {
                ended: Ended::AlreadyClosed,
                reset,
                ..
            }) => brief::already_closed(&held.ticket, &held.tree, &title, reset),
            Ok(Given {
                ended: Ended::NotHeld,
                saved,
                reset,
            }) => brief::not_held(&held.ticket, &held.tree, &saved, &title, reset),
            Ok(Given {
                ended: Ended::Taken,
                saved,
                reset,
            }) => brief::released(&held.ticket, &held.tree, &saved, &title, reset),
            Err(error) => failure(&error, &format!("{} is still yours", held.ticket)),
        }
    }

    /// Saves the work before preparing the request to return its lease.
    pub(super) fn prepare_release(
        &self,
        ctx: &Context,
        held: &Held,
        note: impl FnOnce(&Saved) -> String,
        needs: Option<String>,
        cause: ReleaseCause,
    ) -> Result<ReleaseRequest> {
        let saved = self.trees.save(&held.tree, &held.ticket)?;
        Ok(ReleaseRequest {
            ticket: held.ticket.clone(),
            machine: ctx.machine.clone(),
            session: held.session.clone(),
            note: note(&saved),
            needs,
            saved,
            cause,
        })
    }

    /// Returns the lease at the hub, then frees the tree. A refusal that proves the hub
    /// has already closed or taken back the ticket also frees the tree.
    pub(super) fn give_back(
        &self,
        held: &Held,
        release: ReleaseRequest,
        caller: Option<u32>,
    ) -> Result<Given> {
        let released = self.desk.release(&release);
        let ended = match released {
            Ok(()) => Ended::Taken,
            Err(error) => self.refused_as_not_held(held, &error).ok_or(error)?,
        };
        Ok(Given {
            saved: release.saved,
            reset: self.free(held, caller),
            ended,
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
        let ended = match closed {
            Ok(()) => Ended::Taken,
            Err(error) => match self.refused_as_not_held(&held, &error) {
                Some(ended) => ended,
                None => return failure(&error, &format!("{} is still open", held.ticket)),
            },
        };
        match ended {
            Ended::Taken => {
                let dropped = self.trees.drop_saved(&held.ticket).err();
                let reset = self.free(&held, req.pid);
                brief::closed(&held.ticket, &held.tree, &reason, &title, dropped, reset)
            }
            Ended::AlreadyClosed => {
                let dropped = self.trees.drop_saved(&held.ticket).err();
                let reset = self.free(&held, req.pid);
                if let Some(error) = dropped {
                    eprintln!(
                        "ignite: the saved branch of {} remains: {error}",
                        held.ticket
                    );
                }
                brief::already_closed(&held.ticket, &held.tree, &title, reset)
            }
            // The ticket stays open for whoever works it next, so its work is saved, not
            // dropped.
            Ended::NotHeld => match self.trees.save(&held.tree, &held.ticket) {
                Ok(saved) => {
                    let reset = self.free(&held, req.pid);
                    brief::not_held(&held.ticket, &held.tree, &saved, &title, reset)
                }
                Err(error) => failure(&error, &format!("{} not saved", held.tree)),
            },
        }
    }

    /// What the hub's refusal of a release or close means, when it only means that the hub no
    /// longer holds the ticket for the session. Any other refusal is `None`, and the session
    /// keeps the ticket.
    fn refused_as_not_held(&self, held: &Held, error: &Error) -> Option<Ended> {
        if !matches!(error, Error::Conflict(_)) {
            return None;
        }
        match self.at_hub(&held.ticket, &held.session) {
            AtHub::Closed => Some(Ended::AlreadyClosed),
            AtHub::Lost => Some(Ended::NotHeld),
            AtHub::Held | AtHub::Unknown => None,
        }
    }

    /// What the hub says of `ticket`, which this machine holds for `session`.
    pub(super) fn at_hub(&self, ticket: &TicketId, session: &SessionId) -> AtHub {
        match self.desk.detail(ticket) {
            Err(_) => AtHub::Unknown,
            Ok(detail) if detail.ticket.status == Status::Closed => AtHub::Closed,
            Ok(detail) if detail.ticket.assignee.as_deref() == Some(session.as_str()) => {
                AtHub::Held
            }
            Ok(_) => AtHub::Lost,
        }
    }

    /// Frees the trees of held tickets that the hub no longer holds for their session: closed
    /// by hand (`ignite ticket close`), or reopened or reassigned at the hub, none of which tells
    /// the daemon. A tree that a previous daemon leased (`at` unknown) is freed only when its
    /// ticket is closed, because it may be a waiting `ignite start`'s tree that the session has
    /// not claimed yet. The tree's work is saved first, like a release; if the save fails the
    /// ticket stays held and the next tick tries again.
    pub(super) fn free_lost_tickets(&self) {
        let held: Vec<Held> = lock(&self.state).held.values().cloned().collect();
        for held in held {
            let gone = match self.at_hub(&held.ticket, &held.session) {
                AtHub::Closed => "closed",
                AtHub::Lost if held.at.is_some() => "no longer held",
                AtHub::Lost | AtHub::Held | AtHub::Unknown => continue,
            };
            if let Err(error) = self.free_saved(&held) {
                eprintln!("ignite: {} is {gone} but {error}", held.ticket);
            }
        }
    }

    /// Saves a tree's work and frees it, for a ticket nobody on this machine holds any more.
    /// A failed save keeps the ticket held.
    pub(super) fn free_saved(&self, held: &Held) -> std::result::Result<(), String> {
        if let Err(error) = self.trees.save(&held.tree, &held.ticket) {
            return Err(format!("{} not saved: {error}", held.tree));
        }
        match self.free(held, None) {
            Some(error) => Err(format!("{} not reset: {error}", held.tree)),
            None => Ok(()),
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
