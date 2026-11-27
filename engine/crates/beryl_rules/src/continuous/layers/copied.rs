//! Layer 1 inputs and static abilities of a permanent with a copy effect.
//! Implements: none. Reads the kernel's copy exceptions as layer inputs.

use beryl_kernel::derive::Rederive;

use crate::card::AbilityIx;
use crate::fixed::Full;
use crate::layout::Layout;
use crate::state::{AbilityMask, GrantRow, GrantSource};
use crate::table::{Capability, GameTable};

use super::{Applying, Listed, list_static, merged};

/// Applies the copiable values before listing the permanent's static abilities. An ability
/// omitted by a copy exception (see [CR 707.9d]) has no effect to list.
///
/// Returns whether the copy omits a printed ability, whether it has merged faces, and
/// whether the ability mask and the list had room. Only permanents with a copy effect
/// take this path.
///
/// [CR 707.9d]: https://yawgatog.com/resources/magic-rules/#R7079d
#[cold]
#[inline(never)]
#[allow(
    clippy::disallowed_methods,
    reason = "the layers read a permanent's printed face up, by each ability's place on it"
)]
pub(super) fn list<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    list: &mut Option<Applying<L>>,
    at: usize,
) -> (bool, bool, Result<(), Full>) {
    let slot = s.battlefield()[at];
    let mut base = s.rows()[at];
    let mut listed = s.copy_layer_one(t, slot, &mut base);
    let omitted = !{ base.lost }.is_empty();
    s.results()[at] = base;
    listed = listed.and(copy_grants(s, t, at));
    let what = s.definition(slot);
    let (source, face) = what.record(t);
    for (ability, order) in what.face(t).abilities.iter().zip(0..) {
        let Some(spec) = ability.layer() else {
            continue;
        };
        if s.has_ability_so_far(at, AbilityIx::new(order)) {
            let on = Listed {
                at,
                order,
                source,
                face,
            };
            listed = listed.and(list_static(s, list, on, ability, spec));
        }
    }
    if t.can(Capability::Merges) {
        let (merged, full) = merged::list(s, t, list, at);
        return (omitted, merged, listed.and(full));
    }
    (omitted, false, listed)
}

/// Adds a row of the table of grants for each activated ability the exceptions of the copy
/// effect on the permanent at `at` give it ([`beryl_kernel::State::copy_grants`]), as though
/// it had been granted them first of all, and gives the permanent each row's bit. [`Full`] if
/// the table has no place for one.
fn copy_grants<L: Layout>(s: &mut Rederive<'_, L>, t: &GameTable, at: usize) -> Result<(), Full> {
    let slot = s.battlefield()[at];
    // Read one at a time: the table is written between reads.
    for n in 0.. {
        let Some(grant) = s.copy_grants(t, slot).nth(n) else {
            return Ok(());
        };
        let k = s.add_grant(GrantRow {
            card: grant.card,
            grant: grant.grant,
            source: GrantSource::Permanent(slot),
        })?;
        let d = &mut s.results()[at];
        // `add_grant` gives no place past the mask's width.
        d.granted = d.granted.with_row(k).ok_or(Full)?;
    }
    Ok(())
}
