//! Whether the order of the timestamps of permanents that entered the battlefield together
//! could change the layer results, so that their controller is asked for it
//! ([CR 613.7m]): "Objects controlled by the active player ... have an earlier relative
//! timestamp in the order of that player's choice". A choice that cannot change anything is
//! not asked.
//!
//! The test is the one the layers use to find effects that may depend on each other
//! ([`super::dependency`]): two effects in one layer may interact when one reads what the
//! other writes. Here two effects that write the same characteristic count too, unless they
//! give the same result in either order ([`LayerChange::commutes_with`]), because timestamp
//! order alone decides which of them applies last.
//!
//! [CR 613.7m]: https://yawgatog.com/resources/magic-rules/#R6137m

use super::apply;
use crate::card::{AbilityIx, Layer};
use crate::layout::Layout;
use crate::state::{Characteristics, LayerChange, State, read_layer};
use crate::table::GameTable;

/// One part of the effect of a static ability, as far as its order among others goes.
#[derive(Clone, Copy)]
struct Part {
    layer: Layer,
    /// What finding its existence, its set and its amount reads of the layer results.
    reads: Characteristics,
    change: LayerChange,
}

impl Part {
    /// Whether applying `self` and `other` in the other order could change the results.
    fn interacts(self, other: Part) -> bool {
        let (a, b) = (self.change.writes(), other.change.writes());
        self.layer == other.layer
            && (self.reads.intersects(b)
                || other.reads.intersects(a)
                || (a.intersects(b) && !commutes(self.change, other.change)))
    }
}

/// Whether two changes commute. A change whose amount the layers read names its ability by
/// index alone, so two from different cards may look equal: those never commute.
fn commutes(a: LayerChange, b: LayerChange) -> bool {
    !matches!(a, LayerChange::Read { .. }) && a.commutes_with(b)
}

/// Whether the order of `slots`' timestamps, permanents on the battlefield controlled by
/// one player that entered it together, could change the layer results: whether two of
/// them have static abilities with parts in one layer that interact ([`Part::interacts`]).
/// Two permanents with the same copiable values never count: swapping their timestamps
/// changes nothing.
pub(crate) fn order_may_matter<L: Layout>(s: &State<L>, t: &GameTable, slots: &[L::Slot]) -> bool {
    slots.iter().enumerate().any(|(i, &a)| {
        slots[i + 1..].iter().any(|&b| {
            s.copiable(a).record(t) != s.copiable(b).record(t)
                && parts(s, t, a).any(|x| parts(s, t, b).any(|y| x.interacts(y)))
        })
    })
}

/// The parts of the effects of the static abilities of the permanent in `slot` that apply
/// in layers ([CR 613.6]): "If an effect should be applied in different layers and/or
/// sublayers, the parts of the effect each apply in their appropriate ones." What each reads
/// is what [`super::dependency`] finds for its first part, for every part: more than it
/// may read, never less.
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
fn parts<'a, L: Layout>(
    s: &'a State<L>,
    t: &'a GameTable,
    slot: L::Slot,
) -> impl Iterator<Item = Part> + 'a {
    let what = s.copiable(slot);
    let face = what.face(t);
    let (_, face_ix) = what.record(t);
    let you = s.controller(slot);
    face.abilities
        .iter()
        .enumerate()
        .filter_map(|(n, ability)| ability.layer().map(|spec| (n, spec)))
        .flat_map(move |(n, spec)| {
            let reads = apply::existence_reads(spec);
            spec.modifications
                .iter()
                .enumerate()
                .map(move |(part, &modification)| {
                    let change =
                        LayerChange::resolve(modification, you).unwrap_or(LayerChange::Read {
                            layer: read_layer(modification),
                            face: face_ix,
                            ability: AbilityIx::new(n as u8),
                            part: part as u8,
                        });
                    let amount = match change {
                        LayerChange::Read { .. } => apply::amount_reads(spec, part as u8),
                        _ => Characteristics::NONE,
                    };
                    Part {
                        layer: change.layer(),
                        reads: reads.union(amount),
                        change,
                    }
                })
        })
}
