//! `Filter`: the predicates targets and affected sets are written in (architecture §6.2).

use crate::bitboard::Bitboard;
use crate::card::{Subtype, Supertypes, Types};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{ObjectFlags, State, Zone};
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
    pub fn matches<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        slot: L::Slot,
        you: Seat,
    ) -> bool {
        self.matches_x(s, t, slot, you, 0)
    }

    /// Whether the object in `slot` passes, evaluated for player `you` and the value `x` of
    /// X.
    pub fn matches_x<L: Layout>(
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
        // [CR 712.8e](https://yawgatog.com/resources/magic-rules/#R7128e): a transforming
        // double-faced permanent with its back face up "has only the characteristics of its
        // back face. However, its mana value is calculated using the mana cost of its front
        // face. If a permanent is copying the back face of a nonmodal double-faced permanent
        // (even if the object representing that copy is itself a double-faced permanent), the
        // mana value of that permanent is 0." A token with a double-faced card's faces is
        // always such a copy (CR 707.8a): Fractured Identity on a transformed Jace.
        let mana_value = || {
            let up = face();
            if !up.transformed {
                up.cost.mana_value()
            } else if o.flags.has(ObjectFlags::TOKEN) {
                0
            } else {
                t.def(o.card).front().cost.mana_value()
            }
        };
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
                player
                    == match whose {
                        Whose::You => you,
                        Whose::Opponent => you.opponent(),
                    }
            }
            Filter::Tapped => o.flags.has(ObjectFlags::TAPPED),
            Filter::ManaValueAtMost(n) => mana_value() <= n,
            Filter::ManaValueAtMostX => mana_value() <= x,
            Filter::Not(f) => !f.matches_x(s, t, slot, you, x),
            Filter::All(fs) => fs.iter().all(|f| f.matches_x(s, t, slot, you, x)),
            Filter::AnyOf(fs) => fs.iter().any(|f| f.matches_x(s, t, slot, you, x)),
        }
    }

    /// The members of `within` that pass, where X is 0.
    pub fn select<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        within: Bitboard<L::Slot>,
        you: Seat,
    ) -> Bitboard<L::Slot> {
        self.select_x(s, t, within, you, 0)
    }

    /// The members of `within` that pass, for the value `x` of X.
    pub fn select_x<L: Layout>(
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
