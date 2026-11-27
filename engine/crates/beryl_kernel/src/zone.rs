//! The zone-move chokepoints: every change of zone, every draw and every shuffle goes through
//! here, so identities, knowledge and zone membership stay in agreement (architecture §4.3,
//! §5, §13).
//!
//! They are mutators of [`State`] (the crate's contract, "Mutators"), and the only code that
//! writes the zones' membership and order.
//!
//! Implements: CR 400.7 only, CR 400.7a, CR 701.20d, CR 708.9

use crate::bitboard::Bitboard;
use crate::chance;
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, PileCards, Piles, Queue, Table};
use crate::reference::{CardIn, Library};
use crate::rng::SplitMix64;
use crate::state::{
    CombatRole, Counters, Derived, Flag, Object, ObjectRef, State, Tracked, Zone, ZoneKey,
    is_shared,
};
use crate::store::{Store, Stored};
use beryl_card::FaceIx;

/// What [`State::signature`] compares two objects by: the object, for a permanent its layer
/// results and what is tracked of it, and what the stores hold about it: its memos, the
/// counters it keeps in no field and the continuous effects on it.
pub type Signature<'a, L> = (Object<L>, Option<(Derived<L>, Tracked)>, Stored<'a, L>);

/// The other cards of a merged permanent as it leaves the battlefield
/// ([`State::move_object`]). A pile of more is [`Full`].
type Pile<L> = <L as Layout>::Queue<<L as Layout>::Slot, 8>;

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
    /// reset, and what the stores hold about it ends ([`crate::store`]). An exhausted
    /// incarnation returns [`Full`].
    ///
    /// See [CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003): "If an object would go to any
    /// library, graveyard, or hand other than its owner's, it goes to its owner's corresponding
    /// zone." Zones are kept by owner, so that holds by construction.
    ///
    /// In the shared-deck format a card that enters the shared library or graveyard has no
    /// owner again: its owner and base controller become [`Seat::P0`] once the move has done
    /// what depends on who owned it ([`State::claim`]).
    pub fn move_object(
        &mut self,
        sink: &mut Sink<L>,
        slot: L::Slot,
        to: Zone,
        place: Place,
    ) -> Result<(), Full> {
        // The format's one test for each zone move: what only the shared-deck format does is
        // compiled out of the per-player move and kept out of line.
        if self.zones.mask == 0 {
            return self.move_shared(sink, slot, to, place);
        }
        self.move_in::<false>(sink, slot, to, place)
    }

    /// [`State::move_object`] in a game whose players share a library and a graveyard.
    #[cold]
    #[inline(never)]
    fn move_shared(
        &mut self,
        sink: &mut Sink<L>,
        slot: L::Slot,
        to: Zone,
        place: Place,
    ) -> Result<(), Full> {
        self.move_in::<true>(sink, slot, to, place)
    }

    /// [`State::move_object`], with `SHARED` saying whether the players share a library and a
    /// graveyard.
    #[allow(
        clippy::inline_always,
        reason = "each format's move is this body with the other format's code compiled out"
    )]
    #[inline(always)]
    fn move_in<const SHARED: bool>(
        &mut self,
        sink: &mut Sink<L>,
        slot: L::Slot,
        to: Zone,
        place: Place,
    ) -> Result<(), Full> {
        let incarnation = self.object(slot).incarnation.checked_add(1).ok_or(Full)?;
        // A card about to be seen has its identity settled first, before anything reads it
        // ([`chance`]): every player sees a card in a public zone (see CR 400.2 below), and
        // its owner sees one in their hand.
        if to.is_public() || to == Zone::Hand {
            chance::settle(self, sink, slot);
        }
        // See [CR 708.9](https://yawgatog.com/resources/magic-rules/#R7089): "If a face-down permanent
        // ... moves from the battlefield to any other zone, its owner must reveal it to all players
        // as they move it." Whatever zone it goes to, a hidden one included. A face-down
        // component under a merged permanent's face-up top is revealed too: its slot is known
        // to every player already, and the card it becomes is that component
        // ([`crate::Memo::FaceDownUnder`]).
        let o = self.object(slot).clone();
        let stays_down = o.zone == Zone::Stack && to == Zone::Battlefield && o.face == FaceIx::DOWN;
        if o.face == FaceIx::DOWN && !stays_down {
            // CR 708.9: "If a face-down spell moves from the stack to any zone other than
            // the battlefield, its owner must reveal it to all players as they move it."
            self.reveal(sink, o.owner, slot);
        }
        let from = o.zone;
        let old_incarnation = o.incarnation;
        // A token copy stays one ([`State::carried_copy`]); a card under a copy effect is the
        // card it really is again, with the effect ended with the rest of what the stores hold
        // ([`crate::Memo::Really`]).
        let (carried, pile) = if o.flags.has(Flag::Copy) {
            (self.leaving_copy(slot), self.leaving_pile(slot, from)?)
        } else {
            (None, None)
        };
        // CR 400.7a: a permanent spell's text changes go on with the permanent it becomes
        // (`crate::effects::text`).
        let text = if from == Zone::Stack && to == Zone::Battlefield && self.keyed.contains(slot) {
            Some(self.carried_text(slot)?)
        } else {
            None
        };
        leave::<L, SHARED>(self, slot, to);
        if self.keyed.contains(slot) {
            self.forget_moved_object(slot)?;
        }
        let o = self.object_mut(slot);
        o.incarnation = incarnation;
        o.zone = to;
        o.base_controller = o.owner;
        o.flags = o.flags.kept_across_zones();
        o.damage = <L::Count as crate::layout::Number>::ZERO;
        // See [CR 122.2](https://yawgatog.com/resources/magic-rules/#R1222): "Counters on an
        // object are not retained if that object moves from one zone to another." Those of
        // the table's kinds ended with what the stores hold.
        o.counters = Counters::default();
        o.attached_to = None;
        // See CR 716.4: "In every zone except the stack, and while on the stack not as an
        // Adventure, an adventurer card has only its normal characteristics."
        o.face = FaceIx::FRONT;
        o.combat = CombatRole::None;
        let owner = o.owner.index();
        let enters_shared = SHARED && is_shared(to);
        if enters_shared {
            enter_shared(self, slot, to, place)?;
        }
        match to {
            _ if enters_shared => {}
            Zone::Library => place_in_library(&mut self.zones.library, owner, slot, place)?,
            Zone::Graveyard => {
                self.zones.graveyard.push(owner, slot)?;
                self.flags.derived_dirty |= self.layers_read_graveyards();
            }
            Zone::Battlefield => {
                self.enter_battlefield(slot)?;
                // See CR 302.6: a permanent that just arrived hasn't been under its controller's
                // control since their most recent turn began.
                let timestamp = self.take_timestamp();
                let o = self.object_mut(slot);
                o.timestamp = timestamp;
                o.flags.set(Flag::SummoningSick, true);
                self.flags.derived_dirty = true;
            }
            Zone::Stack => self.zones.stack.insert(slot),
            Zone::Hand => self.flags.derived_dirty |= self.layers_read_hands(),
            Zone::Exile | Zone::Nowhere | Zone::Merged => {}
        }
        if (to.index()) < Zone::OWNED && !enters_shared {
            self.zones.members[owner][to.index()].insert(slot);
        }
        if let Some(carried) = carried {
            self.carry_copy(slot, carried)?;
        }
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            self.carry_text(slot, &text)?;
        }
        if matches!(to, Zone::Battlefield | Zone::Nowhere) {
            self.forget_links_to(slot);
        }
        if from == Zone::Stack && to != Zone::Battlefield {
            self.forget_convoked(ObjectRef {
                slot,
                incarnation: old_incarnation,
            });
        }
        // See CR 400.2: cards in public zones are seen by every player.
        if to.is_public() && !stays_down {
            for k in self.knowledge.as_mut_slice() {
                k.insert(slot);
            }
        }
        // See [CR 402.3](https://yawgatog.com/resources/magic-rules/#R4023): "A player may arrange
        // their hand in any convenient fashion and look at it at any time." Whatever put the
        // card there: a draw, or a return from a library.
        if to == Zone::Hand {
            self.knowledge[owner].insert(slot);
            if let Some(sharer) = self.sharer(Seat::BOTH[owner]) {
                self.knowledge[sharer.index()].insert(slot);
            }
        }
        if from == Zone::Hand && to == Zone::Library {
            forget_hand(self, Seat::BOTH[owner], slot);
        }
        // Last, once the reveal of a face-down card, the hand it left and the knowledge of that
        // hand have read who owned it.
        if enters_shared {
            self.disown(slot);
        }
        if let Some(pile) = pile {
            self.move_pile(sink, slot, &pile, to, place)?;
        }
        Ok(())
    }

    /// The other cards of the merged permanent in `slot` as it leaves `from`: none unless it
    /// is a merged permanent leaving the battlefield. Read before the move ends what the
    /// stores hold about it.
    #[cold]
    #[inline(never)]
    fn leaving_pile(&self, slot: L::Slot, from: Zone) -> Result<Option<Pile<L>>, Full> {
        let mut pile = Pile::<L>::default();
        if from == Zone::Battlefield {
            for card in self.pile(slot) {
                pile.push(card)?;
            }
        }
        Ok((!pile.is_empty()).then_some(pile))
    }

    /// Moves the other cards of a merged permanent that left the battlefield to the zone it
    /// went to (see [CR 731.3]): "If a merged permanent leaves the battlefield, one permanent leaves
    /// the battlefield and each of the individual components are put into the appropriate
    /// zone." Each becomes an object of its own there. Those that go to a graveyard or library
    /// go in the order they merged, above the permanent's own card; the rules then ask the
    /// owner to arrange them (see [CR 731.3a], [`State::arrange_lowest`]), which a mutator can't
    /// do. A token that ceases to exist leaves its
    /// cards in its owner's graveyard. Exiled with the permanent's own card, `leader`, each
    /// remembers it ([`crate::memos::Memo::ExiledFromPileOf`]), so that what finds one finds all.
    ///
    /// [CR 731.3]: https://yawgatog.com/resources/magic-rules/#R7313
    /// [CR 731.3a]: https://yawgatog.com/resources/magic-rules/#R7313a
    #[cold]
    #[inline(never)]
    fn move_pile(
        &mut self,
        sink: &mut Sink<L>,
        leader: L::Slot,
        pile: &Pile<L>,
        to: Zone,
        place: Place,
    ) -> Result<(), Full> {
        let to = if to == Zone::Nowhere {
            Zone::Graveyard
        } else {
            to
        };
        let together = to == Zone::Exile && !self.object(leader).is_token();
        // The leader has already moved: the link names the card it is in exile.
        let link = crate::memos::Link::to(self.object_ref(leader));
        for &card in pile.as_slice() {
            self.move_object(sink, card, to, place)?;
            if together {
                self.remember(card, crate::memos::Memo::ExiledFromPileOf(link))?;
            }
        }
        Ok(())
    }

    /// Puts the card in `card` lowest of `among`: cards of its owner's graveyard or library that
    /// went there together from merged or melded permanents, which the owner arranges (see
    /// [CR 731.3a]): "that player may arrange the new objects in any order. If it's put into its
    /// owner's library, that player doesn't reveal the order." Returns the slot that holds the
    /// card now, the lowest of `among`.
    ///
    /// In a graveyard, a public zone, the card moves to the lowest place `among` holds. In a
    /// library the slots stay where they are and the card trades identities with the one in the
    /// lowest slot, as a shuffle deals identities anew ([`State::rebind`]): slots are public
    /// handles (architecture §13), so a player who saw a slot move could follow its card. The
    /// two become new objects, which their owner knows ([`State::hide_library_order`]).
    ///
    /// In the shared-deck format the cards have no owner, and `arranger` arranges them and
    /// knows the two that trade identities. Otherwise `arranger` is not read.
    ///
    /// [CR 731.3a]: https://yawgatog.com/resources/magic-rules/#R7313a
    pub fn arrange_lowest(
        &mut self,
        card: L::Slot,
        among: Bitboard<L::Slot>,
        arranger: Seat,
    ) -> Result<L::Slot, Full> {
        let o = self.object(card);
        let shared = self.shares_zones();
        let (owner, zone) = (if shared { arranger } else { o.owner }, o.zone);
        let list = self.pile_of(o.owner);
        let piles = match zone {
            Zone::Graveyard => &mut self.zones.graveyard,
            Zone::Library => &mut self.zones.library,
            Zone::Hand
            | Zone::Battlefield
            | Zone::Exile
            | Zone::Stack
            | Zone::Nowhere
            | Zone::Merged => panic!("a card arranged outside a graveyard or library"),
        };
        let lowest = (piles.list(list).iter())
            .position(|&slot| among.contains(slot))
            .expect("the cards arranged in their owner's zone");
        let low = piles.list(list)[lowest];
        if low == card {
            return Ok(card);
        }
        if zone == Zone::Graveyard {
            let from = piles
                .position(list, card)
                .expect("a card in its owner's graveyard");
            piles.remove(list, from);
            if shared {
                piles.insert_shared(lowest, card)?;
            } else {
                piles.insert(list, lowest, card)?;
            }
            return Ok(card);
        }
        if self.object(card).incarnation == u16::MAX || self.object(low).incarnation == u16::MAX {
            return Err(Full);
        }
        let (moved, displaced) = (self.object(card).card, self.object(low).card);
        for (slot, identity) in [(low, moved), (card, displaced)] {
            let o = self.object_mut(slot);
            o.card = identity;
            o.incarnation += 1;
        }
        let pair = Bitboard::single(card) | Bitboard::single(low);
        self.forget_all_stored(pair);
        self.knowledge[owner.index()].insert(card);
        self.knowledge[owner.index()].insert(low);
        Ok(low)
    }

    /// Every player but `owner`, and whoever sees `owner`'s hidden zones as they do (see
    /// CR 724.4), forgets the cards `cards` of `owner`'s library, which `owner` is about to
    /// arrange there unseen (see [CR 401.4]): "That library's owner doesn't reveal the order in
    /// which the cards go into the library."
    ///
    /// [CR 401.4]: https://yawgatog.com/resources/magic-rules/#R4014
    pub fn hide_library_order(&mut self, owner: Seat, cards: Bitboard<L::Slot>) {
        let sharer = self.sharer(owner);
        for p in Seat::BOTH {
            if p != owner && Some(p) != sharer {
                self.knowledge[p.index()] &= !cards;
            }
        }
    }

    /// Draws one card and returns its slot, or records an attempt to draw from an empty library.
    ///
    /// See [CR 121.1](https://yawgatog.com/resources/magic-rules/#R1211): "A player draws a card by
    /// putting the top card of their library into their hand."
    ///
    /// See [CR 121.4](https://yawgatog.com/resources/magic-rules/#R1214): "A player who attempts to
    /// draw a card from a library with no cards in it loses the game the next time a player would
    /// receive priority."
    pub fn draw(&mut self, sink: &mut Sink<L>, player: Seat) -> Result<Option<L::Slot>, Full> {
        let Some(slot) = self.library_cards_from_top(player).next() else {
            self.player_mut(player).drew_from_empty = true;
            return Ok(None);
        };
        // A card drawn from the shared library is the drawing player's.
        if self.shares_zones() {
            self.claim(slot, player);
        }
        self.move_object(sink, slot, Zone::Hand, Place::Top)?;
        Ok(Some(slot))
    }

    /// Draws `n` cards, one at a time (see [CR 121.2]): "If a player is instructed to draw multiple
    /// cards, that player performs that many individual card draws."
    ///
    /// [CR 121.2]: https://yawgatog.com/resources/magic-rules/#R1212
    pub fn draw_n(&mut self, sink: &mut Sink<L>, player: Seat, n: u8) -> Result<(), Full> {
        for _ in 0..n {
            self.draw(sink, player)?;
        }
        Ok(())
    }

    /// Gives the card in `slot`, which is in a shared library or graveyard, to `player`, who is
    /// about to take it out: from then on `player` is its owner and its base controller, until it
    /// goes back to a shared zone. The shared-deck format: "The 'owner' of a card on the stack or
    /// permanent on the battlefield is considered to be the person who cast it."
    ///
    /// The kernel does not check, as the card leaves, that it was claimed: a card claimed by
    /// [`Seat::P0`] looks the same as one never claimed, so the rules' zone-change pipeline,
    /// which knows who takes each card, makes that check.
    #[cold]
    #[inline(never)]
    pub fn claim(&mut self, slot: L::Slot, player: Seat) {
        debug_assert!(
            self.shares_zones() && is_shared(self.object(slot).zone),
            "a card claimed outside a shared zone"
        );
        let o = self.object_mut(slot);
        o.owner = player;
        o.base_controller = player;
    }

    /// Undoes [`State::claim`] for a card whose move out of a shared zone was prepared and did
    /// not happen: it has no owner again. The zone move does the same for a card that enters
    /// a shared zone.
    #[cold]
    #[inline(never)]
    pub fn disown(&mut self, slot: L::Slot) {
        let o = self.object_mut(slot);
        o.owner = Seat::P0;
        o.base_controller = Seat::P0;
    }

    /// Moves a card within its owner's library to its top or bottom. Not a zone change, so the
    /// card stays the same object and whoever knew it still does: a player who scries or looks
    /// knows where they put each card.
    pub fn reorder_library(&mut self, card: CardIn<Library, L>, place: Place) -> Result<(), Full> {
        let slot = card.slot();
        let list = self.pile_of(self.objects[slot.index()].owner);
        let library = &mut self.zones.library;
        let i = library
            .position(list, slot)
            .expect("a card in its owner's library");
        library.remove(list, i);
        if self.zones.mask == 0 {
            place_in_shared_library(library, slot, place)
        } else {
            place_in_library(library, list, slot, place)
        }
    }

    /// Shuffles a library.
    ///
    /// See [CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a library or
    /// a face-down pile of cards, randomize the cards within it so that no player knows their
    /// order."
    ///
    /// Slots are public handles (architecture §13), so randomizing the order of the slots alone
    /// would let a player follow a card they know through the shuffle. The identities are dealt
    /// anew among the shuffled slots and every player forgets them; then the slots are reordered.
    /// See [CR 701.20d](https://yawgatog.com/resources/magic-rules/#R70120d): "If cards in a player's
    /// library are shuffled or otherwise reordered, any revealed cards that are reordered stop
    /// being revealed and become new objects."
    ///
    /// In the shared-deck format either player's library is the shared one.
    pub fn shuffle_library(&mut self, player: Seat) -> Result<(), Full> {
        let list = self.pile_of(player);
        let library = self.zones.library.clone();
        let mut rng = self.rng.shuffle;
        self.rebind(library.list(list), &mut rng)?;
        self.rng.shuffle = rng;
        self.rng.shuffle.shuffle(self.zones.library.list_mut(list));
        Ok(())
    }

    /// Moves a player's opening hand back onto their library before a mulligan shuffle.
    /// The moves are reported in card definition order. After a restart, the library keeps that
    /// order so renumbering slots gives the same hand. For ordinary openings, sorting the returned
    /// cards by slot preserves the seeded permutation used by recorded games.
    pub fn return_opening_hand(&mut self, sink: &mut Sink<L>, player: Seat) -> Result<(), Full> {
        let previous_top = self.library(player).len();
        for &slot in self.hand_by_definition(player).iter() {
            self.move_object(sink, slot, Zone::Library, Place::Top)?;
        }
        if !self.restarted() {
            let list = self.pile_of(player);
            self.zones.library.list_mut(list)[previous_top..].sort_unstable();
        }
        Ok(())
    }

    /// Shuffles a player's opening hand back into their library as one mulligan operation.
    /// See [CR 103.5]: "To take a mulligan, a player shuffles the cards in their hand back into
    /// their library, draws a new hand of cards equal to their starting hand size".
    ///
    /// [CR 103.5]: https://yawgatog.com/resources/magic-rules/#R1035
    pub fn shuffle_opening_hand(&mut self, sink: &mut Sink<L>, player: Seat) -> Result<(), Full> {
        self.return_opening_hand(sink, player)?;
        self.shuffle_library(player)
    }

    /// Deals the identities of the cards in `slots`, all in one library, anew among those slots
    /// at random, as cards put there in a random order are: `rebind` with the game's stream
    /// for everything but shuffles.
    pub fn rebind_at_random(&mut self, slots: &[L::Slot]) -> Result<(), Full> {
        let mut rng = self.rng.other;
        self.rebind(slots, &mut rng)?;
        self.rng.other = rng;
        Ok(())
    }

    /// Deals the identities of the cards in `slots`, all in one library, anew among those slots
    /// with `rng`, and every player forgets them: the slots are public handles (architecture §13),
    /// so this is how cards come to be in an order no player knows. Each becomes a new object
    /// ([CR 701.20d]): "If cards in a player's library are shuffled or otherwise reordered, any
    /// revealed cards that are reordered stop being revealed and become new objects."
    ///
    /// [CR 701.20d]: https://yawgatog.com/resources/magic-rules/#R70120d
    pub(crate) fn rebind(&mut self, slots: &[L::Slot], rng: &mut SplitMix64) -> Result<(), Full> {
        let mut rebound = Bitboard::EMPTY;
        let mut cards: PileCards<L> = Default::default();
        for &slot in slots {
            // A library never holds more than its owner's deck slots, or a shared deck's.
            let object = self.object(slot);
            if object.incarnation == u16::MAX || rebound.contains(slot) {
                return Err(Full);
            }
            rebound.insert(slot);
            cards.push_bounded(object.card);
        }
        rng.shuffle(cards.as_mut_slice());
        for (&slot, &card) in slots.iter().zip(cards.iter()) {
            let o = self.object_mut(slot);
            o.card = card;
            o.incarnation += 1;
        }
        self.forget_all_stored(rebound);
        for k in self.knowledge.as_mut_slice() {
            *k &= !rebound;
        }
        Ok(())
    }

    /// A token in a zone other than the battlefield ceases to exist, and its slot returns to the
    /// shared pool (see [CR 704.5d]): "If a token is in a zone other than the battlefield, it ceases to
    /// exist." The incarnation is kept and bumped, so references to the token stay stale after the
    /// slot is reused. The cards exiled with it are exiled with nothing any more.
    ///
    /// [CR 704.5d]: https://yawgatog.com/resources/magic-rules/#R7045d
    pub fn cease_to_exist(&mut self, slot: L::Slot) -> Result<(), Full> {
        let incarnation = self.object(slot).incarnation.checked_add(1).ok_or(Full)?;
        if self.zones.mask == 0 {
            leave::<L, true>(self, slot, Zone::Nowhere);
        } else {
            leave::<L, false>(self, slot, Zone::Nowhere);
        }
        self.forget_stored(slot);
        *self.object_mut(slot) = Object {
            incarnation,
            ..Object::EMPTY
        };
        self.forget_links_to(slot);
        for k in self.knowledge.as_mut_slice() {
            k.remove(slot);
        }
        Ok(())
    }

    /// Returns expired tokens and spell copies to the pool. The removals emit no events,
    /// and each slot gets its own new incarnation ([`State::cease_to_exist`], see CR 704.5d–e).
    pub fn reclaim_tokens(&mut self, tokens: Bitboard<L::Slot>) -> Result<(), Full> {
        for slot in tokens.iter() {
            self.cease_to_exist(slot)?;
        }
        Ok(())
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
                ..o.clone()
            },
            permanent,
            self.stored(slot),
        )
    }

    /// An opponent cannot match a face-down cast with a card they saw in a hand
    /// (see CR 708.5). Forget the slot bindings for that hand, as when it moves an
    /// unrevealed card to a library (see CR 402.3).
    pub fn hide_choice_from_hand(&mut self, moved: L::Slot) {
        let owner = self.object(moved).owner;
        forget_hand(self, owner, moved);
    }

    /// The number of cards in a player's hand.
    pub fn hand_size(&self, player: Seat) -> u32 {
        self.zone(ZoneKey::Of(player, Zone::Hand)).count()
    }
}

