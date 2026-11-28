//! Outlets activated in the middle of a payment: mana abilities whose cost sacrifices or taps a
//! permanent their controller chooses, as "Sacrifice an artifact: Add {C}{C}" (see
//! [CR 605.3a]): "A player may activate an activated mana ability whenever they have priority,
//! whenever they are casting a spell or activating an ability that requires a mana payment, or
//! whenever a rule or effect asks for a mana payment".
//!
//! The solver does not plan outlets, so a payment by plan in which the payer controls one
//! offers the plans that pay the whole cost, then each mana ability of a permanent an outlet
//! could sacrifice, which adds its mana to the pool first (a tapped permanent can still be
//! sacrificed), then each outlet. A payment by hand offers the outlets beside its taps. Every
//! option is offered only if the rest of the cost is still payable after it, so every sequence
//! of answers ends with the cost paid. Choosing an outlet asks which permanent it takes, one
//! for each set of interchangeable permanents, and then which color it adds where it adds any
//! color; a question with one answer is not asked.
//!
//! Implements: none. Offers and carries out outlet activations during a payment (CR 605.3a).
//!
//! [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a

use crate::bitboard::Bitboard;
use crate::card::{ChosenKind, ManaColor, ManaCost, ObjectAbility, Part, Prompt, Purpose};
use crate::decision::{Candidates, Decision, DecisionKind, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::catalog;
use crate::layout::{Layout, Queue, index};
use crate::mana::{self, Outlet, Paying};
use crate::profile::Payment;
use crate::replacement::event::Sink;
use crate::resolution::Resolution;
use crate::state::{CastStep, Casting, Pending, State};
use crate::table::{Capability, GameTable, is_outlet_mana, outlet_part};
use beryl_kernel::chosen::ChosenValue;

use super::costs::total_cost;
use super::mana_payment::{self, SacrificeCost};
use super::steps::{after_mana, pay_mana as pay_again, top};
use super::{parts, spent};

/// Whether the rest of a cost, `ManaCost`, is still payable in a state a step of the payment
/// leaves.
type Keeps<'a, L> = &'a dyn Fn(&State<L>, ManaCost) -> Result<bool, Full>;

/// Whether a payment by `player` offers outlets: in a game whose cards have priced mana
/// abilities, where they control an outlet that could take a permanent now.
#[inline]
pub(super) fn offered<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> bool {
    t.can(Capability::PricedMana) && controls_outlet(s, t, player)
}

#[cold]
#[inline(never)]
fn controls_outlet<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> bool {
    !mana::outlets(s, t, player, Bitboard::EMPTY).is_empty()
}

/// Whether ability `ability` of the permanent in `slot` is an outlet.
pub(super) fn is_outlet<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    t.can(Capability::PricedMana)
        && s.abilities(t, slot)
            .any(|(a, printed)| a == ability && is_outlet_mana(printed))
}

/// Whether the rest of the cost of the spell or ability being cast is payable in `after`: with
/// its other costs too, for a spell that sacrifices a permanent as it is cast.
fn casting_keeps<L: Layout>(t: &GameTable) -> impl Fn(&State<L>, ManaCost) -> Result<bool, Full> {
    move |after, cost| {
        let entry = top(after);
        let paying = Paying::entry(after, t, entry);
        match SacrificeCost::of_entry(t, entry) {
            Some(other) => other.payable(after, t, cost, paying),
            None => mana::payable(after, t, entry.controller(), cost, Bitboard::EMPTY, paying),
        }
    }
}

/// Whether a payment of `cost` by `player`, whose plans pay none of it, can still be made by
/// first adding mana a permanent an outlet could sacrifice makes, or by activating an outlet,
/// as `keeps` judges what follows.
pub(super) fn payable_through<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    paying: Paying<L>,
    keeps: Keeps<'_, L>,
) -> Result<bool, Full> {
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    step_options(s, t, player, cost, paying, keeps, &mut out)?;
    Ok(!out.is_empty())
}

