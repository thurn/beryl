//! `Observation`: a borrowed view of the state for one player, masking what they do not know
//! (architecture §13, decision D9).

use crate::bitboard::Bitboard;
use crate::card::FaceIx;
use crate::filter::Filter;
use crate::fixed::Fixed;
use crate::ids::{CardIx, Seat, Slot};
use crate::rng::SplitMix64;
use crate::state::{
    CastPermission, DECK_SLOTS, Derived, Object, PendingTrigger, Player, StackEntry, State, Turn,
    Zone, deck_slot,
};
use crate::table::GameTable;

/// The state as one player sees it. Slots are public handles; what is hidden is which card
/// sits behind a slot the viewer has not seen. Agents, rollout policies and the UI read the
/// game only through this.
///
/// While a resolution is paused (§7.3) the position shown is not the committed one: the body
/// has run in scratch up to its question ([`crate::view`]). The view keeps the committed
/// state as its root, the position search plays on from; [`crate::observe`] builds such a
/// view.
#[derive(Clone, Copy)]
pub struct Observation<'a> {
    state: &'a State,
    root: &'a State,
    viewer: Seat,
}

impl<'a> Observation<'a> {
    /// The view of a committed state, which is its own root.
    pub fn new(state: &'a State, viewer: Seat) -> Self {
        Self {
            state,
            root: state,
            viewer,
        }
    }

    /// The view of `shown`, the position a paused resolution's question is asked in, whose
    /// committed state is `root`.
    pub(crate) fn paused(shown: &'a State, root: &'a State, viewer: Seat) -> Self {
        Self {
            state: shown,
            root,
            viewer,
        }
    }

    pub fn viewer(&self) -> Seat {
        self.viewer
    }

    pub fn knows(&self, slot: Slot) -> bool {
        self.state.knows(self.viewer, slot)
    }

    /// The game index of the card in a slot, if the viewer knows it.
    pub fn card(&self, slot: Slot) -> Option<CardIx> {
        self.knows(slot).then(|| self.state.card(slot))
    }

    /// The object in a slot, with every field that could betray an unknown identity masked:
    /// its card, which only [`Observation::card`] reads, its copy and the face up.
    pub fn object(&self, slot: Slot) -> Object {
        let mut o = *self.state.object(slot);
        if !self.knows(slot) {
            o.card = CardIx::default();
            o.copy_of = None;
            o.face = FaceIx::FRONT;
        }
        o
    }

    /// Who controls an object: for a permanent the layer-2 result, otherwise its base
    /// controller ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)). Public:
    /// control is never hidden.
    pub fn controller(&self, slot: Slot) -> Seat {
        self.state.controller(slot)
    }

    pub fn player(&self, player: Seat) -> &'a Player {
        self.state.player(player)
    }

    pub fn turn(&self) -> &'a Turn {
        self.state.turn()
    }

    /// The slots a player owns in a zone: public, though the identities may not be.
    pub fn zone(&self, owner: Seat, zone: Zone) -> Bitboard {
        self.state.zone(owner, zone)
    }

    /// A player's library as slots, bottom first.
    pub fn library(&self, player: Seat) -> &'a [Slot] {
        self.state.library(player)
    }

    /// A player's graveyard as slots, bottom first.
    pub fn graveyard(&self, player: Seat) -> &'a [Slot] {
        self.state.graveyard(player)
    }

    pub fn battlefield(&self) -> &'a [Slot] {
        self.state.battlefield()
    }

    /// A permanent's characteristics after continuous effects, public as the battlefield is
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)).
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield.
    pub fn derived(&self, slot: Slot) -> &'a Derived {
        self.state.derived(slot)
    }

    /// The stack, bottom first. It is a public zone (CR 400.2), and a spell is revealed as it
    /// is cast ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a)): "a player
    /// first moves that card ... from where it is to the stack".
    pub fn stack(&self) -> &'a [StackEntry] {
        self.state.stack()
    }

    /// Who may cast which card from a zone it could not otherwise be cast from, and for how
    /// much more: public, as the effects that grant them are. Elite Spellbinder's exiled card
    /// is face up ([CR 406.3](https://yawgatog.com/resources/magic-rules/#R4063): "Exiled cards
    /// are, by default, kept face up and may be examined by any player at any time").
    pub fn permissions(&self) -> &'a [CastPermission] {
        self.state.permissions()
    }

    /// The triggered abilities waiting to be put on the stack. Each names its public source;
    /// [`crate::DecisionKind::TriggerOrder`] options index this list.
    pub fn triggers(&self) -> &'a [PendingTrigger] {
        self.state.triggers()
    }

    /// Whether the object in `slot` passes `filter`, evaluated for the viewer: always false
    /// for an object the viewer does not know, whose characteristics would betray it.
    pub fn matches(&self, t: &GameTable, filter: &Filter, slot: Slot) -> bool {
        self.knows(slot) && filter.matches(self.state, t, slot, self.viewer)
    }

    /// The committed state behind the view, unmasked. Reading it is cheating: only the
    /// omniscient search, which is labeled as cheating wherever a human meets it, does
    /// (architecture §19.1). The information-independence test holds every other agent to the
    /// masked accessors.
    pub fn unmasked_root(&self) -> &'a State {
        self.root
    }

    /// A full committed state consistent with everything the viewer knows there: the root,
    /// with the unknown remainder of each decklist dealt at random into that player's slots
    /// the viewer does not know (architecture §13). Decklists are public (decision D19), so
    /// the remainder is exact. The cards the viewer knows, every public fact and the slot
    /// order of every zone are kept; the game's random streams are reseeded from `rng`, so its
    /// seed does not leak. A paused resolution keeps what it showed the viewer known in the
    /// committed state, so it replays from here to the same question.
    pub fn determinize(&self, t: &GameTable, rng: &mut SplitMix64) -> State {
        let mut s = *self.root;
        for player in Seat::BOTH {
            let deck = t.deck(player);
            let mut remainder: Fixed<CardIx, DECK_SLOTS> = Fixed::new();
            for &card in deck {
                // A deck never holds more than DECK_SLOTS cards (GameTable::new).
                let _ = remainder.push(card);
            }
            let mut unknown: Fixed<Slot, DECK_SLOTS> = Fixed::new();
            for i in 0..deck.len() {
                let slot = deck_slot(player, i);
                if self.root.knows(self.viewer, slot) {
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