/// A card put from its owner's hand into their library unrevealed, as Brainstorm puts two back: the
/// other player sees a card back go and cannot tell which of the hand's cards it was, so they
/// forget it and every card still in that hand, however they came to know them (a look at the hand,
/// a card they saw on top of the library drawn). A hand and a library are hidden zones (see [CR 400.2]:
/// "Library and hand are hidden zones, even if all the cards in one such zone happen to be
/// revealed"), and the owner keeps their hand in any order (see [CR 402.3]: "A player may arrange their
/// hand in any convenient fashion").
///
/// [CR 400.2]: https://yawgatog.com/resources/magic-rules/#R4002
/// [CR 402.3]: https://yawgatog.com/resources/magic-rules/#R4023
fn forget_hand<L: Layout>(s: &mut State<L>, owner: Seat, moved: L::Slot) {
    let hand = s.zones.members[owner.index()][Zone::Hand.index()];
    // The player controlling the owner sees the hand as the owner does (see CR 724.4).
    let sharer = s.sharer(owner);
    for p in Seat::BOTH {
        if p != owner && Some(p) != sharer {
            let k = &mut s.knowledge[p.index()];
            k.remove(moved);
            *k &= !hand;
        }
    }
}

/// Takes the object in `slot` out of its zone's membership and order as it moves to `to`, with
/// `SHARED` saying whether the players share a library and a graveyard.
#[allow(
    clippy::inline_always,
    reason = "inlined into each format's move, which compiles out the other format's code"
)]
#[inline(always)]
fn leave<L: Layout, const SHARED: bool>(s: &mut State<L>, slot: L::Slot, to: Zone) {
    let o = s.object(slot).clone();
    if SHARED && is_shared(o.zone) {
        leave_shared(s, slot, o.zone);
        return;
    }
    let owner = o.owner.index();
    match o.zone {
        Zone::Library => s.zones.library.remove_item(owner, slot),
        Zone::Graveyard => {
            s.zones.graveyard.remove_item(owner, slot);
            s.flags.derived_dirty |= s.layers_read_graveyards();
        }
        Zone::Battlefield => {
            s.leave_battlefield(slot);
            // A phased-out permanent is in no battlefield list (`crate::phasing`).
            s.phased_out.remove(slot);
            s.flags.derived_dirty = true;
            // What was tracked of it this turn ends with it (CR 400.7): the object it
            // returns as is new.
            s.trackers.forget(slot);
            // See [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A permanent is
            // removed from combat if it leaves the battlefield". The creatures attacking it or
            // blocked by it stay in combat, related to nothing; the slot may soon hold
            // another object. What was attached to it is attached to nothing: the object it
            // returns as is new ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)),
            // and an Aura left unattached goes to the graveyard as a state-based action
            // (see CR 704.5m).
            for i in 0..s.battlefield.len() {
                let other = s.object_mut(s.battlefield.as_slice()[i]);
                if !matches!(other.combat, CombatRole::None) {
                    other.combat.forget(slot);
                }
                if other.attached_to == Some(slot) {
                    other.attached_to = None;
                }
            }
            // What phased out directly while attached to it phases in unattached
            // (see [CR 702.26i](https://yawgatog.com/resources/magic-rules/#R70226i)): "if that
            // object is still in the same zone ... If not, that Aura, Equipment, or
            // Fortification phases in unattached."
            if !s.phased_out.is_empty() {
                for other in s.phased_out.iter() {
                    let other = s.object_mut(other);
                    if other.attached_to == Some(slot) {
                        other.attached_to = None;
                    }
                }
            }
        }
        Zone::Stack => s.zones.stack.remove(slot),
        Zone::Hand => s.flags.derived_dirty |= s.layers_read_hands(),
        Zone::Exile => {
            if to != Zone::Stack && o.is_token() {
                s.detach_prepare_copy(slot);
            }
            s.flags.derived_dirty |= s.layers_read_linked();
        }
        Zone::Nowhere | Zone::Merged => {}
    }
    if (o.zone.index()) < Zone::OWNED {
        s.zones.members[owner][o.zone.index()].remove(slot);
    }
}

