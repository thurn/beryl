//! The interpreter: what an ability's [`Behavior`] does to a game (plan K1).
//!
//! A [`Behavior::Custom`] is a Rust body, called as it always was. A [`Behavior::Data`] is
//! effect data: [`run`] carries out its instructions through the same [`EffectCtx`] methods
//! a body calls, so an ability written as data makes the events its body made, and [`scene`]
//! reads its expressions. The data is the same for every layout; only the interpreter is
//! compiled for each, so an ability written as data needs no `bodies!`.

mod run;
mod scene;

use beryl_card::{Effect, Players};

use crate::card::{Ability, Behavior, Flow, ManaColor};
use crate::effect::EffectCtx;
use crate::event::{Event, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Bodies, Layout};
use crate::state::{ObjectRef, PendingTrigger, State};
use crate::table::GameTable;

use run::Run;
use scene::{Registers, Scene};

/// Resolves the ability `ctx` was made for
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of the
/// spell or ability follows its instructions in the order written." `legal` is its targets
/// that were legal as its resolution began ([`crate::resolve::legal_targets`]).
#[inline]
pub(crate) fn resolve<L: Layout>(ctx: &mut EffectCtx<'_, L>, legal: u8) -> Flow {
    let ability: &'static Ability = ctx.ability;
    match ability.body {
        Behavior::Custom(custom) => {
            let bodies = custom.get::<Bodies>().expect("a body made by bodies!");
            L::body(bodies)(ctx)
        }
        Behavior::Data(effect) => data(ctx, effect, legal),
    }
}

/// Carries out an ability written as data. Out of line: most of what resolves in a game is
/// a permanent spell, which has no ability to run, or a mana ability, which
/// [`mana_ability`] does without coming here.
#[inline(never)]
fn data<L: Layout>(ctx: &mut EffectCtx<'_, L>, effect: &'static Effect, legal: u8) -> Flow {
    debug_assert!(
        !ctx.ability.may_pause || effect.asks(),
        "an ability declares may_pause, but its effect never asks"
    );
    Run::new(ctx, legal).carry_out(effect)
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
    // A mana ability never asks a question, and has no targets (CR 605.1a).
    let _ = resolve(&mut ctx, 0);
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
    ability.condition.is_none_or(|condition| {
        let scene = Scene {
            s,
            t,
            subject: trigger,
            costs: None,
            x: 0,
            targets: &[],
            registers: &Registers::default(),
        };
        scene.holds(condition)
    })
}
