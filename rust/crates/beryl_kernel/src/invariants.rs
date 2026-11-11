//! The state's internal invariants, which fuzzing checks after every decision (architecture
//! §17.4). A violation is an engine bug, never a rules outcome.

use crate::bitboard::Bitboard;
use crate::ids::{Place, Seat, SlotId};
use crate::layout::{Layout, Queue, Table};
use crate::state::{Object, ObjectFlags, Pending, State, Zone};
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

fn check(ok: bool, what: &'static str, at: Option<At>) -> Result<(), Violation> {
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
        self.check_objects(t)?;
        self.check_zones()?;
        self.check_players()?;
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
                Place::Deck(player, card) => (Some(player), card < t.deck(player).len()),
                Place::Pool(_) => (None, false),
            };
            if o.zone == Zone::Nowhere {
                check(!in_deck, "a deck card is nowhere", at)?;
                continue;
            }
            let token = o.flags.has(ObjectFlags::TOKEN);
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
            for link in [o.attached_to, o.linked, o.combat] {
                check(
                    link.is_none_or(|l| self.object(l).zone != Zone::Nowhere),
                    "a link to an empty slot",
                    at,
                )?;
            }
            let on_battlefield = |link: Option<L::Slot>| {
                link.is_some_and(|l| self.object(l).zone == Zone::Battlefield)
            };
            check(
                o.attached_to.is_none() || on_battlefield(o.attached_to),
                "attached to an object off the battlefield",
                at,
            )?;
            // An object exiled "until" its exiler leaves returns as it leaves (CR 610.3).
            check(
                !o.flags.has(ObjectFlags::EXILED_UNTIL) || on_battlefield(o.linked),
                "exiled until a permanent that already left",
                at,
            )?;
            if !matches!(o.zone, Zone::Battlefield | Zone::Stack) {
                // What may stay off the battlefield: being a token, and a link to the
                // permanent that exiled it "until" it leaves (CR 610.3) or whose ability
                // refers to the cards exiled with it (CR 607.2a).
                let kept = ObjectFlags::TOKEN
                    | ObjectFlags::ONE_ONE
                    | ObjectFlags::EXILED_UNTIL
                    | ObjectFlags::EXILED_WITH;
                let exiled_by = o
                    .flags
                    .has(ObjectFlags::EXILED_UNTIL | ObjectFlags::EXILED_WITH);
                check(
                    *o == Object {
                        card: o.card,
                        incarnation: o.incarnation,
                        zone: o.zone,
                        owner: o.owner,
                        base_controller: o.owner,
                        timestamp: o.timestamp,
                        flags: ObjectFlags(o.flags.0 & kept),
                        linked: if exiled_by { o.linked } else { None },
                        ..Object::EMPTY
                    } && (!exiled_by || o.zone == Zone::Exile),
                    "permanent status off the battlefield",
                    at,
                )?;
            }
            // A blocker blocks an attacker, or nothing once that attacker left combat
            // (CR 506.4).
            check(
                !o.flags.has(ObjectFlags::BLOCKING)
                    || o.combat
                        .is_none_or(|a| self.object(a).flags.has(ObjectFlags::ATTACKING)),
                "a blocker blocking a nonattacker",
                at,
            )?;
            // CR 400.2: public zones are seen by everyone; CR 402.3: a hand by its owner; CR
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
        for slot in L::Slot::all() {
            let o = self.object(slot);
            match o.zone {
                Zone::Stack => stack.insert(slot),
                Zone::Nowhere => {}
                zone => members[o.owner.index()][zone as usize].insert(slot),
            }
        }
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
            let owned = |zone: Zone| members[p.index()][zone as usize];
            let listed = |list: &[L::Slot]| -> Option<Bitboard<L::Slot>> {
                let set: Bitboard<L::Slot> = list.iter().copied().collect();
                (set.count() as usize == list.len()).then_some(set)
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
            listed == battlefield && listed.count() as usize == self.battlefield().len(),
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
        for e in self.stack.iter() {
            if e.is_spell() {
                check(
                    e.object.is_some_and(|r| {
                        self.is_current(r) && self.object(r.slot).zone == Zone::Stack
                    }),
                    "a spell on the stack names a stale object",
                    e.object.map(|r| At::Slot(r.slot.index())),
                )?;
            }
        }
        Ok(())
    }

    fn check_players(&self) -> Result<(), Violation> {
        let t = self.turn;
        // Both players have passed in succession while the top of the stack resolves, and a
        // resolution paused for an answer is still resolving (§7.3).
        let resolving = matches!(self.pending, Pending::Resolving(_));
        check(
            usize::from(t.passes) < L::Slot::PLAYERS
                || (usize::from(t.passes) == L::Slot::PLAYERS && resolving),
            "a bad turn",
            None,
        )?;
        // State-based actions are checked before anyone gets priority, so a player who has
        // lost has left the game by the time a priority decision is open.
        // [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0
        // or less life, that player loses the game." [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b):
        // "If a player attempted to draw a card from a library with no cards in it since the
        // last time state-based actions were checked, that player loses the game."
        if self.outcome.is_none() && self.pending == Pending::None {
            for p in Seat::BOTH {
                let player = self.player(p);
                check(
                    player.life > 0 && !player.drew_from_empty,
                    "a player who has lost has priority",
                    Some(At::Player(p)),
                )?;
            }
        }
        Ok(())
    }
}
