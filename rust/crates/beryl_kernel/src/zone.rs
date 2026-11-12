//! The zone-move chokepoints: every change of zone, every draw and every shuffle goes through
//! here, so identities, knowledge and zone membership stay in agreement (architecture §4.3,
//! §5, §13).
//!
//! They are mutators of [`State`] (the crate's contract, "Mutators"), and the only code that
//! writes the zones' membership and order.

use crate::bitboard::Bitboard;
use crate::chance;
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Table};
use crate::rng::SplitMix64;
use crate::state::{Counters, Derived, Object, ObjectFlags, State, Tracked, Zone, ZoneKey};
use crate::store::{Store, Stored};
use beryl_card::FaceIx;

/// What [`State::signature`] compares two objects by: the object, for a permanent its layer
/// results and what is tracked of it, and what the stores hold about it: its memos, the
/// counters it keeps in no field and the continuous effects on it.
pub type Signature<'a, L> = (Object<L>, Option<(Derived<L>, Tracked)>, Stored<'a, L>);

/// Where in a library an object goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Top,
    Bottom,
    /// Under this many cards from the top, or at the bottom of a library with fewer: Oust's
    /// "second from the top" is `UnderTop(1)`.
    UnderTop(u8),
}

impl<L: Layout> State<L> {
    /// Moves an object to a zone, to `place` of a library: the raw execution step of the
    /// pipeline (architecture §8.2), which the rules' `event` wraps with replacements and
    /// triggers, and which builds the event with the move's reason.
    ///
    /// [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves from
    /// one zone to another becomes a new object with no memory of, or relation to, its previous
    /// existence." The slot's incarnation is bumped, everything but its identity and owner is
    /// reset, and what the stores hold about it ends ([`crate::store`]).
    ///
    /// [CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003): "If an object would go to
    /// any library, graveyard, or hand other than its owner's, it goes to its owner's
    /// corresponding zone." Zones are kept by owner, so that holds by construction.
    pub fn move_object(
        &mut self,
        sink: &mut Sink<L>,
        slot: L::Slot,
        to: Zone,
        place: Place,
    ) -> Result<(), Full> {
        // A card about to be seen has its identity settled first, before anything reads it
        // ([`chance`]): CR 400.2 below has every player see a card in a public zone, and CR 402.3
        // its owner see one in their hand.
        if to.is_public() || to == Zone::Hand {
            chance::settle(self, sink, slot);
        }
        // [CR 708.9](https://yawgatog.com/resources/magic-rules/#R7089): "If a face-down permanent
        // ... moves from the battlefield to any other zone, its owner must reveal it to all players
        // as they move it." Whatever zone it goes to, a hidden one included.
        let o = *self.object(slot);
        if o.zone == Zone::Battlefield && o.face == FaceIx::DOWN {
            for k in self.knowledge.as_mut_slice() {
                k.insert(slot);
            }
        }
        let from = o.zone;
        leave(self, slot);
        self.forget_stored(slot);
        let timestamp = self.zones.next_timestamp;
        let o = self.object_mut(slot);
        o.incarnation = o.incarnation.wrapping_add(1);
        o.zone = to;
        o.base_controller = o.owner;
        // A token stays a token wherever it goes, until it ceases to exist (CR 111.7).
        o.flags = ObjectFlags(o.flags.0 & (ObjectFlags::TOKEN | ObjectFlags::ONE_ONE));
        o.damage = 0;
        // [CR 122.2](https://yawgatog.com/resources/magic-rules/#R1222): "Counters on an
        // object are not retained if that object moves from one zone to another." Those of
        // the table's kinds ended with what the stores hold.
        o.counters = Counters::default();
        o.attached_to = None;
        o.face = FaceIx::FRONT;
        o.combat = None;
        let owner = o.owner.index();
        match to {
            Zone::Library => match place {
                Place::Top => self.zones.library[owner].push(slot)?,
                Place::Bottom => self.zones.library[owner].insert(0, slot)?,
                Place::UnderTop(n) => {
                    let library = &mut self.zones.library[owner];
                    library.insert(library.len().saturating_sub(usize::from(n)), slot)?;
                }
            },
            Zone::Graveyard => self.zones.graveyard[owner].push(slot)?,
            Zone::Battlefield => {
                self.enter_battlefield(slot)?;
                // CR 302.6: a permanent that just arrived hasn't been under its controller's
                // control since their most recent turn began.
                let o = self.object_mut(slot);
                o.timestamp = timestamp;
                o.flags.set(ObjectFlags::SUMMONING_SICK, true);
                self.zones.next_timestamp = timestamp.wrapping_add(1);
                self.flags.derived_dirty = true;
            }
            Zone::Stack => self.zones.stack.insert(slot),
            Zone::Hand | Zone::Exile | Zone::Nowhere => {}
        }
        if (to as usize) < Zone::OWNED {
            self.zones.members[owner][to as usize].insert(slot);
        }
        if matches!(to, Zone::Battlefield | Zone::Nowhere) {
            self.forget_exiled_with(slot);
        }
        // CR 400.2: cards in public zones are seen by every player.
        if to.is_public() {
            for k in self.knowledge.as_mut_slice() {
                k.insert(slot);
            }
        }
        // [CR 402.3](https://yawgatog.com/resources/magic-rules/#R4023): "A player may arrange
        // their hand in any convenient fashion and look at it at any time." Whatever put the
        // card there: a draw, or a return from a library.
        if to == Zone::Hand {
            self.knowledge[owner].insert(slot);
        }
        if from == Zone::Hand && to == Zone::Library {
            forget_hand(self, Seat::BOTH[owner], slot);
        }
        Ok(())
    }

