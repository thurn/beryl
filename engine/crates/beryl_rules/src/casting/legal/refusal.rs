//! Why a player may not cast a card at priority now, which the client shows when the card is
//! clicked: the way of casting it that passes the most of the checks the priority options make.
//!
//! Implements: none. Reruns the checks of the casting rules (CR 601.3) to name the one a card fails.

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, ObjectAbility, Types};
use crate::continuous::rule_change;
use crate::decision::Refusal;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{RuleKind, State};
use crate::table::GameTable;

use super::super::costs::From;
use super::super::timing::instant_speed;
use super::activation::activatable;
use super::ways::Ways;
use super::{Situation, preparation, refused, special};

/// Keeps in `most` whichever of it and `r` got further through [`refused`]'s checks, which
/// run in the order of `progress`.
pub(super) fn keep_furthest<L: Layout>(most: &mut Option<Refusal<L>>, r: Refusal<L>) {
    let progress = |r| match r {
        Refusal::Timing { .. }
        | Refusal::LandPlayed
        | Refusal::Tapped
        | Refusal::SummoningSickness
        | Refusal::CantAttack
        | Refusal::DefenderNeeds(_) => 0,
        Refusal::Mana => 1,
        Refusal::Cost => 2,
        Refusal::Target => 3,
    };
    if most.is_none_or(|m| progress(r) > progress(m)) {
        *most = Some(r);
    }
}

/// Why `player` may not cast the card in `slot` at priority now: the refusal of the way of
/// casting it, by face and casting options, that passes the most checks, or `land`, why they
/// may not play it as a land, if that passes as many. None if one way is legal, if they may
/// activate one of its abilities where it lies (cycling from a hand), or if the card has no
/// face to cast and `land` is none.
#[allow(
    clippy::disallowed_methods,
    reason = "reads each face the card may be cast as"
)]
pub(crate) fn refusal<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    land: Option<Refusal<L>>,
) -> Result<Option<Refusal<L>>, Full> {
    let now = Situation::of(player);
    if activates_now(s, t, &now, slot) {
        return Ok(None);
    }
    let Some(def) = s.card_def(t, slot) else {
        return if s.prepare_spells_for(player).contains(slot) {
            preparation::refusal(s, t, &now, slot)
        } else {
            Ok(land)
        };
    };
    let ways = Ways::of(s, t, player, slot, From::Priority);
    let mut most = land;
    for f in def.face_ixs() {
        let face = def.face(f);
        if !face.playable() || face.types.intersects(Types::LAND) {
            continue;
        }
        let ways = if ways.on_adventure && f == FaceIx::BACK {
            ways.back(s, player, slot)
        } else {
            ways
        };
        for costs in ways.option_sets(face) {
            // A cast for escape is under no permission's terms.
            let increase = if costs.escapes() { 0 } else { ways.increase };
            let Some(r) = refused(s, t, &now, slot, f, costs, From::Priority, increase) else {
                return Ok(None);
            };
            keep_furthest(&mut most, r);
        }
    }
    for (face, costs) in special::casts(s, t, player, slot, def, ways) {
        let Some(r) = refused(s, t, &now, slot, face, costs, From::Priority, ways.increase) else {
            return Ok(None);
        };
        keep_furthest(&mut most, r);
    }
    // Only sorcery speed, where a face could be cast at instant speed but for the limit.
    if most == Some(Refusal::Timing { by: None }) {
        let instant = (def.face_ixs().map(|f| def.face(f)))
            .find(|face| face.playable() && instant_speed(s, t, face, player));
        if let Some(face) = instant {
            let limit = RuleKind::CastOnlyAsSorcery;
            most = Some(Refusal::Timing {
                by: rule_change::maker(s, t, limit, player, face)
                    .and_then(|slot| s.permanent_at(slot)),
            });
        }
    }
    now.complete()?;
    Ok(most)
}

/// Whether the priority holder may activate one of the abilities of the object in `slot` now,
/// as [`activation_options`] would offer it: a card refused every way of being cast or played
/// may still be clicked for this. see CR 117.1b: "A player may activate an activated ability
/// any time they have priority."
fn activates_now<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L>,
    slot: L::Slot,
) -> bool {
    let facts = s.definition(slot).facts(t);
    if !facts.activates() {
        return false;
    }
    let prohibited = rule_change::prohibitions(s, t, now.player);
    if !prohibited.is_empty()
        && rule_change::unprohibited(s, t, prohibited, now.player, Bitboard::single(slot))
            .is_empty()
    {
        return false;
    }
    let lost = !s.has_every_ability(slot);
    (facts.activated().map(ObjectAbility::Own))
        .any(|ability| (!lost || s.has(slot, ability)) && activatable(s, t, now, slot, ability))
}
