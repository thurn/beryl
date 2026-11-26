//! Trigger matching off the main path of [`super::match_event`], out of line: the abilities
//! of objects outside the battlefield that work where they are, as events happen and as
//! permanents leave, the triggered abilities that copy exceptions give the permanents that
//! are copies, and those of the cards under a merged permanent.
//!
//! Implements: none. Matches triggers under the rules declared by its parent module.

use super::{
    ability, away, away_controllers, fire, keep_last_known, look_back, look_back_kinds, of_object,
};
use crate::card::FaceAbility;
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{Event, EventKind};
use crate::state::{AbilityRef, Flag, State, TriggeredAbility, Zone};
use crate::table::GameTable;
use beryl_kernel::listener;

/// Matches `e`, an event of kind `k`, against the triggered abilities that copy exceptions give
/// the permanents that are copies ([`Copiable::given_abilities`]), in the order of the
/// battlefield (see [CR 707.9a]): "This ability becomes part of the copiable values for the copy,
/// along with any other abilities that were copied." No face of a copy's definition lists such
/// an ability, so the table's listeners do not hold it. Out of line: only a game with a card
/// whose exception gives one looks ([`Capability::GivesTriggers`]).
///
/// It matches the triggered abilities of the faces of the cards under a merged permanent too
/// ([`Copiable::merged_faces`]), from the table's listeners of those faces (see [CR 702.140e]): "A
/// mutated permanent has all abilities of each card and token that represents it." Only a game
/// with mutate has them ([`Capability::Merges`]).
///
/// What a permanent lost is kept by the index of its face's own abilities
/// ([`State::has_ability`]), which has no place for these, so an effect that removes abilities
/// does not remove them yet: no card of the pool removes abilities from a copy or a merged
/// permanent.
///
/// [CR 707.9a]: https://yawgatog.com/resources/magic-rules/#R7079a
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
#[cold]
#[inline(never)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads each card under a merged permanent by the face it has there"
)]
pub(super) fn match_given<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    k: EventKind,
    e: &Event<L>,
) -> Result<(), Full> {
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        if !s.object(slot).flags().has(Flag::Copy) {
            continue;
        }
        let values = s.copiable(slot);
        for (card, face, grant) in values.given_abilities(t) {
            let given = TriggeredAbility::of(FaceAbility::Grant(grant));
            let of = of_object(s, slot, card, face, given);
            let a = ability(t, &of);
            let Some(trigger) = a.trigger() else {
                continue;
            };
            let listens = listener::listens_to(trigger, a.zones());
            if matches!(listens, Ok(Some(kind)) if kind == k) {
                fire(s, t, trigger, of, e)?;
            }
        }
        for (card, face) in values.merged_faces(t) {
            let of = of_object(s, slot, card, face, TriggeredAbility::default());
            for l in t.listeners(card, face, k) {
                let ability_def = &t.def(card).face(face).abilities[l.ability.index()];
                if crate::objects::designation::available(s, slot, ability_def) {
                    let ability = TriggeredAbility::own(l.ability);
                    fire(s, t, l.trigger, AbilityRef { ability, ..of }, e)?;
                }
            }
        }
    }
    Ok(())
}

/// [`super::match_look_back`] for the triggered abilities of the cards under a merged
/// permanent ([`Copiable::merged_faces`]), as the permanent `left` leaves for `to`: a card
/// under it that says "when this creature dies" sees the permanent die, once, however many
/// cards it is made of (see [CR 731.3]): "If a merged permanent leaves the battlefield, one
/// permanent leaves the battlefield". As the permanent is now, just before it leaves (CR 603.10a).
/// Out of line: only a game with mutate looks ([`Capability::Merges`]).
///
/// [CR 731.3]: https://yawgatog.com/resources/magic-rules/#R7313
#[cold]
#[inline(never)]
pub(super) fn look_back_merged<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    left: L::Slot,
    to: Zone,
) -> Result<(), Full> {
    let object = s.object_ref(left);
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        if !s.object(slot).flags().has(Flag::Copy) {
            continue;
        }
        let values = s.copiable(slot);
        let words = s.words_of(slot);
        for (card, face) in values.merged_faces(t) {
            let of = of_object(s, slot, card, face, TriggeredAbility::default());
            for kind in look_back_kinds(to) {
                for l in t.listeners(card, face, kind) {
                    let ability = TriggeredAbility::own(l.ability);
                    look_back(
                        s,
                        t,
                        l.trigger.subject,
                        AbilityRef { ability, ..of },
                        &words,
                        object,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Matches `e`, an event of kind `k`, against the abilities of the objects outside the battlefield
/// that work where they are ([`away`]), once for each of the players who control them
/// ([`away_controllers`]). Out of line: most games have none.
#[cold]
#[inline(never)]
pub(super) fn match_away<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    k: EventKind,
    e: &Event<L>,
) -> Result<(), Full> {
    for &slot in s.in_game_order(t, away(s, t))?.iter() {
        let zone = s.object(slot).zone();
        let (card, face) = s.definition(slot).record(t);
        let of = of_object(s, slot, card, face, TriggeredAbility::default());
        for controller in away_controllers(s, slot).in_seat_order() {
            let controller = controller.into();
            for l in t.away_listeners(card, face, k) {
                if l.zones.contains(zone) && s.has_ability(slot, l.ability) {
                    let ability = TriggeredAbility::own(l.ability);
                    let of = AbilityRef {
                        ability,
                        controller,
                        ..of
                    };
                    fire(s, t, l.trigger, of, e)?;
                }
            }
        }
    }
    Ok(())
}

/// [`super::match_look_back`] for the abilities of objects outside the battlefield that work where they
/// are ([`away`]): a card in a graveyard that says "whenever a creature dies" sees each creature of
/// the batch die. Out of line: most games have none.
#[cold]
#[inline(never)]
pub(super) fn look_back_away<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    leaving: impl Iterator<Item = (L::Slot, Zone)>,
) -> Result<(), Full> {
    let away = s.in_game_order(t, away(s, t))?;
    for (left, to) in leaving {
        let object = s.object_ref(left);
        let waiting = s.triggers().len();
        for &slot in away.iter() {
            let zone = s.object(slot).zone();
            let words = s.words_of(slot);
            let (card, face) = s.definition(slot).record(t);
            let of = of_object(s, slot, card, face, TriggeredAbility::default());
            for controller in away_controllers(s, slot).in_seat_order() {
                let controller = controller.into();
                for kind in look_back_kinds(to) {
                    for l in t.away_listeners(card, face, kind) {
                        if l.zones.contains(zone) && s.has_ability(slot, l.ability) {
                            let ability = TriggeredAbility::own(l.ability);
                            let of = AbilityRef {
                                ability,
                                controller,
                                ..of
                            };
                            look_back(s, t, l.trigger.subject, of, &words, object)?;
                        }
                    }
                }
            }
        }
        keep_last_known(s, object, waiting)?;
    }
    Ok(())
}
