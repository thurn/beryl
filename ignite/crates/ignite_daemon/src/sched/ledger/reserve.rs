//! The tip reserve: cores kept for the build checks of a tip round, which decide when the green
//! branch moves and which every new ticket waits for.
//!
//! On 2026-11-19 on one machine, 8,053 of 9,692 commands started on one core, and every command
//! of the slowest landing ran on one core, because a higher class does not take cores from the
//! running grants of a lower one. So the machine keeps `tip_build_cores` out of the cores it
//! shares ([`super::Limits::tip_build`]), the way it keeps the express lane's cores:
//!
//! - A request with `Reserve::TipBuild` takes up to all of the reserve that no other such grant
//!   holds and starts at once, whatever the shared cores and the load outside Ignite. It takes
//!   the rest of what it wants from the shared cores in its class, at once or, with a cores file,
//!   by growing. Several such requests share the reserve in the order they were first asked for.
//! - No other request is ever granted the reserve's cores. Idle ones are lent to the express
//!   lane and the release build ([`Ledger::lend_reserve`]), which do not stop a tip build from
//!   taking them.
//! - The reserve's cores come back when the grant is released, also for a grant a restarted
//!   daemon adopted ([`Ledger::adopt_reserved`]).

use std::collections::BTreeMap;

use ignite_core::protocol::Class;
use ignite_core::traits::Reserve;

use super::super::measured::GrantUse;
use super::{Held, Ledger, Origin};

impl Ledger {
    /// The cores of the tip reserve each waiting `Reserve::TipBuild` request would take if it
    /// started now: what it wants, from what no grant holds, in the order the requests were
    /// first asked for. A request that cannot start for its resources or memory keeps its part
    /// until the next step. None while the machine is `pressed` for memory, when nothing new
    /// starts.
    pub(super) fn reserve_parts(&self, pressed: bool) -> BTreeMap<u64, u32> {
        let mut free = if pressed { 0 } else { self.tip_reserve_free() };
        let mut parts = BTreeMap::new();
        for waiter in &self.waiting {
            if free == 0 {
                break;
            }
            if waiter.request.reserve != Some(Reserve::TipBuild) {
                continue;
            }
            let part = self.want_of(waiter).min(free);
            free -= part;
            parts.insert(waiter.id, part);
        }
        parts
    }

    /// The cores of the tip reserve that `Reserve::TipBuild` grants hold.
    pub(super) fn tip_reserve_used(&self) -> u32 {
        self.held.values().map(|h| h.reserved).sum()
    }

    /// The cores of the tip reserve no `Reserve::TipBuild` grant holds. Cores lent to a loan
    /// count as free: the loan only borrows them while they are idle.
    fn tip_reserve_free(&self) -> u32 {
        self.limits
            .tip_build
            .saturating_sub(self.tip_reserve_used())
    }

    /// Records that adopted grant `id` was made from the tip reserve, so it holds the reserve's
    /// cores again, as many as it holds and the reserve has free, until it is released.
    pub fn adopt_reserved(&mut self, id: u64) {
        let free = self.tip_reserve_free();
        if let Some(held) = self.held.get_mut(&id) {
            held.reserved = held.cores.min(free);
        }
    }

    /// Lends up to `wanted` idle cores of the tip reserve to a loan for the express lane or the
    /// release build: to grant `id` when the shared cores gave it some, or else to a new grant of
    /// `class`. Idle cores are those neither a `Reserve::TipBuild` grant holds nor another loan
    /// borrows. Returns the grant and its size, or `None` when nothing was lent and there is
    /// no grant `id`. Release the grant to return them.
    pub fn lend_reserve(
        &mut self,
        id: Option<u64>,
        class: Class,
        wanted: u32,
    ) -> Option<(u64, u32)> {
        let lent: u32 = self.held.values().map(|h| h.lent).sum();
        let extra = wanted.min(self.tip_reserve_free().saturating_sub(lent));
        if let Some(held) = id.and_then(|id| self.held.get_mut(&id)) {
            held.cores += extra;
            held.first += extra;
            held.lent += extra;
            return id.map(|id| (id, held.cores));
        }
        if extra == 0 {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.held.insert(
            id,
            Held {
                class,
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
            },
        );
        Some((id, extra))
    }
}
