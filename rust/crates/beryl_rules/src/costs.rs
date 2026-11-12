//! Costs ([CR 118](https://yawgatog.com/resources/magic-rules/#R118)) paid while a spell or
//! ability resolves ([CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812)): "The
//! action [do something] is a cost, paid when the spell or ability resolves."
//!
//! The cost of a spell or an ability is paid as it is cast or activated, by the engine's
//! casting steps. What is here is the cost an effect names, which a player pays or declines
//! in the middle of a resolution. Mana is the one cost an effect names so far
//! ([`Effect::Unless`]), so a cost here is a [`ManaCost`].

use beryl_card::Effect;

use crate::bitboard::Bitboard;
use crate::card::{Flow, ManaCost, Part, Players, Prompt, Purpose, Stop};
use crate::choices;
use crate::decision::Opt;
use crate::event::Event;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::Run;
use crate::layout::{Layout, Queue, index};
use crate::mana;
use crate::profile::Payment;
use crate::resolution::Resolution;

/// `player` pays `cost` or does not, as an effect offers
/// ([CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812)): "[A player] may [do
/// something]. If [that player] [does, doesn't, or can't], [effect]." They are asked
/// `prompt`, a yes or no about the cost, and pay on a yes. A player who cannot pay is not
/// asked ([CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183)): "A player can't pay
/// a cost without having the necessary resources to pay it fully." Returns whether they
/// paid.
///
/// [CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g): "If an effect gives a
/// player the option to pay mana, they may activate mana abilities before taking that
/// action."
pub fn may_pay<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cost: ManaCost,
    prompt: Prompt,
) -> Result<bool, Stop> {
    let none = Bitboard::EMPTY;
    if !mana::payable(game.state, game.table, player, cost, none)?
        || !choices::yes_no(game, resolution, player, prompt.cost(cost), none)?
    {
        return Ok(false);
    }
    pay(game, resolution, player, cost)?;
    game.sink.note(Event::ResolutionPayment {
        player,
        item: resolution.entry.clone(),
    });
    Ok(true)
}

/// `player` pays the mana cost `cost`, which they can pay, under the profile's payment rules
/// (§10): by hand, one source tapped or one floating mana spent with each answer until
/// nothing is left to pay, or else by one of the whole payments the engine worked out.
pub fn pay<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cost: ManaCost,
) -> Flow {
    let (none, mana) = (Bitboard::EMPTY, Prompt::new(Purpose::Mana));
    if game.state.profile().payment == Payment::Manual {
        let mut remaining = cost;
        while !remaining.is_zero() {
            let options = mana::manual_options(game.state, game.table, player, remaining, none)?;
            let (options, step) = (options.as_slice(), mana.cost(remaining));
            let option = choices::pay(game, resolution, player, options, step)?;
            let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
            remaining = mana::manual_pay(s, t, sink, player, remaining, option)?;
        }
        return Ok(());
    }
    let greedy = game.state.profile().payment == Payment::Greedy;
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    let plans = mana::plans(s, t, sink, player, cost, none, greedy)?;
    let mut opts: L::Queue<Opt<L>, 64> = Default::default();
    for i in 0..plans.len() {
        opts.push(Opt::Plan(index::<L>(i)))?;
    }
    let plan = mana.cost(cost).part(Part::Plan);
    let Opt::Plan(i) = choices::pay(game, resolution, player, opts.as_slice(), plan)? else {
        unreachable!("a plan");
    };
    let plan = &plans.as_slice()[Into::<usize>::into(i)];
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    mana::pay(s, t, sink, player, cost, none, plan)?;
    Ok(())
}

/// Whether to pay so that an [`Effect::Unless`] does not happen.
pub(crate) const UNLESS: Prompt = Prompt::new(Purpose::UnlessPays);

/// [`Effect::Unless`]: "[Effect] unless [player] pays [cost]"
/// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "This means the same
/// thing as '[A player may do something else]. If [that player doesn't], [do something].'"
/// With no such player, no one pays. The outcome is the effect's, or nothing when the cost
/// was paid.
pub(crate) fn unless_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    effect: &'static Effect,
    player: Players,
    cost: ManaCost,
) -> Flow {
    let paid = match run.one(player) {
        Some(player) => {
            let (game, resolution) = run.resolving();
            may_pay(game, resolution, player, cost, UNLESS)?
        }
        None => false,
    };
    run.carry_out_or_nothing((!paid).then_some(effect))
}
