//! When a player may cast a spell or activate an ability: sorcery and instant speed
//! (CR 307.1, see CR 117.1a), and which ability of a permanent is activated.
//!
//! Implements: CR 307.1

use crate::card::{Face, FaceAbility, Keywords, ObjectAbility, Types};
use crate::continuous::rule_change;
use crate::definition::Definition;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{State, TurnStep, Zone};
use crate::table::GameTable;

/// The card, face and ability a stack entry records for activating ability `ability` of the
/// permanent in `slot`: one of its own, one granted to it, which is printed on the
/// granting card's front face (§9), or one of a card under it, if it is a merged permanent,
/// printed on that card's face (see [CR 702.140e]): "A mutated permanent has all abilities of
/// each card and token that represents it."
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
pub(super) fn activated<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Option<(Definition<L>, FaceAbility)> {
    match ability {
        ObjectAbility::Own(i) => Some((s.definition(slot), FaceAbility::Own(i))),
        ObjectAbility::Granted(k) => {
            if s.object(slot).zone() != Zone::Battlefield {
                return None;
            }
            let grant = s.granted_ability(slot, k)?;
            let granter = Definition::of_card(grant.card);
            Some((granter, FaceAbility::Grant(grant.grant)))
        }
        ObjectAbility::Merged(..) => merged(s, t, slot, ability),
    }
}

/// [`activated`] for an ability of a card under a merged permanent. Out of line: only a game
/// with a card that merges offers one.
#[cold]
#[inline(never)]
fn merged<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Option<(Definition<L>, FaceAbility)> {
    let (card, face, index, ability) = s.merged_ability(t, slot, ability)?;
    crate::objects::designation::available(s, slot, ability)
        .then_some((Definition::of_face(card, face), FaceAbility::Own(index)))
}

/// Whether `player` may cast a sorcery now
/// ([CR 307.1](https://yawgatog.com/resources/magic-rules/#R3071)): "A player who has
/// priority may cast a sorcery card from their hand during a main phase of their turn when the
/// stack is empty."
pub(crate) fn sorcery_timing<L: Layout>(s: &State<L>, player: Seat) -> bool {
    player == s.turn().active()
        && matches!(
            s.turn().step(),
            TurnStep::PrecombatMain | TurnStep::PostcombatMain
        )
        && s.stack().is_empty()
}

/// Whether `player` may cast this face at instant speed: an instant
/// (see [CR 117.1a](https://yawgatog.com/resources/magic-rules/#R1171a)), a face with flash
/// (see [CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)), or a sorcery while a
/// rule change lets them cast sorceries as though they had flash.
pub(super) fn instant_speed<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    face: &Face,
    player: Seat,
) -> bool {
    face.types.intersects(Types::INSTANT)
        || face.keywords.contains(Keywords::FLASH)
        || rule_change::cast_as_instant(s, t, player, face)
}
