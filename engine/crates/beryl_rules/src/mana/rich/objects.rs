//! Whether a spell with convoke or improvise can be paid in a game whose cards have priced mana
//! abilities or change mana abilities, as Mana Flare does, where the plain units of
//! `Funds::pays_with_objects` cannot say what the lands make.
//!
//! Implements: none. Checks the payability of convoke and improvise spells on the rich path.

use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::ids::Seat;
use crate::layout::{Layout, Table};
use crate::state::State;
use crate::table::GameTable;

use super::super::CLASSES;
use super::super::remembered::Paying;
use super::super::sources::Sources;
use super::super::units::Units;
use super::pays_from_state_with;

/// The most objects that make mana [`pays_with_objects`] tries both as mana sources and as
/// payment in their place, which is `2^BOTH_WAYS` searches.
const BOTH_WAYS: usize = 6;

/// Whether `player` can pay `cost` for what `paying` is for, with none of `exclude`, tapping any
/// of `objects` rather than paying one mana of the kinds `kinds` gives each, and activating
/// their other permanents' mana abilities in sequence for the rest (see [CR 702.51a]): "For
/// each colored mana in this spell's total cost, you may tap an untapped creature of that color
/// you control rather than pay that mana. For each generic mana in this spell's total cost,
/// you may tap an untapped creature you control rather than pay that mana." Improvise says the
/// same of artifacts and generic mana (see CR 702.126a).
///
/// The objects pay part of the total cost as it is paid, after the mana abilities are
/// activated (see [CR 601.2g]; see [CR 601.2h]), so what one pays may pay no mana ability's
/// price. An object that makes no mana is always tapped, since that only adds to what pays. One
/// that makes mana cannot also be tapped for convoke (see [CR 118.3]: "a permanent that's
/// already tapped can't be tapped to pay a cost"), so it is tried both ways. Past
/// [`BOTH_WAYS`] of those, the rest are mana sources only, so that the answer may say no to a
/// cost a payment could make, never yes to one it cannot.
///
/// [CR 702.51a]: https://yawgatog.com/resources/magic-rules/#R70251a
/// [CR 601.2g]: https://yawgatog.com/resources/magic-rules/#R6012g
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
/// [CR 118.3]: https://yawgatog.com/resources/magic-rules/#R1183
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(in crate::mana) fn pays_with_objects<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
    objects: Bitboard<L::Slot>,
    kinds: impl Fn(L::Slot) -> u8,
) -> bool {
    // More classes than a small layout holds: say it may be paid, so that the payment's own
    // scan finds the layout full and the game goes on in a larger one.
    let Ok(all) = Sources::of(s, t, player, exclude, paying) else {
        return true;
    };
    let makers = all
        .classes()
        .iter()
        .map(|c| c.members)
        .chain(all.extras.iter().map(|e| e.members))
        .fold(Bitboard::EMPTY, |m, members| m | members);
    let objects = objects & !exclude;
    let spare = (objects & !makers).fold(Units::NONE, |mut u, slot| {
        u.add(kinds(slot), 1);
        u
    });
    let (both, len) = (objects & makers).fold(
        ([L::Slot::default(); BOTH_WAYS], 0usize),
        |(mut list, len), slot| {
            if len < BOTH_WAYS {
                list[len] = slot;
                (list, len + 1)
            } else {
                (list, len)
            }
        },
    );
    let both = &both[..len];
    let pool = s.player(player).mana().amounts_u8();
    (0u32..1 << len).any(|tapped| {
        let mut out = exclude;
        let mut spare = spare;
        for (i, &slot) in both.iter().enumerate() {
            if tapped & (1 << i) != 0 {
                out.insert(slot);
                spare.add(kinds(slot), 1);
            }
        }
        let fewer;
        let sources = if tapped == 0 {
            &all
        } else {
            let Ok(found) = Sources::of(s, t, player, out, paying) else {
                return true;
            };
            fewer = found;
            &fewer
        };
        pays_from_state_with(
            pool,
            sources,
            &Table::filled(0),
            0,
            &[0; CLASSES],
            None,
            cost,
            spare,
        )
    })
}