/// Adds to `out` the options of a payment by plan of `cost` beside its plans: each mana
/// ability of a permanent that an outlet could sacrifice, then each outlet, each only if
/// `keeps` says the cost is still payable after it.
fn step_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    paying: Paying<L>,
    keeps: Keeps<'_, L>,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let outlets = mana::outlets(s, t, player, Bitboard::EMPTY);
    let fodder = (outlets.iter())
        .filter(|o| o.sacrifices())
        .fold(Bitboard::EMPTY, |all, o| all | o.fodder);
    if !fodder.is_empty() {
        for &option in mana::manual_candidates(s, t, player, ManaCost::NONE, paying)?.iter() {
            let (Opt::Mana { slot, .. } | Opt::Activate { slot, .. }) = option else {
                continue;
            };
            if !fodder.contains(slot) {
                continue;
            }
            let after = simulate(s, t, |after, sink| {
                mana::manual_pay(after, t, sink, player, ManaCost::NONE, option, paying)?;
                Ok(())
            })?;
            if keeps(&after, cost)? {
                out.push(option)?;
            }
        }
    }
    for o in &outlets {
        if !takes(s, t, o, cost, keeps)?.is_empty() {
            out.push(Opt::Activate {
                slot: o.slot,
                ability: o.ability,
            })?;
        }
    }
    Ok(())
}

/// `s` after `act`, with the layers brought up to date and nothing recorded.
fn simulate<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    act: impl FnOnce(&mut State<L>, &mut Sink<L>) -> Result<(), Full>,
) -> Result<State<L>, Full> {
    let mut after = s.clone();
    crate::coverage::without_recording(|| {
        let mut sink = Sink::off();
        act(&mut after, &mut sink)?;
        crate::continuous::layers::refresh(&mut after, t, &mut sink)
    })?;
    Ok(after)
}

/// The kinds of mana outlet `o` may add, as one bit each.
fn kinds<L: Layout>(o: &Outlet<L>) -> impl Iterator<Item = ManaColor> {
    let kinds = o.kinds();
    (0..6)
        .filter(move |&k| kinds & (1 << k) != 0)
        .map(|k| ManaColor::ALL[k])
}

/// The colors, a bit per [`ManaColor`], that outlet `o` taking the permanent in `paid` may add
/// with `cost` still payable after.
fn colors<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    o: &Outlet<L>,
    paid: L::Slot,
    cost: ManaCost,
    keeps: Keeps<'_, L>,
) -> Result<u8, Full> {
    let mut out = 0;
    for color in kinds(o) {
        let after = simulate(s, t, |after, sink| {
            mana::activate_outlet(after, t, sink, o.slot, o.ability, paid, color)
        })?;
        if keeps(&after, cost)? {
            out |= 1 << color.index();
        }
    }
    Ok(out)
}

/// The permanents outlet `o` may take, one for each set of interchangeable ones, with `cost`
/// still payable after.
fn takes<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    o: &Outlet<L>,
    cost: ManaCost,
    keeps: Keeps<'_, L>,
) -> Result<Bitboard<L::Slot>, Full> {
    let mut out = Bitboard::EMPTY;
    for paid in collapse(s, o.fodder).sorted(|slot| slot) {
        if colors(s, t, o, paid, cost, keeps)? != 0 {
            out.insert(paid);
        }
    }
    Ok(out)
}

/// The outlet that ability `ability` of the permanent in `slot` is.
fn outlet<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Outlet<L> {
    mana::outlets(s, t, player, Bitboard::EMPTY)
        .into_iter()
        .find(|o| o.slot == slot && o.ability == ability)
        .expect("an outlet offered")
}

/// The color of mana a color answer names.
fn color_of<L: Layout>(option: Option<Opt<L>>) -> ManaColor {
    let ChosenValue::Color(colors) = catalog::value(ChosenKind::Color, option) else {
        unreachable!("a color among a color's candidates");
    };
    ManaColor::ALL[colors.0.trailing_zeros() as usize]
}

/// Pays the mana of a spell or ability being cast by plan, where its payer controls an outlet
/// ([`super::steps::pay_mana`]): the first plan under [`Payment::Greedy`] if there is one, else
/// the one option there is, else the question of [`CastStep::Plan`].
pub(super) fn pay_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
    cost: ManaCost,
    payment: Payment,
) -> Result<(), Full> {
    let greedy = payment == Payment::Greedy;
    let plans = mana_payment::plans(s, t, sink, top(s), cost, greedy)?;
    if greedy && let Some(plan) = plans.as_slice().first() {
        let paying = Paying::entry(s, t, top(s));
        let spent = mana::pay(s, t, sink, c.player, cost, Bitboard::EMPTY, plan, paying)?;
        spent::record(s, t, &c, spent)?;
        return after_mana(s, t, sink, c);
    }
    let options = plan_options(s, t, sink, &c, cost)?;
    debug_assert!(!options.is_empty(), "a spell offered is payable");
    if greedy || options.len() == 1 {
        return take(s, t, sink, c, options.as_slice()[0], greedy);
    }
    c.step = CastStep::Plan;
    s.ask(Pending::Casting(c));
    Ok(())
}

