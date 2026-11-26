//! The interpreter: what an ability's [`Behavior`] does to a game.
//!
//! A [`Behavior::Custom`] is a Rust body, called with the game and the resolution. A
//! [`Behavior::Data`] is effect data: [`run`] carries out its instructions with the
//! library's functions, and [`scene`] reads its expressions. The data is the same for every
//! layout; only the interpreter is compiled for each.
//!
//! Implements: CR 101.4, CR 608.2c

pub(crate) mod catalog;
pub mod choices;
mod named;
mod run;
mod scene;

use beryl_card::{Amount, Condition, Effect, Objects, Players, Zone};
use beryl_kernel::memos::LinkKind;

use crate::bodies::Bodies;
use crate::card::{Ability, Behavior, FaceIx, Flow, ManaColor, ObjectAbility, Stop};
use crate::continuous::layers;
use crate::filter::Filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::replacement::event::Sink;
use crate::resolution::Resolution;
use crate::state::TriggeredAbility;
use crate::state::{AbilityRef, PendingTrigger, State, Tracker, TriggerInfo, ZoneKey};
use crate::table::GameTable;

pub(crate) use run::{Run, no_primitive};
pub(crate) use scene::Outcome;
use scene::Registers;
pub(crate) use scene::Scene;

/// Resolves the ability `resolution` is of
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of the
/// spell or ability follows its instructions in the order written."
#[inline]
pub(crate) fn resolve<'a, L: Layout>(
    game: &mut Game<'a, L>,
    resolution: &mut Resolution<'a, L>,
) -> Flow {
    match resolution.ability.body() {
        Behavior::Custom(custom) => {
            let bodies = custom.get::<Bodies>().expect("a body made by custom_body");
            bodies.of::<L>()(game, resolution)
        }
        Behavior::Data(effect) => data(game, resolution, effect),
    }
}

/// Carries out an ability written as data. Out of line: most of what resolves in a game is
/// a permanent spell, which has no ability to run, or a mana ability, which
/// [`mana_ability`] does without coming here.
#[inline(never)]
fn data<'a, L: Layout>(
    game: &mut Game<'a, L>,
    resolution: &mut Resolution<'a, L>,
    effect: &'static Effect,
) -> Flow {
    Run::new(game, resolution).carry_out(effect)
}

/// Carries out the instructions of a replacement of draws in place of `player`'s draw, as a
/// resolution carries out instructions, with `player` as "that player"
/// (see [CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects that use the
/// word 'instead' are replacement effects." The card table holds only such replacements whose
/// body is data (`beryl_kernel::replacement::instead_of_drawing`).
pub(crate) fn instead_of_drawing<'a, L: Layout>(
    game: &mut Game<'a, L>,
    resolution: &mut Resolution<'a, L>,
    player: Seat,
) -> Flow {
    let Behavior::Data(effect) = resolution.ability.body() else {
        return Ok(());
    };
    Run::new(game, resolution)
        .for_player(player)
        .carry_out(effect)
}

/// Ends a resolution that came to `flow`, with a layer refresh for what follows. Returns
/// whether it paused at a question, or [`Full`] if a capacity ran out: the one place a
/// resolution's `Full` becomes the engine's (the crate's contract, "How a resolution stops").
#[inline]
pub(crate) fn finish<L: Layout>(game: &mut Game<'_, L>, flow: Flow) -> Result<bool, Full> {
    layers::refresh(game.state, game.table, game.sink)?;
    match flow {
        Ok(()) | Err(Stop::EndTurn | Stop::RestartGame) => Ok(false),
        Err(Stop::Pause) => Ok(true),
        Err(Stop::Full) => Err(Full),
    }
}

