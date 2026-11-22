//! The state's internal invariants, which fuzzing checks after every decision (architecture
//! §17.4). A violation is an engine bug, never a rules outcome.
//!
//! Implements: none. The checks validate the engine's internal state.

use crate::bitboard::Bitboard;
use crate::format::Format;
use crate::ids::{CardId, Seat, SlotId, SlotPlace};
use crate::layout::{Layout, Queue, Table};
use crate::state::{AbilityTarget, Flag, Object, Pending, State, Zone};
use crate::table::GameTable;
use beryl_card::FaceIx;

/// Which invariant failed, and the slot or player it failed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Violation {
    pub what: &'static str,
    /// The slot or player concerned, if one is.
    pub at: Option<At>,
}

/// What a [`Violation`] concerns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// A slot, by its number.
    Slot(usize),
    Player(Seat),
}

pub(crate) fn check(ok: bool, what: &'static str, at: Option<At>) -> Result<(), Violation> {
    if ok {
        Ok(())
    } else {
        Err(Violation { what, at })
    }
}

impl<L: Layout> State<L> {
    /// Checks the invariants that hold whenever `next` has returned: zone bitboards and lists
    /// agree with the objects' zones, no object is in two zones, every stack reference names
    /// a live incarnation, knowledge covers the public zones, off-battlefield objects carry no
    /// permanent status, no player who has lost is still playing, and derived characteristics
    /// are up to date (§9: reads never recompute).
    pub fn check_invariants(&self, t: &GameTable) -> Result<(), Violation> {
        if let Some(outer) = self.suspended_game() {
            check(
                matches!(outer.pending(), Pending::Resolving(r) if r.question.is_empty()),
                "a subgame with no suspended resolution",
                None,
            )?;
            check(
                outer.outcome().is_none(),
                "a suspended game with an outcome",
                None,
            )?;
            outer.check_invariants(t)?;
        }
        self.check_objects(t)?;
        self.check_zones()?;
        self.check_players(t.format())?;
        self.check_stores(t)?;
        self.check_last_known()?;
        check(
            self.outcome.is_some() || !self.flags.derived_dirty,
            "derived characteristics are stale",
            None,
        )
    }

    fn check_objects(&self, t: &GameTable) -> Result<(), Violation> {
        for slot in L::Slot::all() {
            let at = Some(At::Slot(slot.index()));
            let o = self.object(slot);
            let (deck_owner, in_deck) = match slot.place() {
                SlotPlace::Deck(player, card) => (Some(player), card < t.deck_len(player)),
                SlotPlace::Pool(_) => (None, false),
            };
            if o.zone == Zone::Nowhere {
                check(
                    !in_deck || self.suspended_game().is_some(),
                    "a deck card is nowhere",
                    at,
                )?;
                continue;
            }
            let token = o.flags.has(Flag::Token);
            if deck_owner.is_none() {
                // The shared pool holds tokens.
                check(token, "a non-token in the token pool", at)?;
            } else {
                check(in_deck && !token, "an object outside the deck slots", at)?;
                check(
                    Some(o.owner) == deck_owner,
                    "a deck card with another owner",
                    at,
                )?;
            }
            check(o.card.index() < t.len(), "no such card", at)?;
            for link in o.names() {
                check(
                    self.object(link).zone != Zone::Nowhere,
                    "a link to an empty slot",
                    at,
                )?;
            }
            // An Aura may enchant a card in a graveyard (see CR 303.4, Animate Dead's
            // "enchant creature card in a graveyard").
            let attachable = |link: Option<L::Slot>| {
                link.is_some_and(|l| {
                    matches!(self.object(l).zone, Zone::Battlefield | Zone::Graveyard)
                })
            };
            check(
                o.attached_to.is_none() || attachable(o.attached_to),
                "attached to an object off the battlefield and out of the graveyards",
                at,
            )?;
            // What is attached to a permanent phases out with it (see CR 702.26g).
            check(
                o.flags.has(Flag::PhasedOut)
                    || !o.attached_to.is_some_and(|l| self.phased_out.contains(l)),
                "a phased-in permanent attached to a phased-out one",
                at,
            )?;
            if !matches!(o.zone, Zone::Battlefield | Zone::Stack) {
                // What may stay off the battlefield: being a token. What exiled a card is
                // a memo of it, held to the exile zone by the Memos store's own check.
                check(
                    *o == Object {
                        card: o.card,
                        incarnation: o.incarnation,
                        zone: o.zone,
                        owner: o.owner,
                        base_controller: o.owner,
                        timestamp: o.timestamp,
                        flags: o.flags.kept_across_zones(),
                        ..Object::EMPTY
                    },
                    "permanent status off the battlefield",
                    at,
                )?;
            }
            // A blocker blocks attackers, or nothing once they left combat (see CR 506.4).
            check(
                o.blocked_attackers()
                    .iter()
                    .all(|a| self.object(a).is_attacking()),
                "a blocker blocking a nonattacker",
                at,
            )?;
            // See CR 400.2: public zones are seen by everyone; see CR 402.3: a hand by its owner; CR
            // 708.5: a face-down permanent by its controller.
            let known_by = |p: Seat| self.knows(p, slot);
            if o.face == FaceIx::DOWN {
                check(
                    o.zone == Zone::Battlefield && known_by(self.controller(slot)),
                    "a face-down permanent its controller doesn't know",
                    at,
                )?;
            } else if o.zone.is_public() {
                check(
                    self.who_knows(slot) == self.players(),
                    "a public card unknown",
                    at,
                )?;
            }
            if o.zone == Zone::Hand {
                check(known_by(o.owner), "a hand card its owner doesn't know", at)?;
            }
        }
        Ok(())
    }

