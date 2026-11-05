//! `Filter`: the predicates targets and affected sets are written in (architecture §6.2).

use crate::bitboard::Bitboard;
use crate::card::{Subtype, Supertypes, Types};
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
/// Characteristics are read from the card's face until the layers work provides derived
/// ones for permanents.
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
    Not(&'static Filter),
    All(&'static [Filter]),
    AnyOf(&'static [Filter]),
}

impl Filter {
    /// Whether the object in `slot` passes, evaluated for player `you`.
    pub fn matches(&self, s: &State, t: &GameTable, slot: u8, you: u8) -> bool {
        let o = s.object(slot);
        let face = || &t.def(o.card).faces[usize::from(o.face)];
        match *self {
            Filter::Any => true,
            Filter::Types(types) => face().types.intersects(types),
            Filter::Supertypes(supertypes) => face().supertypes.intersects(supertypes),
            Filter::Subtype(subtype) => face().subtypes.contains(subtype),
            Filter::Whose(whose) => {
                let player = if matches!(o.zone, Zone::Battlefield | Zone::Stack) {
                    o.controller
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
            Filter::Not(f) => !f.matches(s, t, slot, you),
            Filter::All(fs) => fs.iter().all(|f| f.matches(s, t, slot, you)),
            Filter::AnyOf(fs) => fs.iter().any(|f| f.matches(s, t, slot, you)),
        }
    }

    /// The members of `within` that pass.
    pub fn select(&self, s: &State, t: &GameTable, within: Bitboard, you: u8) -> Bitboard {
        within
            .iter()
            .filter(|&slot| self.matches(s, t, slot, you))
            .collect()
    }
}
