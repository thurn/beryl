//! The tip reserve: cores kept for the work of a tip round before `master` moves, first of all
//! its build checks, which decide when the green branch moves and which every new ticket waits
//! for.
//!
//! On 2026-11-19 on one machine, 8,053 of 9,692 commands started on one core, and every command
//! of the slowest landing ran on one core, because a higher class does not take cores from the
//! running grants of a lower one. So the machine keeps `tip_build_cores` out of the cores it
//! shares ([`super::Limits::tip_build`]), the way it keeps the express lane's cores:
//!
//! - A request with `Reserve::TipBuild` takes up to all of the reserve that no other such grant
//!   holds and starts at once, whatever the shared cores and the load outside Ignite. It takes
//!   the rest of what it wants from the shared cores ahead of every class, at once or, with a
//!   cores file, by growing (see `rank` in `ledger/order.rs`). Several such requests share the
//!   reserve in the order they were first asked for.
//! - A request with `Reserve::TipRound`, one of the round's other builds and checks before
//!   `master` moves, takes up to all of the reserve that no `Reserve::TipBuild` request takes or
//!   holds, and starts at once on it. It takes the rest of what it wants from the shared cores
//!   in its own class, `tip`, below agents. On 2026-11-25 one machine kept 2 cores for build
//!   checks only, and its other tip commands waited up to 1,500 s for cores behind agent work,
//!   so its tip rounds took 12 to 150 minutes and `master` stood still for hours.
//! - Express runs and release builds can borrow idle tip cores. Agents can borrow from either
//!   reserve while its owner has no work. Neither kind of loan delays the reserve owner.
//! - The reserve's cores come back when the grant is released, also for a grant a restarted
//!   daemon adopted ([`Ledger::adopt_reserved`]).

use std::collections::BTreeMap;

use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_core::traits::Reserve;

use super::super::measured::GrantUse;
use super::super::{Grant, GrantRequest};
use super::{Held, Ledger, Origin, Waiter, gives_back};

impl Ledger {
    /// The cores of the tip reserve each waiting request on it would take if it started now:
    /// what it wants, from what no grant holds, `Reserve::TipBuild` requests before
    /// `Reserve::TipRound` ones, each kind in the order the requests were first asked for. A
    /// request that cannot start for its resources or memory keeps its part until the next
    /// step. None while the machine is `pressed` for memory, when nothing new starts.
    pub(super) fn reserve_parts(&self, pressed: bool) -> BTreeMap<u64, u32> {
        let mut free = if pressed { 0 } else { self.tip_reserve_free() };
        let mut parts = BTreeMap::new();
        for reserve in [Reserve::TipBuild, Reserve::TipRound] {
            for waiter in &self.waiting {
                if free == 0 {
                    return parts;
                }
                if waiter.request.reserve != Some(reserve) {
                    continue;
                }
                let part = self.want_of(waiter).min(free);
                free -= part;
                parts.insert(waiter.id, part);
            }
        }
        parts
    }

    /// The cores of the tip reserve that grants on it hold.
    pub(super) fn tip_reserve_used(&self) -> u32 {
        self.held.values().map(|h| h.reserved).sum()
    }

    /// The cores of the tip reserve no grant on it holds. Cores lent to a loan
    /// count as free: the loan only borrows them while they are idle.
    fn tip_reserve_free(&self) -> u32 {
        self.limits
            .tip_build
            .saturating_sub(self.tip_reserve_used())
    }

    /// Records that adopted grant `id` was made from the tip reserve for `reserve`, so it holds
    /// the reserve's cores again, as many as it holds and the reserve has free, until it is
    /// released.
    pub fn adopt_reserved(&mut self, id: u64, reserve: Reserve) {
        let free = self.tip_reserve_free();
        if let Some(held) = self.held.get_mut(&id) {
            held.reserved = held.cores.min(free);
            held.reserve = Some(reserve);
        }
    }

    /// Marks an express run active before scheduling any work, even if it wants no extra cores.
    pub(in crate::sched) fn begin_express(
        &mut self,
        request: Option<GrantRequest>,
        now: Timestamp,
    ) -> u64 {
        let id = match request {
            Some(request) => self.enqueue(request, now),
            None => self.next_id(),
        };
        self.express_runs.insert(id);
        id
    }

    /// Adds idle tip cores to an express run's shared grant. Returns a grant even at zero
    /// extra cores when an express reserve is configured, so the run can release its claim.
    pub(in crate::sched) fn finish_express(&mut self, id: u64, wanted: u32) -> Option<u32> {
        self.waiting.retain(|waiter| waiter.id != id);
        let extra = wanted.min(
            self.tip_reserve_free()
                .saturating_sub(self.express_tip_loans()),
        );
        if let Some(held) = self.held.get_mut(&id) {
            held.cores += extra;
            held.first += extra;
            held.lent += extra;
            return Some(held.cores);
        }
        if extra == 0 && self.limits.express() == 0 {
            self.express_runs.remove(&id);
            return None;
        }
        self.held.insert(
            id,
            Held {
                class: Class::Land,
                cores: extra,
                first: extra,
                resources: Vec::new(),
                memory_mb: None,
                grows_to: None,
                shrinks: false,
                unseen_memory: false,
                stopping: false,
                origin: Origin::Granted,
                usage: GrantUse::default(),
                reserved: 0,
                lent: extra,
                agent_lent: 0,
                reserve: None,
                priority_ticket: None,
            },
        );
        Some(extra)
    }

