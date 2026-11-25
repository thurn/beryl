//! The continuous effects that reach objects off the battlefield, listed as the layers run
//! (the kernel's `elsewhere`, which reads them).
//!
//! A static ability that affects "all cards that aren't on the battlefield, spells, and
//! permanents" ([`Objects::Everywhere`]) changes the permanents through the layers, as any
//! static ability on a set does, and every other object it reaches through the kernel's
//! reads of characteristics there ([CR 611.3b]): "The effect applies at all times that the
//! permanent generating it is on the battlefield or the object generating it is in the
//! appropriate zone." Such an effect's parts are listed with the layers' other effects, in
//! the order they apply, so those parts are what the kernel keeps, in the same order.
//!
//! Implements: none. Lists the parts of static effects that reach other zones under CR 611.3.
//!
//! [CR 611.3b]: https://yawgatog.com/resources/magic-rules/#R6113b

use beryl_kernel::derive::Rederive;
use beryl_kernel::query::zone_characteristics::{ELSEWHERE, Reach};

use super::{Apply, GRANTED, Target, position, static_ability_of};
use crate::card::Objects;
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::table::Capability;
use crate::table::GameTable;

/// Stores, beside the layer results, the parts of `list` (sorted in the order they apply)
/// that come from static abilities reaching every zone. [`Full`] past
/// [`ELSEWHERE`] in a fixed layout, with those that fit stored.
///
/// A game whose cards cannot say such an effect pays one AND of the table's capabilities.
#[inline]
pub(super) fn reach<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    list: &[Apply<L>],
) -> Result<(), Full> {
    if t.can(Capability::ReachesElsewhere) {
        list_reaching(s, t, list)
    } else {
        Ok(())
    }
}

/// [`reach`] in a game whose cards can say such an effect. Out of line.
#[inline(never)]
fn list_reaching<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    list: &[Apply<L>],
) -> Result<(), Full> {
    let mut reaching: L::Queue<Reach<L>, ELSEWHERE> = Default::default();
    let mut listed = Ok(());
    for a in list {
        // A face's own static ability: the card table refuses a granted one that reaches
        // every zone (the kernel's `listener::read_when_granted`). One of a card under a
        // merged permanent is not read here: the kernel names a reaching ability by its index
        // on the permanent's own face.
        let (Target::Static(_), Some((from, _))) = (a.target, a.from) else {
            continue;
        };
        if a.order & (GRANTED | super::merged::MERGED) != 0 {
            continue;
        }
        let ability = static_ability_of(s, t, from, a);
        if !matches!(
            ability.layer().map(|spec| spec.affected),
            Some(Objects::Everywhere(_))
        ) {
            continue;
        }
        debug_assert!(beryl_kernel::query::zone_characteristics::reaches(a.change));
        listed = listed.and(reaching.push(Reach {
            source: s.battlefield()[position::<L>(from)],
            ability: a.order,
            part: a.part,
        }));
    }
    if reaching.is_empty() && !s.reaches_elsewhere() {
        return listed;
    }
    listed.and(s.reach(reaching.iter().copied()))
}
