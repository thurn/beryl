//! The interpreter: what an ability's [`Behavior`] does to a game (plan K1).
//!
//! A [`Behavior::Custom`] is a Rust body, called as it always was. A [`Behavior::Data`] is
//! effect data, walked here through the same [`EffectCtx`] methods a body calls, so an
//! ability written as data makes the events its body made.
//!
//! So far it runs the entries the vocabulary's own keyword abilities are written in (cycling,
//! crew, offspring, evoke, impending and the mana abilities of `beryl_card`'s `Ability`).
//! An entry it does not run yet panics, so no card can come to depend on one before the
//! interpreter runs it.

use std::fmt::Debug;

use beryl_card::{
    Amount, CastingKind, Comparison, Condition, CopyException, Duration, Effect, Objects, Players,
};

use crate::card::{Ability, Behavior, Flow, ManaColor};
use crate::effect::{EffectCtx, entered_with};
use crate::event::{Event, Sink, TokenEntry};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Bodies, Conditions, Layout};
use crate::state::{self, Continuous, ObjectFlags, ObjectRef, PendingTrigger, State};
use crate::table::GameTable;

/// Resolves the ability `ctx` was made for
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of the
/// spell or ability follows its instructions in the order written."
#[inline]
pub(crate) fn resolve<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let ability: &'static Ability = ctx.ability;
    match ability.body {
        Behavior::Custom(custom) => {
            let bodies = custom.get::<Bodies>().expect("a body made by bodies!");
            L::body(bodies)(ctx)
        }
        Behavior::Data(effect) => run(ctx, effect),
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
    }) = ability.body
    {
        debug_assert!(mana.includes(color));
        let activation = activation();
        add_mana(
            s,
            sink,
            activation.controller,
            activation.source,
            color,
            mana.amount,
        );
        layers::refresh(s, t, sink);
        return Ok(());
    }
    let mut ctx = EffectCtx::new(s, t, sink, &activation(), ability);
    ctx.mana = color;
    // A mana ability never asks a question.
    let _ = resolve(&mut ctx);
    ctx.finish()
}

/// Adds mana to a player's mana pool
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
/// instructs a player to add mana, that mana goes into a player's mana pool." Nothing
/// triggers on adding mana, so it skips the event pipeline.
#[inline]
pub(crate) fn add_mana<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    player: Seat,
    source: Option<ObjectRef<L>>,
    color: ManaColor,
    amount: u8,
) {
    let pool = &mut s.player_mut(player).mana[color];
    *pool = pool.saturating_add(amount);
    sink.note(Event::Mana {
        player,
        source,
        color,
        amount,
    });
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
    ability
        .condition
        .is_none_or(|condition| holds(s, t, trigger, condition))
}

/// Stops on an entry the interpreter does not run yet.
#[cold]
fn not_yet(entry: &dyn Debug) -> ! {
    unimplemented!("the interpreter does not run {entry:?} yet")
}

/// Carries out one instruction, or each of several in order (CR 608.2c).
fn run<L: Layout>(ctx: &mut EffectCtx<'_, L>, effect: &'static Effect) -> Flow {
    match *effect {
        Effect::Sequence(effects) => {
            for effect in effects {
                run(ctx, effect)?;
            }
        }
        // CR 121.1: "A player draws a card by putting the top card of their library into
        // their hand."
        Effect::Draw { player, count } => {
            let (player, count) = (player_of(ctx, player), amount(ctx, count));
            ctx.draw(player, count);
        }
        // CR 701.21a: "To sacrifice a permanent, its controller moves it from the battlefield
        // directly to its owner's graveyard."
        Effect::Sacrifice(objects) => {
            let object = object(ctx, objects);
            ctx.sacrifice(object);
        }
        Effect::RemoveCounters {
            objects,
            kind,
            count,
        } => {
            let (object, count) = (object(ctx, objects), amount(ctx, count));
            ctx.remove_counters(object, kind, count);
        }
        // CR 611.2a: a continuous effect from a resolving ability "lasts as long as stated by
        // the spell or ability creating it".
        Effect::Modify {
            objects,
            modification,
            duration,
        } => {
            let object = object(ctx, objects);
            let duration = match duration {
                Duration::EndOfTurn => state::Duration::EndOfTurn,
                Duration::UntilYourNextTurn => state::Duration::UntilYourNextTurn(ctx.controller()),
                Duration::Indefinite => state::Duration::Indefinite,
            };
            ctx.add_effect(object, Continuous::Layer(modification), duration);
        }
        // A copy of the ability's own source, as offspring's (CR 702.175a): the copiable
        // values are its card's ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)),
        // with the 1/1 exception of
        // [CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b), whether or not it is
        // still on the battlefield.
        Effect::CreateCopies {
            player: Players::You,
            of: Objects::This,
            count,
            except: CopyException::PowerToughness(1, 1),
        } => {
            let count = amount(ctx, count);
            let how = TokenEntry {
                one_one: true,
                ..TokenEntry::default()
            };
            ctx.create_tokens_of(ctx.entry.card, count, how);
        }
        // CR 106.3: "A spell or ability that produces mana instructs a player to add that
        // mana": of the kind its activator chose among those it produces.
        Effect::AddMana { player, mana } => {
            debug_assert!(mana.includes(ctx.mana));
            let player = player_of(ctx, player);
            ctx.add_mana(player, ctx.mana, mana.amount);
        }
        _ => not_yet(effect),
    }
    Ok(())
}