    fn express_tip_loans(&self) -> u32 {
        self.held.values().map(|h| h.lent).sum()
    }

    pub(super) fn agent_borrowed(&self) -> u32 {
        self.held.values().map(|h| h.agent_lent).sum()
    }

    pub(super) fn express_active(&self) -> bool {
        !self.express_runs.is_empty()
    }

    /// Treat both idle reserves as one source of loans. A waiting request on the tip reserve
    /// counts even if its memory or resources keep it from starting this step.
    fn agent_reserve(&self) -> u32 {
        let tip_active = self.held.values().any(|h| h.reserve.is_some())
            || self.waiting.iter().any(|w| w.request.reserve.is_some());
        let tip = if tip_active {
            0
        } else {
            self.limits
                .tip_build
                .saturating_sub(self.express_tip_loans())
        };
        let express = if self.express_active() {
            0
        } else {
            self.limits.express()
        };
        tip + express
    }

    /// Space on either reserve that is still unused after charging active grants and any CPU
    /// use outside them. Express runs claim their base cores from their first scheduling step.
    fn reserve_room(&self) -> f64 {
        let held = self.tip_reserve_used() + self.express_tip_loans() + self.agent_borrowed();
        let express = if self.express_active() {
            self.limits.express()
        } else {
            0
        };
        f64::from(self.limits.reserved.saturating_sub(held))
            - self.untracked.max(f64::from(express))
    }

    pub(super) fn outside_cores(&self) -> u32 {
        (self.outside - self.reserve_room().max(0.0)).round() as u32
    }

    pub(super) fn agent_loan_room(&self) -> u32 {
        let idle = (self.reserve_room() - self.outside).round() as u32;
        let excess_shared =
            (self.committed_cores() + self.outside_cores()).saturating_sub(self.limits.cores);
        self.agent_reserve()
            .saturating_sub(self.agent_borrowed())
            .min(idle)
            .saturating_sub(excess_shared)
    }

    /// Reserve owners can start immediately. Shrink borrowers that can return cores and charge
    /// the rest to the shared pool until their commands finish. New loans still have to fit in
    /// any reserve left idle by the owners.
    pub(super) fn recall_agent_loans(&mut self) -> Vec<(u64, u32)> {
        let mut recall = self.agent_borrowed().saturating_sub(self.agent_reserve());
        let mut resized = Vec::new();
        for (&id, held) in self.held.iter_mut().rev() {
            if recall == 0 {
                break;
            }
            if held.grows_to.is_some() {
                let shrink = recall.min(held.agent_lent).min(gives_back(held));
                if shrink > 0 {
                    held.cores -= shrink;
                    held.agent_lent -= shrink;
                    recall -= shrink;
                    resized.push((id, held.cores));
                }
            }
        }
        for held in self.held.values_mut().rev() {
            if recall == 0 {
                break;
            }
            let returned = recall.min(held.agent_lent);
            held.agent_lent -= returned;
            recall -= returned;
        }
        resized
    }

    /// Records the grant with its tip-reserve and agent-loan portions.
    pub(super) fn hold(
        &mut self,
        waiter: Waiter,
        cores: u32,
        reserved: u32,
        agent_lent: u32,
        now: Timestamp,
    ) -> Grant {
        let cores = cores + reserved;
        let queued_s = now.seconds_since(waiter.since).max(0.0) as u64;
        // A grant with a cores file grows: its command follows the file through its jobserver,
        // and a command that rereads the file starts more threads.
        let grows_to = waiter
            .request
            .cores_file
            .is_some()
            .then(|| waiter.request.cores.clamp(1, self.limits.max_per_command()));
        self.held.insert(
            waiter.id,
            Held {
                class: waiter.request.class,
                cores,
                first: cores,
                resources: waiter.request.resources.clone(),
                memory_mb: waiter.request.memory_mb,
                grows_to,
                shrinks: waiter.request.shrinks,
                unseen_memory: true,
                stopping: false,
                origin: Origin::Granted,
                usage: GrantUse::default(),
                reserved,
                lent: 0,
                agent_lent,
                reserve: waiter.request.reserve,
                priority_ticket: waiter.request.priority_ticket.clone(),
            },
        );
        Grant {
            id: waiter.id,
            cores,
            resources: waiter.request.resources,
            queued_s,
            cores_file: waiter.request.cores_file,
            priority_ticket: waiter.request.priority_ticket,
        }
    }
}
