//! Declaring and blocking attacking bands, and applying banding to combat damage.
//!
//! Implements: CR 702.22b, CR 702.22c, CR 702.22d, CR 702.22g, CR 702.22h, CR 702.22j, CR 702.22k

use super::{declare_attacker, has};
use crate::bitboard::Bitboard;
use crate::card::Keywords;
use crate::decision::{OPTIONS, Opt};
use crate::filter::{Evaluate, Filter};
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::state::{Memo, Pending, State};
use crate::table::GameTable;
use beryl_kernel::reference::Permanent;

/// Offers a new band for each creature that can start one, and each legal way to join a band
/// already declared. A band starts with a creature with banding, or with a creature with
/// "bands with other [quality]" that has the quality, and every legal band can be declared
/// in that order. An attacker is picked once.
///
/// Under [CR 702.22c], "As a player declares attackers, they may declare that one or more attacking
/// creatures with banding and up to one attacking creature without banding (even if it has
/// “bands with other”) are all in a “band.” They may also declare that one or more attacking
/// [quality] creatures with “bands with other [quality]” and any number of other attacking
/// [quality] creatures are all in a band." Also, "each creature may be a member of only one of
/// them."
///
/// [CR 702.22c]: https://yawgatog.com/resources/magic-rules/#R70222c
#[cold]
#[inline(never)]
pub(super) fn append_attack_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    attackers: Bitboard<L::Slot>,
    list: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let mut bands = Bitboard::EMPTY;
    for (_, band) in s.band_members() {
        bands.insert(band);
    }
    for slot in s
        .battlefield()
        .iter()
        .copied()
        .filter(|&slot| attackers.contains(slot))
    {
        if is_band(s, t, Bitboard::single(slot)) {
            list.push(Opt::Attack {
                slot,
                band: Some(slot),
            })?;
        }
        for first in s
            .battlefield()
            .iter()
            .copied()
            .filter(|&first| bands.contains(first))
        {
            let mut group = members(s, first);
            group.insert(slot);
            if is_band(s, t, group) {
                list.push(Opt::Attack {
                    slot,
                    band: Some(first),
                })?;
            }
        }
    }
    Ok(())
}

/// The declared members of the band whose first member is `first`.
fn members<L: Layout>(s: &State<L>, first: L::Slot) -> Bitboard<L::Slot> {
    s.band_members()
        .filter(|&(_, band)| band == first)
        .map(|(member, _)| member)
        .collect()
}

/// Whether `group` may be declared as one attacking band, under either of the two ways
/// [CR 702.22c] gives (see [`append_attack_options`]): creatures with banding and at most one
/// without, or creatures that all have a quality, one of which has "bands with other" that
/// quality.
///
/// [CR 702.22c]: https://yawgatog.com/resources/magic-rules/#R70222c
fn is_band<L: Layout>(s: &State<L>, t: &GameTable, group: Bitboard<L::Slot>) -> bool {
    let without_banding = group.filter(|slot| !has(s, slot, Keywords::BANDING));
    (without_banding.count() < group.count() && without_banding.count() <= 1)
        || group.any(|member| {
            bands_with(s, t, member)
                .any(|quality| group.all(|other| has_quality(s, t, quality, member, other)))
        })
}

/// The qualities of the "bands with other" abilities the permanent in `slot` has. One it has
/// lost, as to an effect that makes it lose all its abilities, is not among them.
///
/// Under [CR 702.22b], "If an effect causes a permanent to lose banding, the permanent loses all
/// “bands with other” abilities as well."
///
/// [CR 702.22b]: https://yawgatog.com/resources/magic-rules/#R70222b
fn bands_with<'s, L: Layout>(
    s: &'s State<L>,
    t: &'s GameTable,
    slot: L::Slot,
) -> impl Iterator<Item = &'static Filter> + 's {
    s.abilities(t, slot)
        .filter_map(|(_, ability)| ability.bands_with())
}

/// Whether `other` has the quality of a "bands with other" ability of `holder`, read for its
/// controller through its text changes.
fn has_quality<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    quality: &Filter,
    holder: L::Slot,
    other: L::Slot,
) -> bool {
    quality.matches(s, t, other, s.controller(holder), &s.words_of(holder))
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
pub(super) fn affects_damage<L: Layout>(s: &State<L>, t: &GameTable) -> bool {
    s.band_members().next().is_some()
        || s.battlefield().iter().any(|&slot| {
            super::in_combat_now(s, slot)
                && (has(s, slot, Keywords::BANDING) || bands_with(s, t, slot).next().is_some())
        })
}

/// The player who assigns this creature's damage because the creatures it fights in combat
/// use banding: one of them has banding, or one has "bands with other [quality]" and the
/// quality, and another has the quality. A creature need not belong to an attacking band to
/// do this.
///
/// Under [CR 702.22j], "if an attacking creature is being blocked by a creature with banding, or by
/// both a [quality] creature with “bands with other [quality]” and another [quality] creature,
/// the defending player (rather than the active player) chooses how the attacking creature’s
/// damage is assigned."
/// Under [CR 702.22k], "if a blocking creature is blocking a creature with banding, or both a
/// [quality] creature with “bands with other [quality]” and another [quality] creature, the
/// active player (rather than the defending player) chooses how the blocking creature’s damage
/// is assigned."
///
/// [CR 702.22j]: https://yawgatog.com/resources/magic-rules/#R70222j
/// [CR 702.22k]: https://yawgatog.com/resources/magic-rules/#R70222k
#[cold]
#[inline(never)]
pub(super) fn assigner<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    source: L::Slot,
) -> Option<crate::ids::Seat> {
    let recipients = super::assignment::recipients(s, source);
    let banders = recipients.filter(|slot| {
        has(s, slot, Keywords::BANDING)
            || bands_with(s, t, slot).any(|quality| {
                has_quality(s, t, quality, slot, slot)
                    && recipients
                        .any(|other| other != slot && has_quality(s, t, quality, slot, other))
            })
    });
    s.players_in_turn_order(s.defending_players())
        .find(|&seat| banders.any(|slot| s.controller(slot) == seat))
        .or_else(|| (!banders.is_empty()).then_some(s.turn().active()))
}
