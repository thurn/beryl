//! Legal casts that choose characteristics beyond the printed faces.
//!
//! Implements: none. Applies the zone rules for fuse, morph and casts face down by an effect.

use crate::bitboard::Bitboard;
use crate::card::{CardDef, Castings, FACE_DOWN, FaceIx};
use crate::decision::{Candidates, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
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
    for (face, costs) in casts(s, t, now.player, slot, def, ways) {
        if refused(s, t, now, slot, face, costs, from, ways.increase).is_none() {
            out.push(Opt::Cast { slot, face, costs })?;
        }
    }
    Ok(())
}

/// Appends the casts face down of each of `cards` that an effect offers during a resolution
/// ([`crate::card::CastFace::DownTurnedUpWhenUsed`]): as a 2/2 creature spell, whatever the
/// card is, with "no text, no name, no subtypes, and no mana cost" (see [CR 708.2a]).
///
/// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
#[cold]
#[inline(never)]
pub(in crate::casting) fn face_down_casts<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    cards: Bitboard<L::Slot>,
    from: From,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    Candidates::<L>::extend_options(out, collapse(s, cards), |out, slot| {
        if s.card_def(t, slot).is_some()
            && refused(s, t, now, slot, FaceIx::DOWN, Castings::NONE, from, 0).is_none()
        {
            out.push(Opt::Cast {
                slot,
                face: FaceIx::DOWN,
                costs: Castings::NONE,
            })?;
        }
        Ok(())
    })
}

/// The face and casting options of each cast with combined or face-down characteristics that
/// `ways` let `player` try for the card in `slot`: face down for morph, then both halves for
/// fuse. Each is still to pass the checks of an ordinary cast.
pub(super) fn casts<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    def: &CardDef,
    ways: Ways,
) -> impl Iterator<Item = (FaceIx, Castings)> {
    let morph =
        t.can(Capability::Morph) && ways.plain && !ways.alternative && has_morph(def.front());
    let fuse = can_fuse(s, def, player, slot);
    [
        (morph, FaceIx::DOWN, Castings::NONE),
        (fuse, FaceIx::FUSED, Castings::FUSE),
    ]
    .into_iter()
    .filter_map(|(allowed, face, costs)| allowed.then_some((face, costs)))
}
