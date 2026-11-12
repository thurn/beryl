//! Costs ([CR 118](https://yawgatog.com/resources/magic-rules/#R118)) paid while a spell or
//! ability resolves ([CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812)): "The
//! action [do something] is a cost, paid when the spell or ability resolves."
//!
//! The cost of a spell or an ability is paid as it is cast or activated, by the engine's
//! casting steps. What is here is the cost an effect names ([`Effect::Unless`]), which a
//! player pays or declines in the middle of a resolution: mana ([`may_pay`]), or the discard
//! of a card ([`may_discard`]).

use beryl_card::{Chosen, Cost, CostAction, CostPart, Effect, Measure};

use crate::actions::discard;
use crate::bitboard::Bitboard;
use crate::card::{Flow, ManaCost, Part, Players, Prompt, Purpose, Stop};
use crate::choices;
use crate::decision::Opt;
use crate::event::Event;
use crate::filter::Evaluate;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Run, no_primitive};
use crate::layout::{Layout, Queue, index};
use crate::mana;
use crate::profile::Payment;
use crate::resolution::Resolution;
use crate::state::{Zone, ZoneKey};

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

/// `player` discards one of `cards`, cards in their hand, or does not, as an effect offers
/// ([CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812)): "The action [do
/// something] is a cost, paid when the spell or ability resolves." They are asked `prompt`,
/// a yes or no about the cards, and on a yes choose which
/// ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)). With no such card they
/// cannot pay and are not asked
/// ([CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183)). Returns whether they
/// discarded.
pub fn may_discard<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cards: Bitboard<L::Slot>,
    prompt: Prompt,
) -> Result<bool, Stop> {
    if cards.is_empty() || !choices::yes_no(game, resolution, player, prompt, cards)? {
        return Ok(false);
    }
    discard::discard_one_of(game, resolution, player, cards)?;
    Ok(true)
}

/// Whether to pay so that an [`Effect::Unless`] does not happen.
pub(crate) const UNLESS: Prompt = Prompt::new(Purpose::UnlessPays);

/// [`Effect::Unless`]: "[Effect] unless [player] pays [cost]"
/// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "This means the same
/// thing as '[A player may do something else]. If [that player doesn't], [do something].'"
/// With no such player, no one pays. The outcome is the effect's, or nothing when the cost
/// was paid.
///
/// The cost is mana alone, or the discard of one card that a filter accepts for the player
/// ("unless you discard a basic land card"); the definition check refuses any other.
pub(crate) fn unless_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    effect: &'static Effect,
    player: Players,
    cost: Cost,
) -> Flow {
    let paid = match (run.one(player), cost.parts()) {
        (None, _) => false,
        (Some(player), []) => {
            let (game, resolution) = run.resolving();
            may_pay(game, resolution, player, cost.mana, UNLESS)?
        }
        (
            Some(player),
            [
                CostPart::Chosen(
                    CostAction::Discard,
                    Chosen {
                        zone: Zone::Hand,
                        filter,
                        other: false,
                        measure: Measure::Count(1),
                    },
                ),
            ],
        ) if cost.mana.is_zero() => unless_discards(run, effect, player, *filter)?,
        _ => no_primitive(&cost),
    };
    run.carry_out_or_nothing((!paid).then_some(effect))
}

/// The discard of an [`Effect::Unless`]: whether `player` discarded a card `filter` accepts
/// so that `effect` does not happen.
///
/// Where the effect is itself a discard by that player, "discard two cards unless you
/// discard a basic land card", the question is the discard's own, asked as one card
/// instead of that many ([`Part::Instead`]); and with one card in hand both ways discard
/// it, so nothing is asked and the effect's discard takes it.
fn unless_discards<L: Layout>(
    run: &mut Run<'_, '_, L>,
    effect: &'static Effect,
    player: Seat,
    filter: crate::filter::Filter,
) -> Result<bool, Stop> {
    let instead_of = match *effect {
        Effect::Discard { player: who, count } if run.one(who) == Some(player) => {
            Some(run.scene().amount(count))
        }
        _ => None,
    };
    let (game, resolution) = run.resolving();
    let hand = game.state.zone(ZoneKey::Of(player, Zone::Hand));
    if instead_of.is_some_and(|cards| cards > 0) && hand.count() == 1 {
        return Ok(false);
    }
    let cards = filter.select(game.state, game.table, hand, player);
    let instead = Prompt::new(Purpose::Discard)
        .most(instead_of.unwrap_or(1))
        .part(Part::Instead);
    may_discard(game, resolution, player, cards, instead)
}
