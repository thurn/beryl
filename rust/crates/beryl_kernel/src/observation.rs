//! `Observation`: a borrowed view of the state for one player, masking what they do not know
//! (architecture §13, decision D9).

use crate::bitboard::{Bitboard, Slots};
use crate::filter::{Evaluate, Filter};
use crate::ids::{CardIx, Seat, SlotId};
use crate::layout::{Layout, Queue, Small};
use crate::rng::SplitMix64;
use crate::state::{
    CastPermission, Derived, Object, Pending, PendingTrigger, Player, StackEntry, State, Turn,
    Zone, ZoneKey,
};
use crate::table::GameTable;
use beryl_card::FaceIx;

/// The state as one player sees it. Slots are public handles; what is hidden is which card
/// sits behind a slot the viewer has not seen. Agents, rollout policies and the UI read the
/// game only through this.
///
/// While a resolution is paused (§7.3) the position shown is not the committed one: the body
/// has run in scratch up to its question (`beryl_engine::view`). The view keeps the committed
/// state as its root, the position search plays on from; `beryl_engine::observe` builds such a
/// view.
#[derive(Clone, Copy)]
pub struct Observation<'a, L: Layout = Small> {
    state: &'a State<L>,
    root: &'a State<L>,
    viewer: Seat,
}

impl<'a, L: Layout> Observation<'a, L> {
    /// The view of a committed state, which is its own root.
    pub fn new(state: &'a State<L>, viewer: Seat) -> Self {
        Self {
            state,
            root: state,
            viewer,
        }
    }

    /// The view of `shown`, the position a paused resolution's question is asked in, whose
    /// committed state is `root`.
    pub fn paused(shown: &'a State<L>, root: &'a State<L>, viewer: Seat) -> Self {
        Self {
            state: shown,
            root,
            viewer,
        }
    }

