//! The interpreter: what an ability's [`Behavior`] does to a game.
//!
//! A [`Behavior::Custom`] is a Rust body, called with the game and the resolution. A
//! [`Behavior::Data`] is effect data: [`run`] carries out its instructions with the
//! library's functions, and [`scene`] reads its expressions. The data is the same for every
//! layout; only the interpreter is compiled for each.

mod run;
mod scene;

use beryl_card::{Amount, Condition, Effect, Objects, Players, Zone};

use crate::bodies::Bodies;
use crate::card::{Ability, AbilityIx, Behavior, Flow, ManaColor, Stop};
use crate::event::Sink;
use crate::filter::Filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layers;
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::{PendingTrigger, State, Tracker, TriggerInfo, ZoneKey};
use crate::table::GameTable;

pub(crate) use run::{Run, no_primitive};
pub(crate) use scene::Outcome;
use scene::{Registers, Scene};

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

/// Ends a resolution that came to `flow`, with a layer refresh for what follows. Returns
/// whether it paused at a question, or [`Full`] if a capacity ran out: the one place a
/// resolution's `Full` becomes the engine's (the crate's contract, "How a resolution stops").
#[inline]
pub(crate) fn finish<L: Layout>(game: &mut Game<'_, L>, flow: Flow) -> Result<bool, Full> {
    layers::refresh(game.state, game.table, game.sink)?;
    match flow {
        Ok(()) => Ok(false),
        Err(Stop::Pause) => Ok(true),
        Err(Stop::Full) => Err(Full),
    }
}

/// Carries out a mana ability whose costs are paid, making mana of the kind `color` its
/// activator chose ([CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b)): "An
/// activated mana ability doesn't go on the stack, so it can't be targeted, countered, or
/// otherwise responded to. Rather, it resolves immediately after it is activated."
///
/// One whose whole effect is to add mana, as a land's is, needs none of what resolving an
/// ability sets up, so it is done here directly: the mana solver taps for these in every
/// payment. `activation` describes the ability for any other.
///
/// What it does is the effect of an ability of its source, where its costs were its
/// controller's doing ([`Cause::Object`](crate::event::Cause::Object)). The mana added
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
        let (player, source) = (activation.controller, activation.source);
        let added = s.add_mana(player, source, color, mana.amount);
        sink.note_from(source, added);
        return layers::refresh(s, t, sink);
    }
    // A mana ability never asks a question, and has no targets (CR 605.1a).
    let activation = activation();
    let mut resolution = Resolution::of_trigger(&activation, ability);
    resolution.mana = color;
    if let Some(source) = activation.source {
        sink.enter_source(|| source);
    }
    let mut game = Game::new(s, t, sink);
    let flow = resolve(&mut game, &mut resolution);
    let done = finish(&mut game, flow).map(|_| ());
    if activation.source.is_some() {
        sink.leave();
    }
    done
}

/// Whether a triggered ability's intervening "if" holds, or it has none
/// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "When the trigger event
/// occurs, the ability checks whether the stated condition is true. The ability triggers only
/// if it is; otherwise it does nothing. If the ability triggers, it checks the stated
/// condition again as it resolves."
#[inline]
pub(crate) fn intervening_if<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    trigger: &PendingTrigger<L>,
    ability: &Ability,
) -> bool {
    condition_holds(s, t, trigger, ability)
}

/// Whether the condition of a static or replacement ability of the permanent in `slot`
/// holds, or it has none: its "as long as" or "while", read for the permanent as "this"
/// and its controller as "you". `index` is the ability's place on the face that is up.
#[inline]
pub(crate) fn static_condition_holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    index: AbilityIx,
    ability: &Ability,
) -> bool {
    let Some(condition) = ability.condition() else {
        return true;
    };
    if let Some(holds) = without_a_scene(s, slot, condition) {
        return holds;
    }
    let (card, face) = s.copiable(slot).record(t);
    let subject = PendingTrigger {
        source: Some(s.object_ref(slot)),
        card,
        face,
        ability: index,
        controller: s.controller(slot),
        info: TriggerInfo::default(),
    };
    condition_holds(s, t, &subject, ability)
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
    // CR 607.2a: a linked ability finds only the cards exiled with its own source.
    if objects == Objects::ExiledWithThis {
        return s.exiled_with(slot).is_empty();
    }
    let (card, face) = s.copiable(slot).record(t);
    let subject = PendingTrigger {
        source: Some(s.object_ref(slot)),
        card,
        face,
        ability: AbilityIx::default(),
        controller: s.controller(slot),
        info: TriggerInfo::default(),
    };
    let scene = Scene {
        s,
        t,
        subject: &subject,
        costs: None,
        x: 0,
        targets: &[],
        registers: &Registers::default(),
    };
    scene.objects(objects).is_empty()
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
        _ => None,
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
        _ => None,
    }
}

/// Whether an ability's condition holds, or it has none, read for `subject`: the ability
/// as the trigger it would be, with its source and its controller as "you". A static
/// ability's "as long as" or "while" is read this way too, as often as it is asked.
#[inline]
pub(crate) fn condition_holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    subject: &PendingTrigger<L>,
    ability: &Ability,
) -> bool {
    ability.condition().is_none_or(|condition| {
        let scene = Scene {
            s,
            t,
            subject,
            costs: None,
            x: 0,
            targets: &[],
            registers: &Registers::default(),
        };
        scene.holds(condition)
    })
}
