//! `Filter`: the predicates targets and affected sets are written in (architecture §6.2).

use crate::bitboard::Bitboard;
use crate::card::{Subtype, Subtypes, Supertypes, Types};
use crate::state::{ObjectFlags, State, Zone, opponent};
use crate::table::GameTable;

/// Whose objects a filter accepts, relative to the player the filter is evaluated for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Whose {
    You,
    Opponent,
}

/// A predicate over objects. Evaluating one is a pass over a candidate set; the hot ones
/// become ANDs over per-slot attribute bitboards as the cards that need them arrive.
///
/// A permanent's characteristics and controller are its derived ones, after continuous
/// effects (§9); elsewhere they are the face's.
#[derive(Clone, Copy, Debug)]
pub enum Filter {
    Any,
    /// Has at least one of these card types.
    Types(Types),
    Supertypes(Supertypes),
    Subtype(Subtype),
    /// Controlled by (on the battlefield or stack) or owned by (elsewhere) this player
    /// ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)): "Only objects on the
    /// stack or on the battlefield have a controller."
    Whose(Whose),
    Tapped,
    ManaValueAtMost(u8),
    /// Mana value X or less, X being the value of the spell or ability whose target this is
    /// (Thieving Skydiver's "target artifact with mana value X or less").
    ManaValueAtMostX,
    Not(&'static Filter),
    All(&'static [Filter]),
    AnyOf(&'static [Filter]),
}

impl Filter {
    /// Whether the object in `slot` passes, evaluated for player `you`, where X is 0.
    pub fn matches(&self, s: &State, t: &GameTable, slot: u8, you: u8) -> bool {
        self.matches_x(s, t, slot, you, 0)
    }

    /// Whether the object in `slot` passes, evaluated for player `you` and the value `x` of
    /// X.
    pub fn matches_x(&self, s: &State, t: &GameTable, slot: u8, you: u8, x: u8) -> bool {
        let o = s.object(slot);
        let face = || &t.def(o.card).faces[usize::from(o.face)];
        let derived = || (o.zone == Zone::Battlefield).then(|| s.derived(slot));
        match *self {
            Filter::Any => true,
            Filter::Types(types) => derived()
                .map_or(face().types, |d| Types(d.types))
                .intersects(types),
            Filter::Supertypes(supertypes) => derived()
                .map_or(face().supertypes, |d| Supertypes(d.supertypes))
                .intersects(supertypes),
            Filter::Subtype(subtype) => derived()
                .map_or(face().subtypes, |d| Subtypes(d.subtypes))
                .contains(subtype),
            Filter::Whose(whose) => {
                let player = if matches!(o.zone, Zone::Battlefield | Zone::Stack) {
                    s.controller(slot)
                } else {
                    o.owner
                };
                player
                    == match whose {
                        Whose::You => you,
                        Whose::Opponent => opponent(you),
                    }
            }
            Filter::Tapped => o.flags.has(ObjectFlags::TAPPED),
            Filter::ManaValueAtMost(n) => face().cost.mana_value() <= n,
            Filter::ManaValueAtMostX => face().cost.mana_value() <= x,
            Filter::Not(f) => !f.matches_x(s, t, slot, you, x),
            Filter::All(fs) => fs.iter().all(|f| f.matches_x(s, t, slot, you, x)),
            Filter::AnyOf(fs) => fs.iter().any(|f| f.matches_x(s, t, slot, you, x)),
        }
    }

    /// The members of `within` that pass, where X is 0.
    pub fn select(&self, s: &State, t: &GameTable, within: Bitboard, you: u8) -> Bitboard {
        self.select_x(s, t, within, you, 0)
    }

    /// The members of `within` that pass, for the value `x` of X.
    pub fn select_x(&self, s: &State, t: &GameTable, within: Bitboard, you: u8, x: u8) -> Bitboard {
        within
            .iter()
            .filter(|&slot| self.matches_x(s, t, slot, you, x))
            .collect()
    }
}
