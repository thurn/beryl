//! Declaring and blocking attacking bands, and applying banding to combat damage.
//!
//! Implements: CR 702.22c, CR 702.22d, CR 702.22g, CR 702.22h, CR 702.22j, CR 702.22k

use super::{declare_attacker, has};
use crate::bitboard::Bitboard;
use crate::card::Keywords;
use crate::decision::{OPTIONS, Opt};
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::state::{Memo, Pending, State};
use crate::table::GameTable;
use beryl_kernel::reference::Permanent;

/// Offers a new band for each bander and legal ways to join bands already declared.
/// Starting with a bander can express every legal group. An attacker is picked once.
///
/// Under [CR 702.22c], "As a player declares attackers, they may declare that one or more attacking
/// creatures with banding and up to one attacking creature without banding ... are all in a
/// 'band.'" Also, "each creature may be a member of only one of them."
///
/// [CR 702.22c]: https://yawgatog.com/resources/magic-rules/#R70222c
#[cold]
#[inline(never)]
pub(super) fn append_attack_options<L: Layout>(
    s: &State<L>,
    attackers: Bitboard<L::Slot>,
    list: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let mut bands = Bitboard::EMPTY;
    let mut with_nonbander = Bitboard::EMPTY;
    for (member, band) in s.band_members() {
        bands.insert(band);
        if !has(s, member, Keywords::BANDING) {
            with_nonbander.insert(band);
        }
    }
    for slot in s
        .battlefield()
        .iter()
        .copied()
        .filter(|&slot| attackers.contains(slot))
    {
        let joinable = if has(s, slot, Keywords::BANDING) {
            list.push(Opt::Attack {
                slot,
                band: Some(slot),
            })?;
            bands
        } else {
            bands & !with_nonbander
        };
        for first in s
            .battlefield()
            .iter()
            .copied()
            .filter(|&first| joinable.contains(first))
        {
            list.push(Opt::Attack {
                slot,
                band: Some(first),
            })?;
        }
    }
    Ok(())
}

/// Joins the band and attacks its target. A new band's first member chooses its target.
///
/// Under [CR 702.22d], "All creatures in an attacking band must attack the same player, planeswalker,
/// or battle."
///
/// [CR 702.22d]: https://yawgatog.com/resources/magic-rules/#R70222d
#[cold]
#[inline(never)]
pub(super) fn attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    attacker: Permanent<L>,
    first: L::Slot,
) -> Result<(), Full> {
    s.remember(attacker.slot(), Memo::CombatBand(first))?;
    declare_attacker(s, t, attacker);
    let aiming = if first == attacker.slot() {
        Some(first)
    } else {
        let target = s
            .object(first)
            .attack_target()
            .expect("a band has an attack target");
        s.choose_attacked(attacker, target);
        None
    };
    s.ask(Pending::DeclareAttackers { attacker: aiming });
    Ok(())
}

/// Applies the declared block to the other band members. Only the declared block must pass
/// the evasion checks. Members keep their own abilities and keep the band after losing banding.
///
/// See [CR 702.22e]: "Once an attacking band has been announced, it lasts for the rest of combat,
/// even if something later removes banding or 'bands with other' from one or more of the
/// creatures in the band."
/// Under [CR 702.22g], "Banding doesn't cause attacking creatures to share abilities, nor does it
/// remove any abilities."
/// Under [CR 702.22h], "If an attacking creature becomes blocked by a creature, each other creature in
/// the same band as the attacking creature becomes blocked by that same blocking creature."
///
/// [CR 702.22e]: https://yawgatog.com/resources/magic-rules/#R70222e
/// [CR 702.22g]: https://yawgatog.com/resources/magic-rules/#R70222g
/// [CR 702.22h]: https://yawgatog.com/resources/magic-rules/#R70222h
#[cold]
#[inline(never)]
pub(super) fn spread_block<L: Layout>(
    s: &mut State<L>,
    blocker: Permanent<L>,
    attacker: Permanent<L>,
) -> Result<(), Full> {
    let Some(band) = s.combat_band(attacker.slot()) else {
        return Ok(());
    };
    for index in 0..s.battlefield().len() {
        let member = s.battlefield()[index];
        if member != attacker.slot() && s.combat_band(member) == Some(band) {
            let member = s.permanent_at(member).expect("a band member");
            s.block(blocker, member)?;
        }
    }
    Ok(())
}

/// A band still matters after its members lose banding, for example when two of its
/// blocked attackers together assign enough damage for trample.
#[cold]
#[inline(never)]
pub(super) fn affects_damage<L: Layout>(s: &State<L>) -> bool {
    s.band_members().next().is_some()
        || s.battlefield()
            .iter()
            .any(|&slot| super::in_combat_now(s, slot) && has(s, slot, Keywords::BANDING))
}

/// The player who assigns this creature's damage because one of its opponents in
/// combat has banding. A creature need not belong to an attacking band to do this.
///
/// Under [CR 702.22j], "if an attacking creature is being blocked by a creature with banding, ...
/// the defending player (rather than the active player) chooses how the attacking
/// creature's damage is assigned."
/// Under [CR 702.22k], "if a blocking creature is blocking a creature with banding, ...
/// the active player (rather than the defending player) chooses how the blocking creature's
/// damage is assigned."
///
/// [CR 702.22j]: https://yawgatog.com/resources/magic-rules/#R70222j
/// [CR 702.22k]: https://yawgatog.com/resources/magic-rules/#R70222k
#[cold]
#[inline(never)]
pub(super) fn assigner<L: Layout>(s: &State<L>, source: L::Slot) -> Option<crate::ids::Seat> {
    let banders =
        super::assignment::recipients(s, source).filter(|slot| has(s, slot, Keywords::BANDING));
    s.players_in_turn_order(s.defending_players())
        .find(|&seat| banders.any(|slot| s.controller(slot) == seat))
        .or_else(|| (!banders.is_empty()).then_some(s.turn().active()))
}
