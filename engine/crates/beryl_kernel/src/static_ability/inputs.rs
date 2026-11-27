//! Layer inputs that static abilities and state triggers read besides the battlefield's own
//! changes ([`Inputs`]).
//!
//! Implements: none. Classifies which non-battlefield changes mark the layer results stale.

use beryl_card::{Condition, Face, LentFrom, Modification, Objects};

use super::{
    Counted, compares, condition_reads_filter, mentions_counter, mentions_tapped, reads_graveyards,
    state_condition,
};

/// What a face's static abilities read as the layers apply that no layer writes, one bit
/// each: the layer results are stale when one of these changes while a permanent on the
/// battlefield reads it. The battlefield's own changes (what is on it, its faces, its
/// statuses and the effects that apply) mark the results stale whatever the permanents read.
/// The card table holds a set for each face ([`crate::table::FaceFacts::inputs`]), and the
/// recompute stores the union of those of the permanents on the battlefield.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Inputs(u8);

impl Inputs {
    /// Nothing.
    pub const NONE: Inputs = Inputs(0);
    /// The counters of some kind on the permanent with the ability, which an "as long as"
    /// compares ([`super::reads_counters`] says which kinds).
    pub const COUNTERS: Inputs = Inputs(1);
    /// The cards in graveyards, which an amount counts
    /// ([`beryl_card::Amount::CardTypesAmong`]) or a lending ability reads
    /// ([`LentFrom::Graveyards`]).
    pub const GRAVEYARDS: Inputs = Inputs(2);
    /// Whether a permanent is tapped, which a filter or an "as long as" asks
    /// ([`beryl_card::Filter::Tapped`]): "untapped creatures you control get +0/+2".
    pub const TAPPED: Inputs = Inputs(4);
    /// A player's life total, which an "as long as" compares ([`beryl_card::Amount::Life`]):
    /// "as long as an opponent has 10 or less life".
    pub const LIFE: Inputs = Inputs(8);
    /// The number of cards in the controller's hand.
    pub const HAND: Inputs = Inputs(16);
    /// The cards exiled with a permanent (linked exile, [`LentFrom::ExiledWithThis`]).
    pub const LINKED: Inputs = Inputs(32);
    /// Whether any permanent has a counter of some kind, which a filter asks
    /// ([`beryl_card::Filter::WithCounter`]): "creatures you control with +1/+1 counters on them
    /// have ...".
    pub const ANY_COUNTERS: Inputs = Inputs(64);

    /// Both sets' inputs.
    #[inline]
    #[must_use]
    pub const fn union(self, other: Inputs) -> Inputs {
        Inputs(self.0 | other.0)
    }

    /// Whether every input of `other` is in this set.
    #[inline]
    pub const fn contains(self, other: Inputs) -> bool {
        self.0 & other.0 == other.0
    }

    /// What the static abilities of `face` read that no layer writes, with what the
    /// conditions of its state triggers read ([`state_condition`]): a state trigger whose
    /// condition is [`super::watched`] is matched only after the layer results were stored,
    /// so a change in what it reads must mark them stale as a change in an "as long as" does.
    pub fn of(face: &Face) -> Inputs {
        let mut inputs = Inputs::NONE;
        for spec in face.abilities.iter().filter_map(beryl_card::Ability::layer) {
            if let Some(condition) = &spec.condition {
                inputs = inputs.union(Inputs::read_by(condition));
            }
            let graveyards = spec.modifications.iter().any(|m| {
                m.pt_amounts().is_some_and(|(power, toughness)| {
                    reads_graveyards(power) || toughness.is_some_and(reads_graveyards)
                }) || matches!(
                    m,
                    Modification::Lend(lent) if matches!(lent.from, LentFrom::Graveyards(_))
                )
            });
            if graveyards {
                inputs = inputs.union(Inputs::GRAVEYARDS);
            }
            if spec.modifications.iter().any(|m| {
                matches!(
                    m,
                    Modification::Lend(lent) if matches!(lent.from, LentFrom::ExiledWithThis(_))
                )
            }) {
                inputs = inputs.union(Inputs::LINKED);
            }
            if affected_mentions(spec.affected, mentions_tapped) {
                inputs = inputs.union(Inputs::TAPPED);
            }
            if affected_mentions(spec.affected, mentions_counter) {
                inputs = inputs.union(Inputs::ANY_COUNTERS);
            }
        }
        for condition in face.abilities.iter().filter_map(state_condition) {
            inputs = inputs.union(Inputs::read_by(condition));
        }
        inputs
    }

    /// What `condition`, an "as long as" or a state trigger's condition, reads that no layer
    /// writes.
    fn read_by(condition: &Condition) -> Inputs {
        let compared = |read: fn(&Counted) -> bool| compares(condition, &read);
        [
            (
                compared(|n| matches!(n, Counted::Counters(_))),
                Inputs::COUNTERS,
            ),
            (compared(|n| matches!(n, Counted::Life(_))), Inputs::LIFE),
            (compared(|n| matches!(n, Counted::Hand)), Inputs::HAND),
            (
                condition_reads_filter(condition, mentions_tapped),
                Inputs::TAPPED,
            ),
            (
                condition_reads_filter(condition, mentions_counter),
                Inputs::ANY_COUNTERS,
            ),
        ]
        .into_iter()
        .filter(|&(reads, _)| reads)
        .fold(Inputs::NONE, |inputs, (_, input)| inputs.union(input))
    }
}

/// Whether `affected`'s filter satisfies `mentions`.
fn affected_mentions(affected: Objects, mentions: fn(beryl_card::Filter) -> bool) -> bool {
    matches!(
        affected,
        Objects::All { filter, .. } | Objects::Everywhere(filter) if mentions(filter)
    ) || matches!(
        affected,
        Objects::Other(&Objects::All { filter, .. }) if mentions(filter)
    )
}
