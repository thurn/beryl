//! What each player knows: looking at, revealing and forgetting cards (architecture §13;
//! the crate's contract, "Mutators"). A card a player is about to learn has its identity
//! settled first ([`chance`]), so a save replays what was seen.
//!
//! [CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002): "Public zones are zones in
//! which all players can see the cards' faces ... Hidden zones are zones in which not all
//! players can be expected to see the cards' faces." The zone moves keep what a zone tells
//! every player ([`crate::zone`]); these are the other ways a player learns or forgets a card.

use crate::bitboard::Bitboard;
use crate::chance;
use crate::event::Sink;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::State;

impl<L: Layout> State<L> {
    /// Reveals a card to every player
    /// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
    /// show that card to all players for a brief time." Each player knows it from then on,
    /// until it is shuffled or otherwise hidden anew.
    pub fn reveal(&mut self, sink: &mut Sink<L>, slot: L::Slot) {
        chance::settle(self, sink, slot);
        for k in &mut self.knowledge {
            k.insert(slot);
        }
    }

    /// `player` looks at the cards in `cards` and from then on knows them: a library and a
    /// hand are hidden zones
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)), "zones in which not
    /// all players can be expected to see the cards' faces."
    pub fn look(&mut self, sink: &mut Sink<L>, player: Seat, cards: Bitboard<L::Slot>) {
        chance::settle_all(self, sink, cards);
        self.knowledge[player.index()] |= cards;
    }

    /// `player` no longer knows the card in `slot`, as when a permanent they knew enters
    /// face down under another player's control
    /// ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "You can't look at
    /// face-down cards in any other zone or face-down spells or permanents controlled by
    /// another player."
    pub fn forget(&mut self, player: Seat, slot: L::Slot) {
        self.knowledge[player.index()].remove(slot);
    }

    /// Gives each player back what they knew at an earlier moment ([`State::knowledge`]),
    /// as a paused resolution run again from its start needs. Returns the cards some player
    /// knew beyond that: what the earlier runs revealed, which chance must not count as
    /// revealed again ([`chance`]).
    pub fn restore_knowledge(&mut self, knowledge: [Bitboard<L::Slot>; 2]) -> Bitboard<L::Slot> {
        let revealed = (self.knowledge[0] | self.knowledge[1]) & !(knowledge[0] | knowledge[1]);
        self.knowledge = knowledge;
        revealed
    }

    /// What `run`, a copy of this state played on, showed a player stays known here
    /// (architecture §7.3): the cards a paused resolution revealed before its question.
    ///
    /// A slot keeps its identity through the moves of a run, so a card seen there is the
    /// card this state holds in that slot, unless something random dealt identities anew (a
    /// shuffle, a random order: `random`); then only a slot that is still the same object
    /// counts.
    pub fn keep_seen(&mut self, run: &State<L>, random: bool) {
        for p in Seat::BOTH {
            let gained = run.knowledge[p.index()] & !self.knowledge[p.index()];
            for slot in gained.iter() {
                let (now, then) = (self.object(slot), run.object(slot));
                if now.card == then.card && (!random || now.incarnation == then.incarnation) {
                    self.knowledge[p.index()].insert(slot);
                }
            }
        }
    }
}