/// Carries out a mana ability whose costs are paid, making mana of the kind `color` its
/// activator chose (see [CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b)): "An
/// activated mana ability doesn't go on the stack, so it can't be targeted, countered, or
/// otherwise responded to. Rather, it resolves immediately after it is activated."
///
/// One whose whole effect is to add mana, as a land's is, needs none of what resolving an
/// ability sets up, so it is done here directly: the mana solver taps for these in every
/// payment. `activation` describes the ability for any other.
///
/// What it does is the effect of an ability of its source, where its costs were its
/// controller's doing ([`Cause::Object`](crate::replacement::event::Cause::Object)). The mana added
/// directly is tagged so as it is noted, which opens no scope.
#[inline]
pub(crate) fn mana_ability<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    activation: impl FnOnce() -> PendingTrigger<L>,
    ability: &'static Ability,
    color: ManaColor,
) -> Result<(), Full> {
    crate::coverage::hit(ability);
    if let Behavior::Data(&Effect::AddMana {
        player: Players::You,
        mana,
    }) = ability.body()
    {
        debug_assert!(mana.includes(color));
        let activation = activation();
        let (player, source) = (activation.of.controller.seat(), activation.of.source);
        let added = s.add_mana(player, source, color, mana.amount);
        sink.note_from(source, added);
        return layers::refresh(s, t, sink);
    }
    // A mana ability has no targets (see CR 605.1a). Its body may ask a choice.
    let activation = activation();
    let mut resolution = Resolution::of_trigger(&activation, ability);
    resolution.mana = color;
    if let Some(source) = activation.of.source {
        sink.enter_source(|| source);
    }
    let mut game = Game::new(s, t, sink);
    let flow = resolve(&mut game, &mut resolution);
    let done = if let Some(question) = resolution.question.take() {
        // An answer is needed by the enclosing transaction, which shares the sink's record
        // with this ability. Unwind back to it the same way a replacement choice does.
        Err(game.sink.asking.stop(question))
    } else {
        finish(&mut game, flow).map(|_| ())
    };
    if activation.of.source.is_some() {
        sink.leave();
    }
    done
}

/// Whether the condition of a static or replacement ability of the permanent in `slot`
/// holds, or it has none: its "as long as" or "while", read for the permanent as "this"
/// and its controller as "you". `ability` names it among the permanent's abilities
/// ([`crate::state::State::abilities`]): one of the face that is up, or one of a card under a
/// merged permanent, which is read as that card's ability, as its triggered abilities are
/// (see [CR 702.140e]): "A mutated permanent has all abilities of each card and token that
/// represents it."
///
/// # Panics
///
/// For a granted ability: the card table refuses a card that grants a replacement effect or
/// a rule change (`TableError::Granted`), the only abilities asked here.
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
#[inline]
pub(crate) fn static_condition_holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
    a: &Ability,
) -> bool {
    let Some(condition) = a.condition() else {
        return true;
    };
    if let Some(holds) = without_a_scene(s, slot, condition) {
        return holds;
    }
    let (card, face, index) = match ability {
        ObjectAbility::Own(index) => {
            let (card, face) = s.definition(slot).record(t);
            (card, face, index)
        }
        ObjectAbility::Merged(..) => {
            let Some((card, face, index, _)) = s.merged_ability(t, slot, ability) else {
                return false;
            };
            (card, face, index)
        }
        ObjectAbility::Granted(_) => {
            unreachable!("the card table refuses a granted replacement or rule change")
        }
    };
    let subject = ability_of(s, slot, card, face, TriggeredAbility::own(index));
    condition_holds(s, t, &subject, a)
}

/// The ability `ability` of the object in `slot`, printed on `card`'s `face`, as the trigger
/// it would be with nothing triggering it: the object as "this" and its controller as "you".
#[inline]
fn ability_of<L: Layout>(
    s: &State<L>,
    slot: L::Slot,
    card: L::Card,
    face: FaceIx,
    ability: TriggeredAbility,
) -> PendingTrigger<L> {
    PendingTrigger {
        of: AbilityRef {
            source: Some(s.object_ref(slot)),
            card,
            face,
            ability,
            controller: s.controller(slot).into(),
        },
        info: TriggerInfo::default(),
    }
}

/// What `read` finds of a scene of `subject` with no resolution: no costs, X and targets,
/// nothing done yet, and its source's words as they are now.
#[inline]
fn without_resolution<L: Layout, R>(
    s: &State<L>,
    t: &GameTable,
    subject: &PendingTrigger<L>,
    read: impl FnOnce(&Scene<'_, L>) -> R,
) -> R {
    read(&Scene {
        s,
        t,
        subject,
        costs: None,
        x: 0,
        targets: &[],
        registers: &Registers::default(),
        words: crate::state::READ_NOW,
    })
}

/// Whether `objects` names nothing, read for the object in `slot` as "this" and its
/// controller as "you": what an ability of it would find to act on, were it activated now
/// ([`Ability::needs`]).
#[inline]
pub(crate) fn names_nothing<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    objects: Objects,
) -> bool {
    // See CR 607.2a: a linked ability finds only the cards exiled with its own source.
    if objects == Objects::ExiledWithThis {
        return s
            .linked(LinkKind::ExiledWith, s.object_ref(slot))
            .is_empty();
    }
    let (card, face) = s.definition(slot).record(t);
    let subject = ability_of(s, slot, card, face, TriggeredAbility::default());
    without_resolution(s, t, &subject, |scene| scene.objects(objects).is_empty())
}

