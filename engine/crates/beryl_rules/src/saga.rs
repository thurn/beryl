//! Saga chapters, lore progression and the final sacrifice.
//!
//! Implements: CR 715.2b, CR 715.2c, CR 715.2d, CR 715.3a, CR 715.3c, CR 715.4

use crate::bitboard::Bitboard;
use crate::card::{FaceAbility, FaceIx, ObjectAbility, Subtype, Types};
use crate::continuous::layers;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::query::Copiable;
use crate::replacement::event::{Entry, Sink};
use crate::stack::trigger;
use crate::state::{
    AbilityMask, AbilityRef, CounterKind, ObjectRef, PendingTrigger, State, TriggerInfo,
    TriggeredAbility,
};
use crate::table::{Capability, GameTable};
use beryl_kernel::reference::Permanent;

type Chapter<L> = (<L as Layout>::Card, FaceIx, TriggeredAbility, (u8, u8));

/// The chapter abilities currently on a permanent, including abilities supplied by the layers.
fn chapters<'a, L: Layout>(
    s: &'a State<L>,
    t: &'a GameTable,
    slot: L::Slot,
) -> impl Iterator<Item = Chapter<L>> + 'a {
    let (card, face) = s.definition(slot).record(t);
    let ordinary = s.abilities(t, slot).filter_map(move |(source, ability)| {
        let numbers = ability.chapter_numbers()?;
        let (card, face, index) = match source {
            ObjectAbility::Own(index) => (card, face, FaceAbility::Own(index)),
            ObjectAbility::Merged(..) => {
                let (card, face, index, _) = s.merged_ability(t, slot, source)?;
                (card, face, FaceAbility::Own(index))
            }
            ObjectAbility::Granted(index) => {
                let grant = s.derived(slot).granted[usize::from(index)]?;
                (grant.card, FaceIx::FRONT, FaceAbility::Grant(grant.grant))
            }
        };
        Some((card, face, TriggeredAbility::of(index), numbers))
    });
    let exceptions = (t.can(Capability::GivesTriggers) && s.keeps_merged(slot))
        .then(|| s.copiable(slot))
        .into_iter()
        .flat_map(|values| values.given_abilities(t))
        .filter_map(move |(card, face, index)| {
            let index = FaceAbility::Grant(index);
            let numbers = t.def(card).face(face).ability(index).chapter_numbers()?;
            Some((card, face, TriggeredAbility::of(index), numbers))
        });
    ordinary.chain(exceptions)
}

/// Matches every chapter crossed by one lore placement, once per crossed number. A range
/// shares one body in the definition but represents a separate trigger for each number.
///
/// ([CR 715.2b]): “‘{rN}—[Effect]’ means ‘When one or more lore counters are put onto this Saga,
/// if the number of lore counters on it was less than N and became at least N, [effect].’”
///
/// ([CR 715.2c]): “‘{rN1}, {rN2}—[Effect]’ means the same as ‘{rN1}—[Effect]’ and
/// ‘{rN2}—[Effect].’”
///
/// [CR 715.2b]: https://yawgatog.com/resources/magic-rules/#R7152b
/// [CR 715.2c]: https://yawgatog.com/resources/magic-rules/#R7152c
#[cold]
#[inline(never)]
pub(crate) fn match_lore<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    slot: L::Slot,
    before: u8,
) -> Result<(), Full> {
    let after = s.counters_of(slot, CounterKind::Lore);
    if before >= after {
        return Ok(());
    }
    let source = Some(s.object_ref(slot));
    let controller = s.controller(slot);
    let mut matching: L::Queue<Chapter<L>, 64> = Default::default();
    for chapter @ (_, _, _, (first, last)) in chapters(s, t, slot) {
        if before < last && after >= first {
            matching.push(chapter)?;
        }
    }
    for &(card, face, ability, (first, last)) in matching.as_slice() {
        for _ in first.max(before + 1)..=last.min(after) {
            let info = TriggerInfo {
                object: source,
                player: controller,
                amount: 0,
            };
            let of = trigger::of_object(s, slot, card, face, ability);
            trigger::enqueue(s, t, PendingTrigger { of, info })?;
        }
    }
    Ok(())
}

/// Places the first lore counter as the Saga enters (see CR 715.3a).
#[cold]
#[inline(never)]
pub(crate) fn place_entry_lore<L: Layout>(
    s: &mut State<L>,
    permanent: Permanent<L>,
) -> Result<(), Full> {
    let object = s.current(ObjectRef::from(permanent)).expect("just arrived");
    let _ = s.add_counters(object, CounterKind::Lore, 1, false)?;
    Ok(())
}

