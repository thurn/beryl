//! Evaluating a [`Filter`], the predicates targets and affected sets are written in
//! (architecture §6.2), against a game state.

pub use beryl_card::Filter;
use beryl_card::{CardDef, Players};

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
        // A permanent's characteristics are the layers' results; its face is read only for
        // an object that is none.
        match *self {
            Filter::Any => true,
            Filter::Types(types) => derived()
                .map_or_else(|| face().types, |d| d.types)
                .intersects(types),
            Filter::Supertypes(supertypes) => derived()
                .map_or_else(|| face().supertypes, |d| d.supertypes)
                .intersects(supertypes),
            Filter::Subtype(subtype) => derived()
                .map_or_else(|| face().subtypes, |d| d.subtypes)
                .contains(subtype),
            Filter::Colors(colors) => derived()
                .map_or_else(|| face().colors, |d| d.colors)
                .intersects(colors),
            // CR 108.4a: a card that is no permanent or spell has no controller, and "its
            // owner" is used instead, which is what the query answers.
            Filter::Whose(players) => is_for(players, s.controller(slot), you),
            Filter::Tapped => o.flags.has(ObjectFlags::TAPPED),
            Filter::ManaValueAtMost(n) => s.mana_value(t, slot) <= n,
            Filter::ManaValueAtMostX => s.mana_value(t, slot) <= x,
            Filter::Not(f) => !f.matches_x(s, t, slot, you, x),
            Filter::All(fs) => fs.iter().all(|f| f.matches_x(s, t, slot, you, x)),
            Filter::AnyOf(fs) => fs.iter().any(|f| f.matches_x(s, t, slot, you, x)),
        }
    }
}

/// Whether the engine reads `players` where no spell or ability is resolving: in a filter, a
/// trigger or a rule a static ability changes. It reads "you", "each opponent" and "each
/// player", which a player and the reader's controller settle. The rest need a resolution
/// (a target, "that player") or an object to ask (its controller, its owner), and are read by
/// the interpreter alone; a game's card table is built only from cards that say none of them
/// here ([`crate::table::TableError::Players`], [`crate::listener::Unmatched::Whose`]).
#[inline]
pub const fn reads(players: &Players) -> bool {
    matches!(
        players,
        Players::You | Players::EachOpponent | Players::EachPlayer
    )
}

/// Whether `player` is one of the players `players` names, read for `you`. Every reading of
/// a [`Players`] outside a resolution comes here: the controller or owner of an object
/// ([`Filter::Whose`]), a player a target may choose, the player of a trigger's event and a
/// player a rule is changed for.
///
/// # Panics
///
/// For players [`reads`] refuses, which nothing in a game's table says here.
#[inline]
pub fn is_for(players: &Players, player: Seat, you: Seat) -> bool {
    match players {
        // CR 109.5: "The words 'you' and 'your' on an object refer to the object's
        // controller".
        Players::You => player == you,
        Players::EachOpponent => is_an_opponent(player, you),
        // CR 102.1: "A player is one of the people in the game."
        Players::EachPlayer => true,
        _ => unreachable!("the card table refuses these players outside a resolution"),
    }
}

/// Whether every filter of a card names only players the engine reads there ([`reads`]):
/// the filters of its targets, triggers, conditions and effect data, at any depth.
pub(crate) fn reads_players_of(def: &CardDef) -> bool {
    let mut read = true;
    def.filters(&mut |filter| {
        if let Filter::Whose(players) = filter {
            read &= reads(players);
        }
    });
    read
}

/// [`Players::EachOpponent`]: whether `player` is one of `you`'s opponents
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_player_is_among_the_players_named_for_you() {
        let (you, other) = (Seat::BOTH[0], Seat::BOTH[1]);
        assert!(is_for(&Players::You, you, you));
        assert!(!is_for(&Players::You, other, you));
        assert!(is_for(&Players::EachOpponent, other, you));
        assert!(!is_for(&Players::EachOpponent, you, you));
        assert!(is_for(&Players::EachPlayer, you, you));
        assert!(is_for(&Players::EachPlayer, other, you));
    }
}