/// The options of [`CastStep::Plan`] where the payer controls an outlet: the plans, then what
/// [`step_options`] adds.
fn plan_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: &Casting<L>,
    cost: ManaCost,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let entry = top(s);
    let plans = mana_payment::plans(s, t, sink, entry, cost, false)?;
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for i in 0..plans.len() {
        out.push(Opt::Plan(index::<L>(i)))?;
    }
    let paying = Paying::entry(s, t, entry);
    step_options(s, t, c.player, cost, paying, &casting_keeps(t), &mut out)?;
    Ok(out)
}

/// The question of [`CastStep::Plan`] where the payer controls an outlet.
pub(super) fn plan_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: &Casting<L>,
) -> Result<Decision<L>, Full> {
    let cost = total_cost(s, t, top(s), c);
    let list = plan_options(s, t, sink, c, cost)?;
    let prompt = Prompt::new(Purpose::Mana).cost(cost).part(Part::Plan);
    Ok(Decision::options(
        c.player,
        DecisionKind::PaymentPlan,
        list,
        prompt,
    ))
}

/// Carries out the answer `option` to [`CastStep::Plan`] where the payer controls an outlet.
pub(super) fn take<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting<L>,
    option: Opt<L>,
    greedy: bool,
) -> Result<(), Full> {
    let entry = top(s);
    let cost = total_cost(s, t, entry, &c);
    let paying = Paying::entry(s, t, entry);
    if let Opt::Plan(i) = option {
        let plans = mana_payment::plans(s, t, sink, entry, cost, false)?;
        let plan = &plans.as_slice()[Into::<usize>::into(i)];
        let spent = mana::pay(s, t, sink, c.player, cost, Bitboard::EMPTY, plan, paying)?;
        spent::record(s, t, &c, spent)?;
        return after_mana(s, t, sink, c);
    }
    if let Opt::Activate { slot, ability } = option
        && is_outlet(s, t, slot, ability)
    {
        return begin_outlet(s, t, sink, c, slot, ability, None, greedy);
    }
    // A mana ability of a permanent an outlet could sacrifice: its mana floats.
    mana::manual_pay(s, t, sink, c.player, ManaCost::NONE, option, paying)?;
    pay_again(s, t, sink, c)
}

/// Begins activating the outlet `ability` of the permanent in `slot` during the payment:
/// asks which permanent it takes ([`CastStep::Outlet`]), unless one set of interchangeable
/// permanents keeps the rest payable or `greedy` takes the first. `remaining` is what the
/// manual profile has left to pay, `None` for a payment by plan.
#[allow(clippy::too_many_arguments)]
pub(super) fn begin_outlet<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
    slot: L::Slot,
    ability: ObjectAbility,
    remaining: Option<ManaCost>,
    greedy: bool,
) -> Result<(), Full> {
    let cost = remaining.unwrap_or_else(|| total_cost(s, t, top(s), &c));
    let o = outlet(s, t, c.player, slot, ability);
    let able = takes(s, t, &o, cost, &casting_keeps(t))?;
    if greedy || able.count() == 1 {
        let paid = able
            .min(|slot| slot)
            .expect("an outlet offered takes a permanent");
        return choose_color(s, t, sink, c, slot, ability, paid, remaining, greedy);
    }
    c.step = CastStep::Outlet {
        slot,
        ability,
        paid: None,
        remaining,
    };
    s.ask(Pending::Casting(c));
    Ok(())
}

/// Asks which color the outlet adds, taking the permanent in `paid`, unless one color keeps
/// the rest payable or `greedy` takes the first; then activates it.
#[allow(clippy::too_many_arguments)]
fn choose_color<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
    slot: L::Slot,
    ability: ObjectAbility,
    paid: L::Slot,
    remaining: Option<ManaCost>,
    greedy: bool,
) -> Result<(), Full> {
    let cost = remaining.unwrap_or_else(|| total_cost(s, t, top(s), &c));
    let o = outlet(s, t, c.player, slot, ability);
    let colors = colors(s, t, &o, paid, cost, &casting_keeps(t))?;
    debug_assert!(colors != 0, "a permanent offered keeps the cost payable");
    if greedy || colors.is_power_of_two() {
        let color = ManaColor::ALL[colors.trailing_zeros() as usize];
        return activate(s, t, sink, c, slot, ability, paid, color, remaining);
    }
    c.step = CastStep::Outlet {
        slot,
        ability,
        paid: Some(paid),
        remaining,
    };
    s.ask(Pending::Casting(c));
    Ok(())
}

