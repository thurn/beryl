//! What a landing keeps from one pass to the next (see "Landing" in `docs/ignite.md`).
//!
//! A landing whose tip moved while its gate ran rebases onto the new tip and gates again. On
//! 2026-11-18 each such pass re-ran the whole gate, often in another landing tree that had to
//! build again from where that tree last stood, and under load a pass took 10 minutes or more,
//! during which the tip moved again. Two things now make a lost race cheap:
//!
//! - [`GateHome`]: a landing gates every pass in the tree it gated its first pass in. A landing
//!   tree stays leased to the landing until it ends and is checked out at each new rebased
//!   commit, so its builds are incremental from the last pass; a landing that started in its
//!   ticket tree stays there, because the rebase moves that tree too. A landing whose branch
//!   was rebased over other commits starts in its ticket tree too when that tree leaves less to
//!   build than any free landing tree (see [`LandTrees::ticket_builds_less`]).
//! - [`Passed`]: the last gate that passed, so the next pass judges again only the families
//!   whose inputs the commits landed since touch (see [`Carried`]).

use std::sync::Arc;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use super::Rebased;
use super::env::{GateTree, LandTrees, TicketTree};
use crate::checks::{Carried, GateRun};
use crate::server::Context;
use crate::steps;

/// Where a landing gates its passes.
enum Place {
    /// The ticket tree, which every rebase moves to the new rebased commit.
    Ticket,
    /// A landing tree leased to this landing until it ends.
    Landing(GateTree),
}

/// The tree a landing gates in, chosen on its first gated pass and kept for the rest. A
/// landing tree it holds is freed when this is dropped, however the landing ends.
pub(super) struct GateHome {
    trees: Arc<dyn LandTrees>,
    place: Option<Place>,
    /// The commit the ticket tree had checked out before the landing's first rebase, which is
    /// what its builds match; `None` when it could not be read.
    built: Option<Commit>,
}

impl GateHome {
    pub(super) fn new(trees: Arc<dyn LandTrees>, built: Option<Commit>) -> Self {
        Self {
            trees,
            place: None,
            built,
        }
    }

    /// The tree this pass gates the rebased commit `head`, which changes `change` from the tip,
    /// in. The first pass
    /// gates in the ticket tree when nothing landed since the branch's base, because the ticket
    /// tree has built that commit already, or when it leaves less to build than any free
    /// landing tree; otherwise in the free landing tree built nearest it, or in the ticket tree
    /// when every landing tree is busy. Later passes keep that tree; a landing tree is checked
    /// out at `head`.
    pub(super) fn for_pass(
        &mut self,
        holder: &str,
        rebased: &Rebased,
        ticket_tree: &TicketTree,
        change: &[String],
    ) -> Result<GateTree> {
        let Rebased {
            head, landed_since, ..
        } = rebased;
        let ticket = GateTree {
            tree: ticket_tree.tree.clone(),
            path: ticket_tree.path.clone(),
        };
        match self.place.take() {
            Some(Place::Ticket) => {
                self.place = Some(Place::Ticket);
                return Ok(ticket);
            }
            None if *landed_since == 0 || self.ticket_builds_less(ticket_tree, head, change) => {
                self.place = Some(Place::Ticket);
                return Ok(ticket);
            }
            Some(Place::Landing(kept)) => {
                steps::step(format!("moving {} to the rebased commit", kept.tree));
                let leased = self.trees.lease_landing(holder, head);
                // A tree other than the one held (a lease with no handle to match it by) means
                // the held one is no longer this landing's to keep.
                if !matches!(&leased, Ok(Some(tree)) if tree.tree == kept.tree) {
                    self.free(&kept, None);
                }
                self.place = Some(match leased? {
                    Some(tree) => Place::Landing(tree),
                    None => Place::Ticket,
                });
            }
            None => {
                steps::step("taking a landing tree");
                self.place = Some(match self.trees.lease_landing(holder, head)? {
                    Some(tree) => Place::Landing(tree),
                    None => Place::Ticket,
                });
            }
        }
        Ok(match &self.place {
            Some(Place::Landing(tree)) => tree.clone(),
            _ => ticket,
        })
    }

    /// Whether the ticket tree leaves less to build at `head` than any free landing tree. A
    /// failure to measure keeps the landing tree, as before there was a measure.
    fn ticket_builds_less(&self, ticket: &TicketTree, head: &Commit, change: &[String]) -> bool {
        match self
            .trees
            .ticket_builds_less(&ticket.tree, self.built.as_ref(), head, change)
        {
            Ok(less) => {
                if less {
                    steps::step(format!(
                        "gating in {}, which leaves less to build than any free landing tree",
                        ticket.tree
                    ));
                }
                less
            }
            Err(error) => {
                eprintln!("ignite: the landing trees' builds were not measured: {error}");
                false
            }
        }
    }

    /// Frees the landing tree the landing holds, if any; `built` is the commit its builds now
    /// match.
    pub(super) fn release(&mut self, built: Option<&Commit>) {
        if let Some(Place::Landing(tree)) = self.place.take() {
            self.free(&tree, built);
        }
    }

    fn free(&self, tree: &GateTree, built: Option<&Commit>) {
        if let Err(error) = self.trees.release_landing(&tree.tree, built) {
            eprintln!("ignite: landing tree {} not freed: {error}", tree.tree);
        }
    }
}

impl Drop for GateHome {
    fn drop(&mut self) {
        self.release(None);
    }
}

/// The last gate of a landing that passed, and what it gated.
pub(super) struct Passed {
    /// The rebased commit the gate passed on.
    head: Commit,
    /// The change's own files on that commit.
    changed: Vec<String>,
    gated: GateRun,
}

impl Passed {
    pub(super) fn new(head: Commit, changed: Vec<String>, gated: GateRun) -> Self {
        Self {
            head,
            changed,
            gated,
        }
    }

    /// What the gate of `head` may carry over from this one. `None` when the change's own files
    /// differ from those this gate passed with (a rebase that merged a landed edit into one of
    /// them), because the gate decides which families the change can affect from them.
    pub(super) fn carried(
        &self,
        ctx: &Context,
        head: &Commit,
        changed: &[String],
    ) -> Result<Option<Carried>> {
        if self.changed != changed {
            return Ok(None);
        }
        Ok(Some(Carried {
            families: self.gated.scoped.clone(),
            moved: ctx.git.changed(&self.head, head)?,
        }))
    }

    /// `gated`, which carried families over from this gate, with this gate's verdicts on those
    /// families added as answered from the cache, so the landing's output and the release
    /// branch's decision see every family the change can affect. A carried family whose checks
    /// this gate left to tip verification is left there again.
    pub(super) fn merge(&self, mut gated: GateRun) -> GateRun {
        let carried: Vec<_> = self
            .gated
            .run
            .outcomes
            .iter()
            .filter(|(check, _)| gated.carried.iter().any(|f| f == check.family()))
            .cloned()
            .collect();
        gated.run.from_cache += u32::try_from(carried.len()).unwrap_or(u32::MAX);
        gated.run.outcomes.extend(carried);
        gated.left = gated.left.max(self.gated.left);
        for known in &self.gated.known_failing {
            let carried = gated.carried.iter().any(|f| f == known.0.family());
            if carried && !gated.known_failing.contains(known) {
                gated.known_failing.push(known.clone());
            }
        }
        for family in &gated.carried {
            if let Some(left) = self.gated.left_by_family.get(family) {
                gated.left_by_family.insert(family.clone(), *left);
            }
            if self.gated.run.cold_families.contains(family) {
                gated.run.cold_families.push(family.clone());
            }
        }
        gated
    }
}