    /// Draws one card.
    ///
    /// [CR 121.1](https://yawgatog.com/resources/magic-rules/#R1211): "A player draws a card by
    /// putting the top card of their library into their hand."
    ///
    /// [CR 121.4](https://yawgatog.com/resources/magic-rules/#R1214): "A player who attempts to
    /// draw a card from a library with no cards in it loses the game the next time a player would
    /// receive priority."
    pub fn draw(&mut self, sink: &mut Sink<L>, player: Seat) -> Result<(), Full> {
        let Some(&slot) = self.zones.library[player.index()].as_slice().last() else {
            self.player_mut(player).drew_from_empty = true;
            return Ok(());
        };
        self.move_object(sink, slot, Zone::Hand, Place::Top)
    }

    /// Draws `n` cards, one at a time ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)):
    /// "If a player is instructed to draw multiple cards, that player performs that many
    /// individual card draws."
    pub fn draw_n(&mut self, sink: &mut Sink<L>, player: Seat, n: u8) -> Result<(), Full> {
        for _ in 0..n {
            self.draw(sink, player)?;
        }
        Ok(())
    }

    /// Moves a card within its owner's library to its top or bottom. Not a zone change, so the
    /// card stays the same object and whoever knew it still does: a player who scries or looks
    /// knows where they put each card.
    pub fn reorder_library(&mut self, slot: L::Slot, place: Place) -> Result<(), Full> {
        let library = &mut self.zones.library[self.objects[slot.index()].owner.index()];
        let i = library
            .position(slot)
            .expect("a card in its owner's library");
        library.remove(i);
        match place {
            Place::Top => library.push(slot),
            Place::Bottom => library.insert(0, slot),
            Place::UnderTop(n) => {
                library.insert(library.len().saturating_sub(usize::from(n)), slot)
            }
        }
    }

    /// Shuffles a library.
    ///
    /// [CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a library
    /// or a face-down pile of cards, randomize the cards within it so that no player knows their
    /// order."
    ///
    /// Slots are public handles (architecture §13), so randomizing the order of the slots alone
    /// would let a player follow a card they know through the shuffle. The identities are dealt
    /// anew among the shuffled slots and every player forgets them; then the slots are
    /// reordered. [CR 701.20d](https://yawgatog.com/resources/magic-rules/#R70120d): "If cards in
    /// a player's library are shuffled or otherwise reordered, any revealed cards that are
    /// reordered stop being revealed and become new objects."
    pub fn shuffle_library(&mut self, player: Seat) {
        let p = player.index();
        let library = self.zones.library[p].clone();
        let mut rng = self.rng.shuffle;
        self.rebind(library.as_slice(), &mut rng);
        self.rng.shuffle = rng;
        self.rng
            .shuffle
            .shuffle(self.zones.library[p].as_mut_slice());
    }

    /// Deals the identities of the cards in `slots`, all in one library, anew among those slots
    /// at random, as cards put there in a random order are: `rebind` with the game's stream
    /// for everything but shuffles.
    pub fn rebind_at_random(&mut self, slots: &[L::Slot]) {
        let mut rng = self.rng.other;
        self.rebind(slots, &mut rng);
        self.rng.other = rng;
    }

    /// Deals the identities of the cards in `slots`, all in one library, anew among those slots
    /// with `rng`, and every player forgets them: the slots are public handles (architecture
    /// §13), so this is how cards come to be in an order no player knows. Each becomes a new
    /// object ([CR 701.20d](https://yawgatog.com/resources/magic-rules/#R70120d)): "If cards in a
    /// player's library are shuffled or otherwise reordered, any revealed cards that are
    /// reordered stop being revealed and become new objects."
    pub(crate) fn rebind(&mut self, slots: &[L::Slot], rng: &mut SplitMix64) {
        let mut cards: L::Deck<L::Card> = Default::default();
        for &slot in slots {
            // A library never holds more than its owner's deck slots.
            let _ = cards.push(self.object(slot).card);
        }
        rng.shuffle(cards.as_mut_slice());
        for (&slot, &card) in slots.iter().zip(cards.iter()) {
            let o = self.object_mut(slot);
            o.card = card;
            o.incarnation = o.incarnation.wrapping_add(1);
        }
        let rebound: Bitboard<L::Slot> = slots.iter().copied().collect();
        self.forget_all_stored(rebound);
        for k in self.knowledge.as_mut_slice() {
            *k &= !rebound;
        }
    }