/// Takes a card out of the shared library or graveyard, `zone`, and out of both players' sets
/// of it.
#[cold]
#[inline(never)]
fn leave_shared<L: Layout>(s: &mut State<L>, slot: L::Slot, zone: Zone) {
    if zone == Zone::Library {
        s.zones.library.remove_item(0, slot);
    } else {
        s.zones.graveyard.remove_item(0, slot);
        s.flags.derived_dirty |= s.layers_read_graveyards();
    }
    for members in s.zones.members.as_mut_slice() {
        members[zone.index()].remove(slot);
    }
}

/// Puts a card into the shared library, at `place`, or on top of the shared graveyard, `to`,
/// and into both players' sets of it.
#[cold]
#[inline(never)]
fn enter_shared<L: Layout>(
    s: &mut State<L>,
    slot: L::Slot,
    to: Zone,
    place: Place,
) -> Result<(), Full> {
    if to == Zone::Library {
        place_in_shared_library(&mut s.zones.library, slot, place)?;
    } else {
        s.zones.graveyard.push_shared(slot)?;
        s.flags.derived_dirty |= s.layers_read_graveyards();
    }
    for members in s.zones.members.as_mut_slice() {
        members[to.index()].insert(slot);
    }
    Ok(())
}

/// Where `place` is in a library of `len` cards.
#[inline]
fn library_index(len: usize, place: Place) -> usize {
    match place {
        Place::Top => len,
        Place::Bottom => 0,
        // Putting a card under more cards than the zone holds places it at the bottom (index 0).
        Place::UnderTop(n) => len.saturating_sub(usize::from(n)),
    }
}

#[inline]
fn place_in_library<S: SlotId>(
    piles: &mut impl Piles<S>,
    list: usize,
    slot: S,
    place: Place,
) -> Result<(), Full> {
    match place {
        Place::Top => piles.push(list, slot),
        Place::Bottom | Place::UnderTop(_) => {
            piles.insert(list, library_index(piles.list(list).len(), place), slot)
        }
    }
}

fn place_in_shared_library<S: SlotId>(
    piles: &mut impl Piles<S>,
    slot: S,
    place: Place,
) -> Result<(), Full> {
    piles.insert_shared(library_index(piles.list(0).len(), place), slot)
}

#[cfg(test)]
mod tests;
