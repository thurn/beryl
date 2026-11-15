//! Trigger matching off the main path of [`super::match_event`], out of line: the abilities
//! of objects outside the battlefield that work where they are, as events happen and as
//! permanents leave, and the triggered abilities that copy exceptions give the permanents that
//! are copies.

use super::{ability, about, away, enqueue, fires, times};
use crate::card::FaceAbility;
use crate::event::{Event, EventKind};
use crate::fixed::Full;
use crate::layout::Layout;
use crate::state::{Flag, State, TriggerInfo, TriggeredAbility, Zone};
use crate::table::GameTable;
use beryl_kernel::listener;

/// Matches `e`, an event of kind `k`, against the triggered abilities that copy exceptions give
/// the permanents that are copies ([`Copiable::given_abilities`]), in the order of the
/// battlefield ([CR 707.9a]): "This ability becomes part of the copiable values for the copy,
/// along with any other abilities that were copied." No face of a copy's definition lists such
/// an ability, so the table's listeners do not hold it. Out of line: only a game with a card
/// whose exception gives one looks ([`Capability::GivesTriggers`]).
///
/// What a permanent lost is kept by the index of its face's own abilities
/// ([`State::has_ability`]), which has no place for these, so an effect that removes abilities
/// does not remove them yet: no card of the pool removes abilities from a copy.
///
/// [CR 707.9a]: https://yawgatog.com/resources/magic-rules/#R7079a
#[cold]
#[inline(never)]
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
        let source = s.object_ref(slot);
        let controller = s.controller(slot);
        for (card, face, grant) in values.given_abilities(t) {
            let a = ability(t, card, face, FaceAbility::Grant(grant));
            let Some(trigger) = a.trigger() else {
                continue;
            };
            let listens = listener::listens_to(trigger, a.zones());
            if !matches!(listens, Ok(Some(kind)) if kind == k) {
                continue;
            }
            if let Some(info) = fires(s, t, trigger, Some(slot), controller, e) {
                let given = TriggeredAbility::of(FaceAbility::Grant(grant));
                for _ in 0..times(e) {
                    enqueue(s, t, Some(source), card, face, given, controller, info)?;
                }
            }
        }
    }
    Ok(())
}

/// Matches `e`, an event of kind `k`, against the abilities of the objects outside the battlefield
/// that work where they are ([`away`]). Out of line: most games have none.
#[cold]
#[inline(never)]
pub(super) fn match_away<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    k: EventKind,
    e: &Event<L>,
) -> Result<(), Full> {
    for slot in s.in_candidate_order(away(s, t)) {
        let zone = s.object(slot).zone();
        let source = s.object_ref(slot);
        let controller = s.controller(slot);
        let (card, face) = s.definition(slot).record(t);
        for l in t.away_listeners(card, face, k) {
            if !l.zones.contains(zone) || !s.has_ability(slot, l.ability) {
                continue;
            }
            if let Some(info) = fires(s, t, l.trigger, Some(slot), controller, e) {
                let ability = TriggeredAbility::own(l.ability);
                for _ in 0..times(e) {
                    enqueue(s, t, Some(source), card, face, ability, controller, info)?;
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
    let away = away(s, t);
    for (left, to) in leaving {
        let object = s.object_ref(left);
        let waiting = s.triggers().len();
        for slot in s.in_candidate_order(away) {
            let zone = s.object(slot).zone();
            let source = s.object_ref(slot);
            let controller = s.controller(slot);
            let words = s.words_of(slot);
            let (card, face) = s.definition(slot).record(t);
            let info = TriggerInfo {
                object: Some(object),
                player: s.controller(left),
                amount: 0,
            };
            // CR 700.4: dying is going to a graveyard.
            let kinds = [
                (EventKind::Dies, to == Zone::Graveyard),
                (EventKind::Leaves, true),
            ];
            for (kind, happened) in kinds {
                if !happened {
                    continue;
                }
                for l in t.away_listeners(card, face, kind) {
                    if l.zones.contains(zone)
                        && s.has_ability(slot, l.ability)
                        && about(
                            s,
                            t,
                            l.trigger.subject,
                            Some(slot),
                            controller,
                            &words,
                            object,
                        )
                    {
                        enqueue(
                            s,
                            t,
                            Some(source),
                            card,
                            face,
                            TriggeredAbility::own(l.ability),
                            controller,
                            info,
                        )?;
                    }
                }
            }
        }
        if s.triggers().len() > waiting {
            s.record_last_known(left)?;
        }
    }
    Ok(())
}
