//! How a card body lets a player cast a card they otherwise could not: a permission, an
//! entry of the Effects store on the card (`beryl_kernel`'s contract, "Stores"), which
//! [`crate::permission`] reads as spells are offered and cast.

use super::EffectCtx;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Duration, MoveReplacement, ObjectRef, Payload, Permission};

impl<L: Layout> EffectCtx<'_, L> {
    /// Lets `player` cast the card in `object`'s slot from the zone it is in, for as long as
    /// it stays there, or until end of turn with `this_turn`
    /// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin
    /// to cast a spell only if a rule or effect allows that player to cast it". A spell cast
    /// this way costs `extra` generic mana more, and with `exile` it is exiled rather than
    /// put into its owner's graveyard.
    pub fn allow_cast(
        &mut self,
        object: ObjectRef<L>,
        player: Seat,
        extra: u8,
        this_turn: bool,
        exile: bool,
    ) {
        let terms = Permission {
            player,
            increase: extra,
            replacement: exile.then_some(MoveReplacement::ExileInsteadOfGraveyard),
        };
        // It lasts while the card stays where it is, which is no duration: the entry ends
        // with the card's object.
        let duration = if this_turn {
            Duration::EndOfTurn
        } else {
            Duration::Indefinite
        };
        self.permit(object, terms, duration);
    }

    /// Starts a permission to cast the card `object`, if it is still that object.
    fn permit(&mut self, object: ObjectRef<L>, terms: Permission, duration: Duration) {
        if !self.game.state.is_current(object) {
            return;
        }
        let source = self.resolution.entry.card;
        let r = self
            .game
            .state
            .start_effect(source, object.into(), Payload::Cast(terms), duration);
        self.checked(r);
    }

    /// The card in `object`'s slot gains flashback until end of turn, its flashback cost
    /// equal to its mana cost, for `player`
    /// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "'Flashback
    /// [cost]' means 'You may cast this card from your graveyard if the resulting spell is an
    /// instant or sorcery spell by paying [cost] rather than paying its mana cost' and 'If the
    /// flashback cost was paid, exile this card instead of putting it anywhere else any time
    /// it would leave the stack.'" Only its owner can cast it from their graveyard, so
    /// `player` is its owner.
    pub fn grant_flashback(&mut self, object: ObjectRef<L>, player: Seat) {
        let terms = Permission {
            player,
            increase: 0,
            replacement: Some(MoveReplacement::Flashback),
        };
        self.permit(object, terms, Duration::EndOfTurn);
    }
}
