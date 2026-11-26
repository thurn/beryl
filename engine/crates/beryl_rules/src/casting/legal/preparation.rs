//! Legal casts of prepare spell copies.
//! Implements: none. Applies the ordinary casting checks to a copy (see CR 723.3c).

use super::{
    Candidates, From, Full, GameTable, Layout, OPTIONS, Opt, Queue, Refusal, Situation, State,
    collapse, keep_furthest, option_sets, refused,
};
use crate::card::FaceIx;

/// Appends legal prepare spell casts from exile. See [CR 723.3c]: "For as long as the copy
/// remains in exile, the prepared permanent’s controller may cast the copy." The inset's
/// timing, options and targets are checked by the normal casting rules.
///
/// [CR 723.3c]: https://yawgatog.com/resources/magic-rules/#R7233c
#[cold]
#[inline(never)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads the casting options printed on the prepare card's face"
)]
pub(super) fn casts<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    Candidates::<L>::extend_options(
        out,
        collapse(s, s.prepare_spells_for(now.player)),
        |out, slot| {
            let face = FaceIx::BACK;
            for costs in option_sets(s.copiable(slot).face(t), false) {
                if refused(s, t, now, slot, face, costs, From::Priority, 0).is_none() {
                    out.push(Opt::Cast { slot, face, costs })?;
                }
            }
            Ok(())
        },
    )
}

#[cold]
#[inline(never)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads the casting options printed on the prepare card's face"
)]
pub(super) fn refusal<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L>,
    slot: L::Slot,
) -> Result<Option<Refusal<L>>, Full> {
    let mut most = None;
    for costs in option_sets(s.copiable(slot).face(t), false) {
        let Some(reason) = refused(s, t, now, slot, FaceIx::BACK, costs, From::Priority, 0) else {
            return Ok(None);
        };
        keep_furthest(&mut most, reason);
    }
    now.complete()?;
    Ok(most)
}