    /// The position shown, for the engine's own reads of public facts about it.
    pub fn shown(&self) -> &'a State<L> {
        self.state
    }

    pub fn viewer(&self) -> Seat {
        self.viewer
    }

    pub fn knows(&self, slot: L::Slot) -> bool {
        self.state.knows(self.viewer, slot)
    }

    /// The game index of the card in a slot, if the viewer knows it.
    pub fn card(&self, slot: L::Slot) -> Option<CardIx> {
        self.knows(slot).then(|| self.state.card(slot))
    }

    /// The object in a slot, with every field that could betray an unknown identity masked:
    /// its card, which only [`Observation::card`] reads, and the face up. Whether a
    /// permanent is face down is public: it is part of its status
    /// ([CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105)).
    pub fn object(&self, slot: L::Slot) -> Object<L> {
        let mut o = *self.state.object(slot);
        if !self.knows(slot) {
            o.card = CardIx::default();
            if o.face != FaceIx::DOWN {
                o.face = FaceIx::FRONT;
            }
        }
        o
    }

    /// Who controls an object: for a permanent the layer-2 result, otherwise its base
    /// controller ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)). Public:
    /// control is never hidden.
    pub fn controller(&self, slot: L::Slot) -> Seat {
        self.state.controller(slot)
    }

    pub fn player(&self, player: Seat) -> &'a Player<L> {
        self.state.player(player)
    }

    pub fn turn(&self) -> &'a Turn {
        self.state.turn()
    }

    /// The player chosen to take the first turn; none while the coin flip's winner chooses.
    /// [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031): "the players determine
    /// which one of them will choose who takes the first turn ... The player chosen to take
    /// the first turn is the starting player."
    pub fn starting_player(&self) -> Option<Seat> {
        match self.state.pending() {
            Pending::PlayOrDraw { .. } => None,
            _ => Some(self.state.turn().starting_player),
        }
    }

    /// The objects in the zone `key` names: public, though the identities may not be.
    pub fn zone(&self, key: ZoneKey) -> Bitboard<L::Slot> {
        self.state.zone(key)
    }

    /// `objects` by ascending slot, the order a decision lists its candidates in and no order
    /// of the rules ([`State::in_candidate_order`]).
    #[inline]
    pub fn in_candidate_order(&self, objects: Bitboard<L::Slot>) -> Slots<L::Slot> {
        objects.iter()
    }

    /// A player's library as slots, bottom first.
    pub fn library(&self, player: Seat) -> &'a [L::Slot] {
        self.state.library(player)
    }

    /// A player's graveyard as slots, bottom first.
    pub fn graveyard(&self, player: Seat) -> &'a [L::Slot] {
        self.state.graveyard(player)
    }

    pub fn battlefield(&self) -> &'a [L::Slot] {
        self.state.battlefield()
    }

    /// A permanent's characteristics after continuous effects, public as the battlefield is
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)).
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield.
    pub fn derived(&self, slot: L::Slot) -> &'a Derived {
        self.state.derived(slot)
    }

    /// The stack, bottom first. It is a public zone (CR 400.2), and a spell is revealed as it
    /// is cast ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a)): "a player
    /// first moves that card ... from where it is to the stack".
    pub fn stack(&self) -> &'a [StackEntry<L>] {
        self.state.stack()
    }

    /// Who may cast which card from a zone it could not otherwise be cast from, and for how
    /// much more: public, as the effects that grant them are. Elite Spellbinder's exiled card
    /// is face up ([CR 406.3](https://yawgatog.com/resources/magic-rules/#R4063): "Exiled cards
    /// are, by default, kept face up and may be examined by any player at any time").
    pub fn permissions(&self) -> &'a [CastPermission<L>] {
        self.state.permissions()
    }

    /// The triggered abilities waiting to be put on the stack. Each names its public source;
    /// [`crate::decision::DecisionKind::TriggerOrder`] options index this list.
    pub fn triggers(&self) -> &'a [PendingTrigger<L>] {
        self.state.triggers()
    }

    /// Whether the object in `slot` passes `filter`, evaluated for the viewer: always false
    /// for an object the viewer does not know, whose characteristics would betray it.
    pub fn matches(&self, t: &GameTable, filter: &Filter, slot: L::Slot) -> bool {
        self.knows(slot) && filter.matches(self.state, t, slot, self.viewer)
    }

    /// The committed state behind the view, unmasked. Reading it is cheating: only the
    /// omniscient search, which is labeled as cheating wherever a human meets it, does
    /// (architecture §19.1). The information-independence test holds every other agent to the
    /// masked accessors.
    pub fn unmasked_root(&self) -> &'a State<L> {
        self.root
    }

    /// A full committed state consistent with everything the viewer knows there: the root,
    /// with the unknown remainder of each decklist dealt at random into that player's slots
    /// the viewer does not know (architecture §13). Decklists are public (decision D19), so
    /// the remainder is exact. The cards the viewer knows, every public fact and the slot
    /// order of every zone are kept; the game's random streams are reseeded from `rng`, so its
    /// seed does not leak. A paused resolution keeps what it showed the viewer known in the
    /// committed state, so it replays from here to the same question.
    pub fn determinize(&self, t: &GameTable, rng: &mut SplitMix64) -> State<L> {
        self.deal(t, rng, |_| false)
    }

    /// [`Self::determinize`] with every hand shown: only the libraries' unknown cards, and
    /// any other card the viewer does not know outside a hand, are dealt at random. This is
    /// half-cheating, for measuring what seeing the opponent's hand is worth: a hand is a
    /// hidden zone ([CR 402.3](https://yawgatog.com/resources/magic-rules/#R4023): "A player
    /// may arrange their hand in any convenient fashion and look at it as much as they
    /// want. A player can't look at the cards in another player's hand but can see how many
    /// cards are in that hand at any time."), and so is a library, whose order no player
    /// knows ([CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Each library
    /// must be kept in a single face-down pile. Players can't look at or change the order of
    /// cards in a library.").
    pub fn determinize_libraries(&self, t: &GameTable, rng: &mut SplitMix64) -> State<L> {
        let root = self.root;
        self.deal(t, rng, |slot| root.object(slot).zone == Zone::Hand)
    }

    /// The root with each player's cards the viewer does not know, `shown` ones aside, dealt
    /// at random from the rest of their decklist.
    fn deal(
        &self,
        t: &GameTable,
        rng: &mut SplitMix64,
        shown: impl Fn(L::Slot) -> bool,
    ) -> State<L> {
        let mut s = self.root.clone();
        for player in Seat::BOTH {
            let deck = t.deck(player);
            let mut remainder: L::Deck<CardIx> = Default::default();
            for &card in deck {
                // A deck never holds more cards than the layout has slots for (State::deal).
                let _ = remainder.push(card);
            }
            let mut unknown: L::Deck<L::Slot> = Default::default();
            for i in 0..deck.len() {
                let slot = L::Slot::deck(player, i);
                if self.root.knows(self.viewer, slot) || shown(slot) {
                    let card = s.object(slot).card;
                    let at = remainder
                        .position(card)
                        .expect("a known card not in its deck");
                    remainder.remove(at);
                } else {
                    let _ = unknown.push(slot);
                }
            }
            rng.shuffle(remainder.as_mut_slice());
            for (&slot, &card) in unknown.iter().zip(remainder.iter()) {
                s.object_mut(slot).card = card;
            }
        }
        s.reseed(rng.next_u64());
        s
    }
}