/// Whether `condition` holds for the permanent in `slot` as "this" and its controller as
/// "you", where that can be read of the permanent and its controller's own zones: what a
/// scene would answer ([`Scene::holds`]), with none made. None for any other condition. A
/// static ability's condition is read at every check of what it does, and most are these.
#[inline]
pub(crate) fn without_a_scene<L: Layout>(
    s: &State<L>,
    slot: L::Slot,
    condition: &Condition,
) -> Option<bool> {
    match *condition {
        Condition::EnteredThisTurn(Objects::This) => {
            Some(s.tracked(Tracker::EnteredThisTurn, slot))
        }
        Condition::Compare(left, comparison, right) => {
            let you = s.controller(slot);
            let (left, right) = (own_amount(s, you, left)?, own_amount(s, you, right)?);
            Some(comparison.holds(i32::from(left), i32::from(right)))
        }
        Condition::Did
        | Condition::Paid(_)
        | Condition::Is(..)
        | Condition::EnteredThisTurn(_)
        | Condition::OnBattlefield(_)
        | Condition::Token(_)
        | Condition::Cast(_)
        | Condition::HasChosenName(_)
        | Condition::WonFlip
        | Condition::DrewInDrawStep(_)
        | Condition::TurnOf(_)
        | Condition::AttackedThisTurn(_)
        | Condition::MeldPair(_)
        | Condition::Not(_)
        | Condition::All(_)
        | Condition::Any(_) => None,
    }
}

/// The value of `amount` for the player `you`, where it is a printed number or the number
/// of cards in a zone of their own; none for any other.
#[inline]
fn own_amount<L: Layout>(s: &State<L>, you: Seat, amount: Amount) -> Option<u8> {
    match amount {
        Amount::Fixed(n) => Some(n),
        Amount::Count(&Objects::All {
            zone: zone @ (Zone::Library | Zone::Hand | Zone::Graveyard),
            filter: Filter::Whose(&Players::You),
        }) => {
            let cards = s.zone(ZoneKey::Of(you, zone)).count();
            Some(u8::try_from(cards).unwrap_or(u8::MAX))
        }
        Amount::X
        | Amount::ThatMany
        | Amount::Carried
        | Amount::ThoseOf(_)
        | Amount::Count(_)
        | Amount::Times(..)
        | Amount::Half { .. }
        | Amount::Plus(..)
        | Amount::CardTypesAmong(_)
        | Amount::Power(_)
        | Amount::Toughness(_)
        | Amount::ManaValue(_)
        | Amount::Counters(..)
        | Amount::Life(_)
        | Amount::Most(..)
        | Amount::SpellsCast(_)
        | Amount::CardsDrawn(_)
        | Amount::SpellsCastBefore => None,
    }
}

/// Whether an ability's condition holds, or it has none, read for `subject`: the ability
/// as the trigger it would be, with its source and its controller as "you". A triggered
/// ability's intervening "if" is read this way as it triggers and again as it resolves
/// (see [CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "When the trigger event
/// occurs, the ability checks whether the stated condition is true. The ability triggers only
/// if it is; otherwise it does nothing. If the ability triggers, it checks the stated
/// condition again as it resolves." A static ability's "as long as" or "while" is read this
/// way too, as often as it is asked.
#[inline]
pub(crate) fn condition_holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    subject: &PendingTrigger<L>,
    ability: &Ability,
) -> bool {
    ability.condition().is_none_or(|condition| {
        // see CR 720.3a: "if [condition] and this Case is not solved".
        let can_solve = ability.designation() != beryl_card::Designation::ToSolve
            || subject
                .of
                .source
                .and_then(|source| s.permanent(source))
                .is_some_and(|case| !s.is_solved(case.slot()));
        can_solve && holds(s, t, subject, condition)
    })
}

/// Whether `condition` holds for a triggered ability, read for its source as "this" and its
/// controller as "you", before it is on the stack: an intervening "if" (see CR 603.4) or a state
/// trigger's condition (see CR 603.8).
pub(crate) fn holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    subject: &PendingTrigger<L>,
    condition: &Condition,
) -> bool {
    without_resolution(s, t, subject, |scene| scene.holds(condition))
}