    /// A token in a zone other than the battlefield ceases to exist, and its slot returns to the
    /// shared pool ([CR 704.5d](https://yawgatog.com/resources/magic-rules/#R7045d)): "If a token
    /// is in a zone other than the battlefield, it ceases to exist." The incarnation is kept and
    /// bumped, so references to the token stay stale after the slot is reused. The cards exiled
    /// with it are exiled with nothing any more.
    pub fn cease_to_exist(&mut self, slot: L::Slot) {
        leave(self, slot);
        self.forget_stored(slot);
        let incarnation = self.object(slot).incarnation.wrapping_add(1);
        *self.object_mut(slot) = Object {
            incarnation,
            ..Object::EMPTY
        };
        self.forget_exiled_with(slot);
        for k in self.knowledge.as_mut_slice() {
            k.remove(slot);
        }
    }

    /// What makes two objects interchangeable (§7.2): every field but the incarnation, the
    /// timestamp and the battlefield index, for a permanent its characteristics after
    /// continuous effects and what is tracked of it this turn, what it remembers of how
    /// it was cast, and the continuous effects on it, so two untapped basic lands match but
    /// a stolen Mana Crypt does not match your own, nor a creature that got -2/-0 one that
    /// didn't, nor a kicked creature one that was not, nor a planeswalker whose loyalty
    /// ability was activated one whose was not, nor a permanent with a counter one without
    /// it, nor a creature that is bigger until end of turn one that is as big for good, nor
    /// a card a player may cast from where it is one they may not. With nothing stored
    /// about the object, its memos, the counters it keeps in no field and its effects cost
    /// one bit test ([`Stored`]).
    pub fn signature(&self, slot: L::Slot) -> Signature<'_, L> {
        let o = self.object(slot);
        let permanent =
            (o.zone == Zone::Battlefield).then(|| (*self.derived(slot), self.trackers(slot)));
        (
            Object {
                incarnation: 0,
                timestamp: 0,
                battlefield_index: 0,
                ..*o
            },
            permanent,
            self.stored(slot),
        )
    }

    /// The number of cards in a player's hand.
    pub fn hand_size(&self, player: Seat) -> u32 {
        self.zone(ZoneKey::Of(player, Zone::Hand)).count()
    }
}

/// A card put from its owner's hand into their library unrevealed, as Brainstorm puts two
/// back: the other player sees a card back go and cannot tell which of the hand's cards it
/// was, so they forget it and every card still in that hand, however they came to know them
/// (a look at the hand, a card they saw on top of the library drawn). A hand and a library are
/// hidden zones ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002): "Library and
/// hand are hidden zones, even if all the cards in one such zone happen to be revealed"), and
/// the owner keeps their hand in any order
/// ([CR 402.3](https://yawgatog.com/resources/magic-rules/#R4023): "A player may arrange their
/// hand in any convenient fashion").
fn forget_hand<L: Layout>(s: &mut State<L>, owner: Seat, moved: L::Slot) {
    let hand = s.zones.members[owner.index()][Zone::Hand as usize];
    for p in Seat::BOTH {
        if p != owner {
            let k = &mut s.knowledge[p.index()];
            k.remove(moved);
            *k &= !hand;
        }
    }
}

fn leave<L: Layout>(s: &mut State<L>, slot: L::Slot) {
    let o = *s.object(slot);
    let owner = o.owner.index();
    match o.zone {
        Zone::Library => remove_slot(&mut s.zones.library[owner], slot),
        Zone::Graveyard => remove_slot(&mut s.zones.graveyard[owner], slot),
        Zone::Battlefield => {
            s.leave_battlefield(slot);
            s.flags.derived_dirty = true;
            // What was tracked of it this turn ends with it (CR 400.7): the object it
            // returns as is new.
            s.trackers.forget(slot);
            // [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A permanent is
            // removed from combat if it leaves the battlefield". The creatures attacking it or
            // blocked by it stay in combat, related to nothing; the slot may soon hold
            // another object. What was attached to it is attached to nothing: the object it
            // returns as is new ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)),
            // and an Aura left unattached goes to the graveyard as a state-based action
            // (CR 704.5m).
            for i in 0..s.battlefield.len() {
                let other = s.object_mut(s.battlefield.as_slice()[i]);
                if other.combat == Some(slot) {
                    other.combat = None;
                }
                if other.attached_to == Some(slot) {
                    other.attached_to = None;
                }
            }
        }
        Zone::Stack => s.zones.stack.remove(slot),
        Zone::Hand | Zone::Exile | Zone::Nowhere => {}
    }
    if (o.zone as usize) < Zone::OWNED {
        s.zones.members[owner][o.zone as usize].remove(slot);
    }
}

fn remove_slot<S: SlotId>(list: &mut impl Queue<S>, slot: S) {
    if let Some(i) = list.position(slot) {
        list.remove(i);
    }
}
