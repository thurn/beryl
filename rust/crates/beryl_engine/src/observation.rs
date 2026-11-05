//! `Observation`: a borrowed view of the state for one player, masking what they do not know
//! (architecture §13, decision D9).

use crate::bitboard::Bitboard;
use crate::fixed::Fixed;
use crate::rng::{Rng, SplitMix64};
use crate::state::{
    DECK_SLOTS, Derived, NONE, Object, PendingTrigger, Player, StackEntry, State, Turn, Zone,
};
use crate::table::GameTable;

/// What [`Observation::card`] reads for a slot whose identity the viewer does not know.
pub const UNKNOWN: u8 = u8::MAX;

/// The state as one player sees it. Slots are public handles; what is hidden is which card
/// sits behind a slot the viewer has not seen. Agents, rollout policies and the UI read the
/// game only through this.
#[derive(Clone, Copy)]
pub struct Observation<'a> {
    state: &'a State,
    viewer: u8,
}

impl<'a> Observation<'a> {
    pub fn new(state: &'a State, viewer: u8) -> Self {
        Self { state, viewer }
    }

    pub fn viewer(&self) -> u8 {
        self.viewer
    }

    pub fn knows(&self, slot: u8) -> bool {
        self.state.knows(self.viewer, slot)
    }

    /// The game index of the card in a slot, or [`UNKNOWN`].
    pub fn card(&self, slot: u8) -> u8 {
        if self.knows(slot) {
            self.state.object(slot).card
        } else {
            UNKNOWN
        }
    }

    /// The object in a slot, with every field that could betray an unknown identity masked.
    pub fn object(&self, slot: u8) -> Object {
        let mut o = *self.state.object(slot);
        if !self.knows(slot) {
            o.card = UNKNOWN;
            o.copy_of = NONE;
            o.face = 0;
            o.chosen = NONE;
        }
        o
    }

    pub fn player(&self, player: u8) -> &'a Player {
        self.state.player(player)
    }

    pub fn turn(&self) -> &'a Turn {
        self.state.turn()
    }

    /// The slots a player owns in a zone: public, though the identities may not be.
    pub fn zone(&self, owner: u8, zone: Zone) -> Bitboard {
        self.state.zone(owner, zone)
    }

    /// A player's library as slots, bottom first.
    pub fn library(&self, player: u8) -> &'a [u8] {
        self.state.library(player)
    }

    /// A player's graveyard as slots, bottom first.
    pub fn graveyard(&self, player: u8) -> &'a [u8] {
        self.state.graveyard(player)
    }

    pub fn battlefield(&self) -> &'a [u8] {
        self.state.battlefield()
    }

    /// A permanent's characteristics after continuous effects, public as the battlefield is
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)).
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield.
    pub fn derived(&self, slot: u8) -> &'a Derived {
        self.state.derived(slot)
    }

    /// The stack, bottom first. It is a public zone (CR 400.2), and a spell is revealed as it
    /// is cast ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a)): "a player
    /// first moves that card ... from where it is to the stack".
    pub fn stack(&self) -> &'a [StackEntry] {
        self.state.stack()
    }

    /// The triggered abilities waiting to be put on the stack. Each names its public source;
    /// [`crate::DecisionKind::TriggerOrder`] options index this list.
    pub fn triggers(&self) -> &'a [PendingTrigger] {
        self.state.triggers()
    }

    /// A full state consistent with everything the viewer knows: the unknown remainder of
    /// each decklist, dealt at random into that player's slots the viewer does not know
    /// (architecture §13). Decklists are public (decision D19), so the remainder is exact.
    /// The cards the viewer knows, every public fact and the slot order of every zone are
    /// kept; the game's random streams are reseeded from `rng`, so its seed does not leak.
    pub fn determinize(&self, t: &GameTable, rng: &mut SplitMix64) -> State {
        let mut s = *self.state;
        for player in 0..2u8 {
            let deck = t.deck(player);
            let mut remainder: Fixed<u8, DECK_SLOTS> = Fixed::new();
            for &card in deck {
                // A deck never holds more than DECK_SLOTS cards (GameTable::new).
                let _ = remainder.push(card);
            }
            let mut unknown: Fixed<u8, DECK_SLOTS> = Fixed::new();
            for i in 0..deck.len() {
                let slot = (usize::from(player) * DECK_SLOTS + i) as u8;
                if self.knows(slot) {
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
        s.rng = Rng::new(rng.next_u64());
        s
    }
}
