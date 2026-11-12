//! Evaluating a [`Filter`], the predicates targets and affected sets are written in
//! (architecture §6.2), against a game state.

pub use beryl_card::{Filter, Whose};

use crate::bitboard::Bitboard;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{ObjectFlags, State, Zone};
use crate::table::GameTable;

/// What a [`Filter`] accepts in a game. Evaluating one is a pass over a candidate set; the
/// hot ones become ANDs over per-slot attribute bitboards as the cards that need them arrive.
///
/// The filter is vocabulary (`beryl_card`), which knows nothing of the state, so its meaning
/// is this trait: bring it into scope to evaluate one.
pub trait Evaluate {
    /// Whether the object in `slot` passes, evaluated for player `you`, where X is 0.
    fn matches<L: Layout>(&self, s: &State<L>, t: &GameTable, slot: L::Slot, you: Seat) -> bool {
        self.matches_x(s, t, slot, you, 0)
    }

    /// Whether the object in `slot` passes, evaluated for player `you` and the value `x` of
    /// X.
    fn matches_x<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        slot: L::Slot,
        you: Seat,
        x: u8,
    ) -> bool;

    /// The members of `within` that pass, where X is 0.
    fn select<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        within: Bitboard<L::Slot>,
        you: Seat,
    ) -> Bitboard<L::Slot> {
        self.select_x(s, t, within, you, 0)
    }

    /// The members of `within` that pass, for the value `x` of X.
    fn select_x<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        within: Bitboard<L::Slot>,
        you: Seat,
        x: u8,
    ) -> Bitboard<L::Slot> {
        within
            .iter()
            .filter(|&slot| self.matches_x(s, t, slot, you, x))
            .collect()
    }
}

impl Evaluate for Filter {
    fn matches_x<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        slot: L::Slot,
        you: Seat,
        x: u8,
    ) -> bool {
        let o = s.object(slot);
        let face = || s.copiable(slot).face(t);
        let derived = || (o.zone == Zone::Battlefield).then(|| s.derived(slot));
        match *self {
            Filter::Any => true,
            Filter::Types(types) => derived()
                .map_or(face().types, |d| d.types)
                .intersects(types),
            Filter::Supertypes(supertypes) => derived()
                .map_or(face().supertypes, |d| d.supertypes)
                .intersects(supertypes),
            Filter::Subtype(subtype) => derived()
                .map_or(face().subtypes, |d| d.subtypes)
                .contains(subtype),
            // CR 108.4a: a card that is no permanent or spell has no controller, and "its
            // owner" is used instead, which is what the query answers.
            Filter::Whose(whose) => is_whose(whose, s.controller(slot), you),
            Filter::Tapped => o.flags.has(ObjectFlags::TAPPED),
            Filter::ManaValueAtMost(n) => s.mana_value(t, slot) <= n,
            Filter::ManaValueAtMostX => s.mana_value(t, slot) <= x,
            Filter::Not(f) => !f.matches_x(s, t, slot, you, x),
            Filter::All(fs) => fs.iter().all(|f| f.matches_x(s, t, slot, you, x)),
            Filter::AnyOf(fs) => fs.iter().any(|f| f.matches_x(s, t, slot, you, x)),
        }
    }
}

/// Whether `player` is the one `whose` means, read for `you`. Every reading of a [`Whose`]
/// comes here: the controller or owner of an object ([`Filter::Whose`]) and a player a
/// target may choose.
#[inline]
pub fn is_whose(whose: Whose, player: Seat, you: Seat) -> bool {
    match whose {
        // CR 109.5: "The words 'you' and 'your' on an object refer to the object's
        // controller".
        Whose::You => player == you,
        Whose::AnOpponent => is_an_opponent(player, you),
    }
}

/// [`Whose::AnOpponent`]: whether `player` is one of `you`'s opponents
/// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022)): "In a two-player game, a
/// player's opponent is the other player."
///
/// The one place that entry is read, and [`crate::State::opponents_of`] is the set it tests.
/// With more players and no teams every other player is an opponent, as written here.
/// Between teams, "the player's opponents are all players not on their team"
/// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)): this function changes
/// then, and no card does.
#[inline]
pub fn is_an_opponent(player: Seat, you: Seat) -> bool {
    player != you
}