/// The one player `players` names.
fn player_of<L: Layout>(ctx: &EffectCtx<'_, L>, players: Players) -> Seat {
    match players {
        // CR 109.5: for an activated ability, "you" is "the player who activated the
        // ability"; for a triggered one, "the controller of the object when the ability
        // triggered".
        Players::You => ctx.controller(),
        _ => not_yet(&players),
    }
}

/// The one object `objects` names.
fn object<L: Layout>(ctx: &EffectCtx<'_, L>, objects: Objects) -> ObjectRef<L> {
    match objects {
        Objects::This => ctx.source(),
        _ => not_yet(&objects),
    }
}

/// The value of `amount` as its instruction is reached (CR 608.2h).
fn amount<L: Layout>(ctx: &EffectCtx<'_, L>, amount: Amount) -> u8 {
    match amount {
        Amount::Fixed(n) => n,
        Amount::X => ctx.x(),
        _ => not_yet(&amount),
    }
}

/// Whether `condition` holds for a triggered ability, as it triggers or as it resolves (CR
/// 603.4).
fn holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    trigger: &PendingTrigger<L>,
    condition: &Condition,
) -> bool {
    match *condition {
        Condition::Custom(custom) => {
            let conditions = custom
                .get::<Conditions>()
                .expect("a condition made by conditions!");
            L::condition(conditions)(s, t, trigger)
        }
        Condition::Paid(kind) => {
            let flag = match kind {
                CastingKind::Kicker => ObjectFlags::KICKED,
                CastingKind::Offspring => ObjectFlags::OFFSPRING,
                CastingKind::Impending => ObjectFlags::IMPENDING,
                CastingKind::Evoke => ObjectFlags::EVOKED,
                CastingKind::Overload => not_yet(condition),
            };
            entered_with(s, trigger, flag)
        }
        Condition::Compare(left, comparison, right) => {
            let (left, right) = (count(s, trigger, left), count(s, trigger, right));
            match comparison {
                Comparison::AtLeast => left >= right,
                Comparison::AtMost => left <= right,
                Comparison::Equal => left == right,
                Comparison::MoreThan => left > right,
                Comparison::LessThan => left < right,
            }
        }
        Condition::Not(condition) => !holds(s, t, trigger, condition),
        Condition::All(conditions) => conditions.iter().all(|c| holds(s, t, trigger, c)),
        Condition::Any(conditions) => conditions.iter().any(|c| holds(s, t, trigger, c)),
        Condition::Did | Condition::Is(..) => not_yet(condition),
    }
}

/// The value of `amount` for a triggered ability's intervening "if". Its source's counters
/// are those it has now; a source that has left has none, being a new object where it went
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
fn count<L: Layout>(s: &State<L>, trigger: &PendingTrigger<L>, amount: Amount) -> u8 {
    match amount {
        Amount::Fixed(n) => n,
        Amount::Counters(Objects::This, kind) => trigger
            .source
            .filter(|&source| s.is_current(source))
            .map_or(0, |source| s.object(source.slot).counters.get(kind)),
        _ => not_yet(&amount),
    }
}
