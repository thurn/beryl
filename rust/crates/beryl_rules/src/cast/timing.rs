//! When a player may cast a spell or activate an ability: sorcery and instant speed
//! (CR 307.1, CR 117.1a), and the ability a stack entry resolves.

use crate::card::{Ability, Face, FaceAbility, Keywords, ObjectAbility, Types};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::rule_change;
use crate::state::{StackEntry, StackItem, State, TurnStep, Zone};
use crate::table::GameTable;

/// The ability a stack entry resolves: an ability of its card's face, or a spell's spell
/// ability, as the casting options paid for it make it (an overloaded spell's). A permanent
/// spell with no targets has none.
pub(crate) fn entry_ability<L: Layout>(
    t: &GameTable,
    entry: &StackEntry<L>,
) -> Option<&'static Ability> {
    let face = t.def(entry.card()).face(entry.face());
    match entry.item {
        StackItem::Spell { costs, .. } => face.spell_ability_with(costs),
        StackItem::Activated { of } => Some(face.ability(of.ability)),
        StackItem::Triggered { of, .. } => Some(face.ability(FaceAbility::Own(of.ability))),
    }
}

/// The ability of a stack entry that is an ability, by its card, face and index.
pub(super) fn entry_ability_of<L: Layout>(
    t: &GameTable,
    entry: &StackEntry<L>,
) -> &'static Ability {
    entry_ability(t, entry).expect("an ability's entry")
}

/// The card, face and ability a stack entry records for activating ability `ability` of the
/// permanent in `slot`: one of its own, or one granted to it, which is printed on the
/// granting card's front face (§9).
pub(super) fn activated<L: Layout>(
    s: &State<L>,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Option<(crate::query::Copiable<L>, FaceAbility)> {
    match ability {
        ObjectAbility::Own(i) => Some((s.copiable(slot), FaceAbility::Own(i))),
        ObjectAbility::Granted(k) => {
            if s.object(slot).zone() != Zone::Battlefield {
                return None;
            }
            let grant = s.derived(slot).granted[usize::from(k)]?;
            let granter = crate::query::Copiable::of_card(grant.card);
            Some((granter, FaceAbility::Grant(grant.grant)))
        }
    }
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
/// ([CR 117.1a](https://yawgatog.com/resources/magic-rules/#R1171a)), a face with flash
/// ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)), or a sorcery while a
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