/// Matches chapters from the lore permanents enter with (see [CR 122.6]):
/// "an object that's given counters as it enters the battlefield."
///
/// [CR 122.6]: https://yawgatog.com/resources/magic-rules/#R1226
#[cold]
pub(crate) fn match_arrivals<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    arrived: &[L::Slot],
) -> Result<(), Full> {
    for &slot in arrived {
        match_lore(s, t, slot, 0)?;
    }
    Ok(())
}

/// Adds lore to the active player's Sagas as one event. Match the triggers after all of
/// the counters are present, so every chapter sees the same battlefield.
///
/// ([CR 715.3c]): “As a player’s precombat main phase begins, that player puts a lore counter
/// on each Saga they control with one or more chapter abilities. This turn-based action
/// doesn’t use the stack.”
///
/// [CR 715.3c]: https://yawgatog.com/resources/magic-rules/#R7153c
#[cold]
#[inline(never)]
pub(crate) fn advance<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    active: Seat,
) -> Result<(), Full> {
    let saga = t.subtypes().has(Subtype::Saga);
    let mut adding = Bitboard::<L::Slot>::EMPTY;
    for (slot, derived) in s.permanents_with_derived() {
        if derived.controller == active
            && derived.types.contains(Types::ENCHANTMENT)
            && saga.of(derived.subtypes)
            && chapters(s, t, slot).next().is_some()
            && s.counters_of(slot, CounterKind::Lore) < u8::MAX
        {
            adding.insert(slot);
        }
    }
    if adding.is_empty() {
        return Ok(());
    }
    let ordered = s.in_game_order(t, adding)?;
    sink.together();
    for &slot in ordered.as_slice() {
        let object = s
            .current(s.object_ref(slot))
            .expect("Saga on the battlefield");
        let layered = s.layers_read_counters(t, slot, CounterKind::Lore);
        sink.push(s.add_counters(object, CounterKind::Lore, 1, layered)?);
    }
    layers::refresh(s, t, sink)?;
    for &slot in ordered.as_slice() {
        let before = s.counters_of(slot, CounterKind::Lore) - 1;
        match_lore(s, t, slot, before)?;
    }
    sink.apart();
    Ok(())
}

/// Sagas that must be sacrificed in the current state-based action batch. The last chapter
/// is read from current abilities. Waiting and stacked triggers are read from the definitions
/// recorded when they triggered, so a change in characteristics cannot erase them.
///
/// ([CR 715.2d]): “A Saga’s final chapter number is the greatest value among chapter abilities
/// it has. If a Saga somehow has no chapter abilities, its final chapter number is 0.”
///
/// ([CR 715.4]): “If the number of lore counters on a Saga permanent with one or more chapter
/// abilities is greater than or equal to its final chapter number, and it isn’t the source
/// of a chapter ability that has triggered but not yet left the stack, that Saga’s controller
/// sacrifices it. This state-based action doesn’t use the stack.”
///
/// [CR 715.2d]: https://yawgatog.com/resources/magic-rules/#R7152d
/// [CR 715.4]: https://yawgatog.com/resources/magic-rules/#R7154
#[cold]
#[inline(never)]
pub(crate) fn completed<L: Layout>(s: &State<L>, t: &GameTable) -> Bitboard<L::Slot> {
    let saga = t.subtypes().has(Subtype::Saga);
    s.permanents_with_derived()
        .filter_map(|(slot, d)| {
            if !saga.of(d.subtypes) {
                return None;
            }
            let final_chapter = chapters(s, t, slot)
                .map(|(_, _, _, (_, last))| last)
                .max()?;
            if s.counters_of(slot, CounterKind::Lore) < final_chapter {
                return None;
            }
            let source = Some(s.object_ref(slot));
            let chapter = |of: &AbilityRef<L, TriggeredAbility>| {
                of.source == source && trigger::ability(t, of).chapter_numbers().is_some()
            };
            (!trigger::waiting_or_on_stack(s, chapter)).then_some(slot)
        })
        .collect()
}

/// The intrinsic entry replacement of a Saga without read ahead (see [CR 614.12] for
/// checking the characteristics it would have on the battlefield).
///
/// ([CR 715.3a]): "Each Saga without read ahead has the intrinsic ability ‘This Saga enters with
/// a lore counter on it.’ This ability creates a replacement effect (see rule 614.1c)."
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
/// [CR 715.3a]: https://yawgatog.com/resources/magic-rules/#R7153a
#[cold]
pub(crate) fn entering<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: &mut Entry<L>,
    copied: Option<Copiable<L>>,
) -> Result<(), Full> {
    let saga = t.subtypes().has(Subtype::Saga);
    if saga.possible() {
        let permanent = layers::entering::as_it_would_exist(s, t, slot, *entry, copied)?;
        if saga.of(permanent.subtypes) && { permanent.lost } != L::AbilityMask::ALL {
            entry.saga_lore = true;
        }
    }
    Ok(())
}