/// Activates the outlet and goes back to the payment: to its plans, or to the manual profile's
/// next tap with `remaining` left to pay.
#[allow(clippy::too_many_arguments)]
fn activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
    slot: L::Slot,
    ability: ObjectAbility,
    paid: L::Slot,
    color: ManaColor,
    remaining: Option<ManaCost>,
) -> Result<(), Full> {
    mana::activate_outlet(s, t, sink, slot, ability, paid, color)?;
    match remaining {
        Some(remaining) => {
            c.step = CastStep::Manual { remaining };
            s.ask(Pending::Casting(c));
            Ok(())
        }
        None => pay_again(s, t, sink, c),
    }
}

/// The question of [`CastStep::Outlet`]: which permanent the outlet takes while `paid` is
/// `None`, then which color it adds.
pub(super) fn outlet_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
    slot: L::Slot,
    ability: ObjectAbility,
    paid: Option<L::Slot>,
    remaining: Option<ManaCost>,
) -> Result<Decision<L>, Full> {
    let cost = remaining.unwrap_or_else(|| total_cost(s, t, top(s), c));
    let o = outlet(s, t, c.player, slot, ability);
    let keeps = casting_keeps(t);
    let about = Bitboard::single(slot);
    match paid {
        None => Ok(pick_decision(&o, c.player, takes(s, t, &o, cost, &keeps)?)),
        Some(paid) => {
            let colors = colors(s, t, &o, paid, cost, &keeps)?;
            color_decision(t, c.player, colors, about)
        }
    }
}

/// The question which permanent outlet `o` takes, among `able`.
fn pick_decision<L: Layout>(o: &Outlet<L>, player: Seat, able: Bitboard<L::Slot>) -> Decision<L> {
    let (action, chosen) = outlet_part(o.printed).expect("an outlet mana ability");
    let prompt = Prompt::new(parts::purpose(action, &chosen)).most(1);
    Decision {
        player,
        decider: player,
        kind: DecisionKind::ChooseObject,
        candidates: Candidates::Objects(able),
        prompt,
        about: Bitboard::single(o.slot),
        filter: None,
    }
}

/// The question which of `colors`, a bit per [`ManaColor`], an outlet adds.
fn color_decision<L: Layout>(
    t: &GameTable,
    player: Seat,
    colors: u8,
    about: Bitboard<L::Slot>,
) -> Result<Decision<L>, Full> {
    let prompt = Prompt::new(ChosenKind::Color.purpose());
    catalog::decision_among(t, player, ChosenKind::Color, prompt, about, |n| {
        matches!(ChosenValue::<L>::of_number(ChosenKind::Color, n),
            Some(ChosenValue::Color(c)) if c.0 & colors != 0)
    })
}

/// Applies the answer to [`CastStep::Outlet`].
#[allow(clippy::too_many_arguments)]
pub(super) fn perform_outlet<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting<L>,
    slot: L::Slot,
    ability: ObjectAbility,
    paid: Option<L::Slot>,
    remaining: Option<ManaCost>,
    option: Option<Opt<L>>,
    object: Option<L::Slot>,
) -> Result<(), Full> {
    match paid {
        None => {
            let paid = object
                .or(option.and_then(|o| o.object_named(s)))
                .expect("a permanent for the outlet");
            choose_color(s, t, sink, c, slot, ability, paid, remaining, false)
        }
        Some(paid) => {
            let color = color_of(option);
            activate(s, t, sink, c, slot, ability, paid, color, remaining)
        }
    }
}

/// The options of [`CastStep::Manual`] where the payer controls an outlet: each tap and each
/// floating mana that [`mana::manual_candidates`] lists, then each outlet, each only if the
/// rest of `remaining` is still payable after it, counting what outlets add.
pub(super) fn manual_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    remaining: ManaCost,
    paying: Paying<L>,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    manual_options_keeping(s, t, player, remaining, paying, &casting_keeps(t))
}

