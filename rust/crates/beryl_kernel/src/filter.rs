//! Evaluating a [`Filter`], the predicates targets and affected sets are written in
//! (architecture §6.2), against a game state.

pub use beryl_card::Filter;
use beryl_card::{CardDef, Players, Words};

use crate::bitboard::Bitboard;
use crate::elsewhere::SoFar;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Flag, State};
use crate::table::GameTable;

/// What a [`Filter`] accepts in a game. Evaluating one is a pass over a candidate set; the
/// hot ones become ANDs over per-slot attribute bitboards as the cards that need them arrive.
///
/// The filter is vocabulary (`beryl_card`), which knows nothing of the state, so its meaning
/// is this trait: bring it into scope to evaluate one.
///
/// A filter is read for the spell or ability it is part of: `you` is its controller, and
/// `words` are the text changes of the object it is printed on ([`State::words_of`]), through
/// which the color words and subtypes it names are read ([`beryl_card::words`]).
pub trait Evaluate {
    /// Whether the object in `slot` passes, evaluated for player `you` through `words`,
    /// where X is 0.
    fn matches<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        slot: L::Slot,
        you: Seat,
        words: &Words,
    ) -> bool {
        self.matches_x(s, t, slot, you, words, 0)
    }

    /// Whether the object in `slot` passes, evaluated for player `you` through `words` and
    /// the value `x` of X.
    fn matches_x<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        slot: L::Slot,
        you: Seat,
        words: &Words,
        x: u8,
    ) -> bool;

    /// The members of `within` that pass, where X is 0.
    fn select<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        within: Bitboard<L::Slot>,
        you: Seat,
        words: &Words,
    ) -> Bitboard<L::Slot> {
        self.select_x(s, t, within, you, words, 0)
    }

    /// The members of `within` that pass, for the value `x` of X.
    fn select_x<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        within: Bitboard<L::Slot>,
        you: Seat,
        words: &Words,
        x: u8,
    ) -> Bitboard<L::Slot> {
        within
            .iter()
            .filter(|&slot| self.matches_x(s, t, slot, you, words, x))
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
        words: &Words,
        x: u8,
    ) -> bool {
        let o = s.object(slot);
        let face = || s.definition(slot).face(t);
        let derived = || o.on_battlefield().then(|| s.derived(slot));
        // A permanent's characteristics are the layers' results. Anything else's are its
        // face's while no effect reaches other zones, and the kernel's reads of them
        // otherwise ([`crate::elsewhere`]), out of line.
        let elsewhere = || s.reaches_elsewhere().then(|| s.elsewhere(t, slot));
        match *self {
            Filter::Any => true,
            Filter::Types(types) => match derived() {
                Some(d) => d.types,
                None => elsewhere().map_or_else(|| face().types, |c| c.types),
            }
            .intersects(types),
            Filter::Supertypes(supertypes) => match derived() {
                Some(d) => d.supertypes,
                None => elsewhere().map_or_else(|| face().supertypes, |c| c.supertypes),
            }
            .intersects(supertypes),
            Filter::SubtypeWord(word) => {
                let subtype = word.read(words);
                match derived() {
                    Some(d) => d.subtypes.contains(t.subtypes(), subtype),
                    None => match elsewhere() {
                        Some(c) => c.subtypes.contains(t.subtypes(), subtype),
                        // The candidate's type line is its own text, read through its own
                        // words.
                        None => face().subtypes.read(&s.words_of(slot)).contains(subtype),
                    },
                }
            }
            Filter::ColorWords(colors) => match derived() {
                Some(d) => d.colors,
                None => elsewhere().map_or_else(|| face().colors, |c| c.colors),
            }
            .intersects(colors.read(words)),
            // CR 108.4a: a card that is no permanent or spell has no controller, and "its
            // owner" is used instead, which is what the query answers.
            Filter::Whose(players) => is_for(players, s.controller(slot), you),
            Filter::Tapped => o.flags.has(Flag::Tapped),
            Filter::Token => o.is_token(),
            Filter::ManaValueAtMost(n) => s.mana_value(t, slot) <= n,
            Filter::ManaValueAtMostX => s.mana_value(t, slot) <= x,
            Filter::PowerAtMost(n) => s.power(t, slot) <= n,
            Filter::InZone(zone) => o.zone == zone,
            Filter::Not(_) | Filter::All(_) | Filter::AnyOf(_) => {
                combined(self, s, t, slot, you, words, x)
            }
        }
    }
}

/// [`Evaluate::matches_x`] for a filter made of other filters. Out of line: a loop that calls
/// back into `matches_x` keeps its operands in saved registers, and inlined there it made
/// every call save them, even for the single test most filters are.
#[inline(never)]
fn combined<L: Layout>(
    filter: &Filter,
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    you: Seat,
    words: &Words,
    x: u8,
) -> bool {
    match *filter {
        Filter::Not(f) => !f.matches_x(s, t, slot, you, words, x),
        Filter::All(fs) => fs.iter().all(|f| f.matches_x(s, t, slot, you, words, x)),
        Filter::AnyOf(fs) => fs.iter().any(|f| f.matches_x(s, t, slot, you, words, x)),
        _ => filter.matches_x(s, t, slot, you, words, x),
    }
}

/// Whether the object in `slot`, which is not a permanent, passes `filter` read for `you`
/// through `words`, its characteristics being `so_far`: what the effects that reach other
/// zones applied before the one whose filter this is made of it ([`crate::elsewhere`]). A
/// filter that reads a characteristic an earlier effect changed sees the change, as the
/// layers find a static ability's set against the layers before it (CR 613.1). Out of line,
/// apart from [`Evaluate::matches_x`]: only a game with such an effect comes here.
#[cold]
#[inline(never)]
pub(crate) fn passes_so_far<L: Layout>(
    filter: &Filter,
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    you: Seat,
    words: &Words,
    so_far: &SoFar,
) -> bool {
    let passes = |f: &Filter| passes_so_far(f, s, t, slot, you, words, so_far);
    match *filter {
        Filter::Types(types) => so_far.types.intersects(types),
        Filter::Supertypes(supertypes) => so_far.supertypes.intersects(supertypes),
        Filter::SubtypeWord(word) => so_far.subtypes.contains(t.subtypes(), word.read(words)),
        Filter::ColorWords(colors) => so_far.colors.intersects(colors.read(words)),
        Filter::Not(f) => !passes(f),
        Filter::All(fs) => fs.iter().all(passes),
        Filter::AnyOf(fs) => fs.iter().any(passes),
        // The rest read no characteristic an effect here changes.
        Filter::Any
        | Filter::Whose(_)
        | Filter::Tapped
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::PowerAtMost(_)
        | Filter::InZone(_) => filter.matches(s, t, slot, you, words),
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

/// [`Players::EachOpponent`]: whether `player` is one of `you`'s opponents ([CR 102.2]): "In a
/// two-player game, a player's opponent is the other player."
///
/// The one place that entry is read, and [`crate::State::opponents_of`] is the set it tests. With
/// more players and no teams every other player is an opponent, as written here. Between teams,
/// "the player's opponents are all players not on their team" ([CR 102.3]): this function changes
/// then, and no card does.
///
/// [CR 102.2]: https://yawgatog.com/resources/magic-rules/#R1022
/// [CR 102.3]: https://yawgatog.com/resources/magic-rules/#R1023
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
