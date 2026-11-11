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
        let face = || t.def(o.card).face(o.face);
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
            Filter::Whose(whose) => {
                let player = if matches!(o.zone, Zone::Battlefield | Zone::Stack) {
                    s.controller(slot)
                } else {
                    o.owner
                };
                is_whose(whose, player, you)
            }
            Filter::Tapped => o.flags.has(ObjectFlags::TAPPED),
            Filter::ManaValueAtMost(n) => mana_value(s, t, slot) <= n,
            Filter::ManaValueAtMostX => mana_value(s, t, slot) <= x,
            Filter::Not(f) => !f.matches_x(s, t, slot, you, x),
            Filter::All(fs) => fs.iter().all(|f| f.matches_x(s, t, slot, you, x)),
            Filter::AnyOf(fs) => fs.iter().any(|f| f.matches_x(s, t, slot, you, x)),
        }
    }
}

/// Whether `player` is the one `whose` means, read for `you`. Every reading of a [`Whose`]
/// comes here: the controller or owner of an object ([`Filter::Whose`]), a player a target
/// may choose, and the player whose step begins.
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
/// The one place that entry meets the two-player game. With more players, "the player's
/// opponents are all players not on their team"
/// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)): this function changes
/// then, and no card does.
#[inline]
pub fn is_an_opponent(player: Seat, you: Seat) -> bool {
    player == you.opponent()
}

/// The mana value of the object in `slot`, without the X of a spell
/// ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)): "The mana value of an
/// object is a number equal to the total amount of mana in its mana cost, regardless of
/// color."
///
/// [CR 712.8e](https://yawgatog.com/resources/magic-rules/#R7128e): a transforming
/// double-faced permanent with its back face up "has only the characteristics of its back
/// face. However, its mana value is calculated using the mana cost of its front face. If a
/// permanent is copying the back face of a nonmodal double-faced permanent (even if the
/// object representing that copy is itself a double-faced permanent), the mana value of that
/// permanent is 0." A token with a double-faced card's faces is always such a copy (CR
/// 707.8a): Fractured Identity on a transformed Jace.
#[inline]
pub fn mana_value<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> u8 {
    let o = s.object(slot);
    let up = t.def(o.card).face(o.face);
    if !up.transformed {
        up.cost.mana_value()
    } else if o.flags.has(ObjectFlags::TOKEN) {
        0
    } else {
        t.def(o.card).front().cost.mana_value()
    }
}