    fn check_zones(&self) -> Result<(), Violation> {
        let mut members: L::PerPlayer<_> = Table::filled([Bitboard::EMPTY; Zone::OWNED]);
        let mut stack = Bitboard::EMPTY;
        let mut phased_out = Bitboard::EMPTY;
        for slot in L::Slot::all() {
            let o = self.object(slot);
            match o.zone {
                Zone::Stack => stack.insert(slot),
                Zone::Nowhere => {}
                // A phased-out permanent is on the battlefield by its zone and in the set of
                // phased-out permanents instead of its owner's zone set (`crate::phasing`).
                Zone::Battlefield if o.flags.has(Flag::PhasedOut) => phased_out.insert(slot),
                // A card of a merged permanent is part of the permanent it names.
                Zone::Merged => check(
                    self.memos(slot)
                        .iter()
                        .any(|m| matches!(m, crate::memos::Memo::PartOf(_))),
                    "a card in the merged zone is part of no permanent",
                    Some(At::Slot(slot.index())),
                )?,
                zone @ (Zone::Library
                | Zone::Hand
                | Zone::Battlefield
                | Zone::Graveyard
                | Zone::Exile) => members[o.owner.index()][zone.index()].insert(slot),
            }
        }
        check(
            phased_out == self.phased_out,
            "the phased-out set disagrees with the objects' flags",
            None,
        )?;
        check(
            members == self.zones.members,
            "zone bitboards disagree",
            None,
        )?;
        check(
            stack == self.zones.stack,
            "the stack bitboard disagrees",
            None,
        )?;
        for p in Seat::BOTH {
            let owned = |zone: Zone| members[p.index()][zone.index()];
            let listed = |list: &[L::Slot]| -> Option<Bitboard<L::Slot>> {
                let set: Bitboard<L::Slot> = list.iter().copied().collect();
                (usize::try_from(set.count()).expect("a bit count fits in usize") == list.len())
                    .then_some(set)
            };
            check(
                listed(self.library(p)) == Some(owned(Zone::Library)),
                "the library list disagrees",
                Some(At::Player(p)),
            )?;
            check(
                listed(self.graveyard(p)) == Some(owned(Zone::Graveyard)),
                "the graveyard list disagrees",
                Some(At::Player(p)),
            )?;
        }
        let battlefield = self.in_zone(Zone::Battlefield);
        let listed: Bitboard<L::Slot> = self.battlefield().iter().copied().collect();
        check(
            listed == battlefield
                && usize::try_from(listed.count()).expect("a bit count fits in usize")
                    == self.battlefield().len(),
            "the battlefield list disagrees",
            None,
        )?;
        for (i, &slot) in self.battlefield().iter().enumerate() {
            check(
                self.battlefield_position(slot) == Some(i),
                "the battlefield index disagrees",
                Some(At::Slot(slot.index())),
            )?;
        }
        check(
            L::Slot::all()
                .filter_map(|slot| self.battlefield_position(slot))
                .count()
                == self.battlefield().len(),
            "the battlefield index lists a slot off the battlefield",
            None,
        )?;
        for (i, e) in self.stack.iter().enumerate() {
            // Only an entry above an ability can name it, which is what lets the kernel
            // repair the names when an entry leaves from below the top
            // ([`State::take_off_stack`]).
            for target in e.targets.iter() {
                if let Some(AbilityTarget::At(place)) = target.as_ability() {
                    check(
                        place < i && self.stack.as_slice()[place].spell().is_none(),
                        "an ability target names no ability below its own entry",
                        None,
                    )?;
                }
            }
            if let Some(r) = e.spell() {
                check(
                    self.is_current(r) && self.object(r.slot).zone == Zone::Stack,
                    "a spell on the stack names a stale object",
                    Some(At::Slot(r.slot.index())),
                )?;
            }
        }
        Ok(())
    }

    fn check_players(&self, format: Format) -> Result<(), Violation> {
        let t = self.turn;
        // Both players have passed in succession while the top of the stack resolves, and a
        // resolution paused for an answer is still resolving (§7.3).
        let resolving = matches!(self.pending, Pending::Resolving(_));
        check(
            t.passes < format.seats() || (t.passes == format.seats() && resolving),
            "a bad turn",
            None,
        )?;
        // State-based actions are checked before anyone gets priority, so a player who has
        // lost has left the game by the time a priority decision is open.
        // See [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0
        // or less life, that player loses the game." see [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b):
        // "If a player attempted to draw a card from a library with no cards in it since the
        // last time state-based actions were checked, that player loses the game."
        if self.outcome.is_none() && self.pending == Pending::None {
            let losses = format.losses();
            for p in Seat::BOTH {
                let player = self.player(p);
                check(
                    player.life > losses.life && !(losses.empty_library && player.drew_from_empty),
                    "a player who has lost has priority",
                    Some(At::Player(p)),
                )?;
            }
        }
        Ok(())
    }
}
