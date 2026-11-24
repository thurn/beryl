//! Legal casts that choose characteristics beyond the printed faces.
//!
//! Implements: none. Applies the zone rules for fuse and morph before checking casting legality.

use crate::card::{CardDef, Castings, FACE_DOWN, FaceIx};
use crate::decision::{OPTIONS, Opt};
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::state::State;
use crate::table::{Capability, GameTable};

use super::ways::{Ways, can_fuse};
use super::{From, Situation, has_morph, refused};

/// Whether the card could be cast fused or face down at this time. Keep these checks out
/// of the scan of potential spells in a game that has neither keyword.
#[cold]
#[inline(never)]
pub(super) fn in_time<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    def: &CardDef,
    from: From,
) -> bool {
    (t.can(Capability::Morph)
        && has_morph(def.front())
        && super::in_time(s, t, now, &FACE_DOWN, from))
        || (can_fuse(s, def, now.player, slot)
            && super::in_time(s, t, now, def.face(FaceIx::FUSED), from))
}

/// Offers the available casts with combined or face-down characteristics. Only games
/// whose cards have fuse or morph visit this path.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn options<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    def: &CardDef,
    ways: Ways,
    from: From,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let morph =
        t.can(Capability::Morph) && ways.plain && !ways.alternative && has_morph(def.front());
    let fuse = can_fuse(s, def, now.player, slot);
    for (allowed, face, costs) in [
        (morph, FaceIx::DOWN, Castings::NONE),
        (fuse, FaceIx::FUSED, Castings::FUSE),
    ] {
        if allowed && refused(s, t, now, slot, face, costs, from, ways.increase).is_none() {
            out.push(Opt::Cast { slot, face, costs })?;
        }
    }
    Ok(())
}