fn manual_options_keeping<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    remaining: ManaCost,
    paying: Paying<L>,
    keeps: Keeps<'_, L>,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for &option in mana::manual_candidates(s, t, player, remaining, paying)?.iter() {
        let mut rest = remaining;
        let after = simulate(s, t, |after, sink| {
            rest = mana::manual_pay(after, t, sink, player, remaining, option, paying)?.0;
            Ok(())
        })?;
        if keeps(&after, rest)? {
            out.push(option)?;
        }
    }
    for o in &mana::outlets(s, t, player, Bitboard::EMPTY) {
        if !takes(s, t, o, remaining, keeps)?.is_empty() {
            out.push(Opt::Activate {
                slot: o.slot,
                ability: o.ability,
            })?;
        }
    }
    Ok(out)
}

/// `player` pays `cost` while a spell or ability resolves ([`super::payment::pay`]), where
/// they control an outlet: by hand under [`Payment::Manual`], else by plan, with the outlets
/// and the mana abilities of the permanents they could sacrifice among the options. Each
/// option keeps the rest payable, so the loop ends with the cost paid.
pub(super) fn pay_resolving<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cost: ManaCost,
) -> crate::card::Flow {
    let t = game.table;
    let keeps = move |after: &State<L>, cost: ManaCost| {
        mana::payable(after, t, player, cost, Bitboard::EMPTY, Paying::NOTHING)
    };
    let paying = Paying::NOTHING;
    let payment = game.state.profile().payment;
    let greedy = payment == Payment::Greedy;
    let mut remaining = cost;
    loop {
        let mut plans = Default::default();
        let option = if payment == Payment::Manual {
            if remaining.is_zero() {
                return Ok(());
            }
            let options = manual_options_keeping(game.state, t, player, remaining, paying, &keeps)?;
            let step = Prompt::new(Purpose::Mana).cost(remaining);
            crate::interpret::choices::pay(game, resolution, player, options.as_slice(), step)?
        } else {
            let none = Bitboard::EMPTY;
            plans = mana::plans(game.state, t, game.sink, player, cost, none, greedy, paying)?;
            let mut options: L::Queue<Opt<L>, OPTIONS> = Default::default();
            for i in 0..plans.len() {
                options.push(Opt::Plan(index::<L>(i)))?;
            }
            if !greedy || options.is_empty() {
                step_options(game.state, t, player, cost, paying, &keeps, &mut options)?;
            }
            if greedy {
                options.as_slice()[0]
            } else {
                let step = Prompt::new(Purpose::Mana).cost(cost).part(Part::Plan);
                crate::interpret::choices::pay(game, resolution, player, options.as_slice(), step)?
            }
        };
        let (s, sink) = (&mut *game.state, &mut *game.sink);
        if let Opt::Plan(i) = option {
            let plan = &plans.as_slice()[Into::<usize>::into(i)];
            sink.enter_action(player);
            let paid = mana::pay(s, t, sink, player, cost, Bitboard::EMPTY, plan, paying);
            sink.leave();
            paid?;
            return Ok(());
        }
        if let Opt::Activate { slot, ability } = option
            && is_outlet(s, t, slot, ability)
        {
            let o = outlet(s, t, player, slot, ability);
            let able = takes(s, t, &o, remaining, &keeps)?;
            let paid = if greedy || able.count() == 1 {
                able.min(|slot| slot)
                    .expect("an outlet offered takes a permanent")
            } else {
                let d = pick_decision(&o, player, able);
                let c = resolution.answer(game, &d)?;
                d.candidates.object(c).expect("an answer among the objects")
            };
            let colors = colors(game.state, t, &o, paid, remaining, &keeps)?;
            let color = if greedy || colors.is_power_of_two() {
                ManaColor::ALL[colors.trailing_zeros() as usize]
            } else {
                let d = color_decision(t, player, colors, Bitboard::single(slot))?;
                let c = resolution.answer(game, &d)?;
                color_of(d.candidates.option(c))
            };
            let (s, sink) = (&mut *game.state, &mut *game.sink);
            sink.enter_action(player);
            let done = mana::activate_outlet(s, t, sink, slot, ability, paid, color);
            sink.leave();
            done?;
        } else {
            let owed = if payment == Payment::Manual {
                remaining
            } else {
                ManaCost::NONE
            };
            sink.enter_action(player);
            let left = mana::manual_pay(s, t, sink, player, owed, option, paying);
            sink.leave();
            if payment == Payment::Manual {
                remaining = left?.0;
            } else {
                left?;
            }
        }
    }
}
