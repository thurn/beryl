//! `Observation`: a borrowed view of the state for one player, masking what they do not know
//! (architecture §13, decision D9).

use crate::bitboard::Bitboard;
use crate::state::{NONE, Object, Player, State, Turn, Zone};

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
}
