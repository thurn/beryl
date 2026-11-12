//! The steps of casting a spell (CR 601.2a to 601.2i) and activating an ability (CR 602.2a), in
//! order: announce, targets, pay, and the spell becomes cast.

use crate::bitboard::Bitboard;
use crate::card::{CastingIx, ManaCost, ObjectAbility, Part, Prompt, Purpose, SpecIx, TargetSpec};
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::engine::collapse;
use crate::event::{self, Event, Reason, Sink};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::keywords::flashback;
use crate::layout::{Layout, Queue, Table, index};
use crate::mana::{self, Funds};
use crate::profile::Payment;
use crate::state::{
    CastStep, Casting, CounterKind, Pending, StackEntry, State, Target, Tracker, Zone,
};
use crate::table::GameTable;
use crate::{casting, counters};

use super::costs::{ExileCost, exile_choices, spent_by_entry, total_cost, x_range_start};
use super::legal::{benders, crew_candidates, power_of, waterbend_payable};
use super::targets::{TargetSet, source_of, target_options};
use super::timing::{activated, entry_ability, entry_ability_of};

/// Begins casting a spell: moves it to the stack and starts the machine.
///
/// [CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "To propose the casting of
/// a spell, a player first moves that card ... from where it is to the stack. It becomes the
/// topmost object on the stack. ... and that player becomes its controller."
///
/// [CR 712.8b](https://yawgatog.com/resources/magic-rules/#R7128b): "A player casting a modal
/// double-faced card ... chooses which face they are casting before putting it onto the
/// stack."
pub(crate) fn begin_cast<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    option: Opt<L>,
    resolving: Option<bool>,
) -> Result<(), Full> {
    let Opt::Cast { slot, face, costs } = option else {
        unreachable!("a cast option");
    };
    let permission = casting::permission(s, player, slot);
    event::move_one(s, t, sink, slot, Zone::Stack, Reason::Cast)?;
    let (card, _) = s.copiable(slot).record(t);
    s.become_spell(slot, face, player);
    s.put_on_stack(StackEntry {
        object: Some(s.object_ref(slot)),
        controller: player,
        ability: None,
        card,
        face,
        costs,
        ..StackEntry::default()
    })?;
    if let Some(permission) = permission {
        casting::begin_on_spell(s, slot, permission)?;
    }
    let free = resolving.unwrap_or(false);
    let def_face = t.def(card).face(face);
    let c = Casting {
        player,
        step: if x_range_start(def_face, costs, free).is_some() {
            CastStep::X
        } else {
            CastStep::TargetCount
        },
        spec: SpecIx::default(),
        graveyard_from: 0,
        left: 0,
        free,
        resolving: resolving.is_some(),
        remaining: ManaCost::NONE,
        bent: 0,
        triggered: false,
    };
    advance(s, t, sink, c)
}

/// Begins activating an ability: puts it on the stack and starts the machine.
///
/// [CR 602.2a](https://yawgatog.com/resources/magic-rules/#R6022a): "That ability is created
/// on the stack as an object that's not a card. It becomes the topmost object on the stack.
/// ... Its controller is the player who activated the ability."
pub(crate) fn begin_activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Result<(), Full> {
    let (source, index) = activated(s, slot, ability).expect("an ability offered");
    let a = source.face(t).ability(index);
    let (card, face) = source.record(t);
    if a.is_loyalty() {
        s.track(t, Tracker::LoyaltyActivated, slot);
    }
    s.put_on_stack(StackEntry {
        object: Some(s.object_ref(slot)),
        controller: player,
        ability: Some(index),
        card,
        face,
        ..StackEntry::default()
    })?;
    let c = Casting {
        player,
        step: if a.cost.mana.x > 0 {
            CastStep::X
        } else {
            CastStep::TargetCount
        },
        spec: SpecIx::default(),
        graveyard_from: 0,
        left: 0,
        free: false,
        resolving: false,
        remaining: ManaCost::NONE,
        bent: 0,
        triggered: false,
    };
    advance(s, t, sink, c)
}

pub(super) fn top<L: Layout>(s: &State<L>) -> StackEntry<L> {
    s.stack()
        .last()
        .expect("the spell or ability being cast")
        .clone()
}

/// The target specs of the spell or ability being cast.
pub(super) fn specs<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> &'static [TargetSpec] {
    entry_ability(t, entry).map_or(&[], |a| a.targets)
}

/// Moves the machine on to its next question, or finishes.
pub(super) fn advance<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
) -> Result<(), Full> {
    loop {
        match c.step {
            CastStep::X
            | CastStep::Plan
            | CastStep::Manual
            | CastStep::Crew
            | CastStep::Waterbend
            | CastStep::Exile => break,
            CastStep::TargetCount | CastStep::Target => {
                let entry = top(s);
                let specs = specs(t, &entry);
                let Some(spec) = specs.get(c.spec.index()) else {
                    if c.triggered {
                        s.settle_question();
                        sink.push(ability_event(&entry));
                        return Ok(());
                    }
                    let crew = entry_ability(t, &entry).map_or(0, |a| a.cost.crew);
                    if crew == 0 {
                        return begin_payment(s, t, sink, c);
                    }
                    c.step = CastStep::Crew;
                    c.left = crew;
                    break;
                };
                if c.step == CastStep::TargetCount {
                    // CR 601.2c: "If the spell has a variable number of targets, the player
                    // announces how many targets they will choose before they announce those
                    // targets."
                    if spec.optional {
                        break;
                    }
                    c.left = spec.count;
                    c.step = CastStep::Target;
                }
                if c.left > 0 {
                    break;
                }
                c.spec = c.spec.next();
                c.step = CastStep::TargetCount;
            }
        }
    }
    s.ask(Pending::Casting(c));
    Ok(())
}

/// Pays an ability's non-mana costs, then turns to mana. A spell's exile choices follow
/// mana payment, so its caster sees the mana spent before selecting the other costs.
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. First, they pay all costs that don't involve random elements or moving objects
/// from the library to a public zone, in any order." The solver never taps a source that pays
/// with itself (`spent_by_entry`), so paying an ability's costs first changes nothing.
fn begin_payment<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
) -> Result<(), Full> {
    let entry = top(s);
    if !entry.is_spell() {
        let a = entry_ability_of(t, &entry);
        let source = entry.object.expect("an activated ability's source");
        if a.cost.tap {
            event::set_tapped(s, t, sink, source, true)?;
        }
        if let Some(n) = a.cost.loyalty {
            // CR 606.4: "The cost to activate a loyalty ability of a permanent is to put on or
            // remove from that permanent a certain number of loyalty counters".
            let (game, amount) = (&mut Game::new(s, t, sink), n.unsigned_abs());
            if n > 0 {
                counters::add(game, source, CounterKind::Loyalty, amount)?;
            } else if n < 0 {
                counters::remove(game, source, CounterKind::Loyalty, amount)?;
            }
        }
        if a.cost.sacrifice_self {
            event::move_one(s, t, sink, source.slot, Zone::Graveyard, Reason::Sacrifice)?;
        }
        if a.cost.discard_self {
            event::move_one(s, t, sink, source.slot, Zone::Graveyard, Reason::Discard)?;
        }
        if let Some(kind) = a.cost.remove {
            counters::remove(&mut Game::new(s, t, sink), source, kind, 1)?;
        }
        if a.cost.waterbend > 0 {
            c.left = a.cost.waterbend.min(total_cost(s, t, &entry, &c).generic);
            if c.left > 0 && !waterbend_options(s, t, &c)?.is_empty() {
                c.step = CastStep::Waterbend;
                s.ask(Pending::Casting(c));
                return Ok(());
            }
        }
    }
    pay_mana(s, t, sink, c)
}

/// The answers of a waterbend step (CR 701.67a): tap one more artifact or creature, while the
/// rest stays payable, or pay what is left with mana, when it can be. Interchangeable ones are
/// one answer (§7.2). The human can also activate a mana ability
/// ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g)): "the player then has a
/// chance to activate mana abilities". Empty when there is no choice to make.
fn waterbend_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    let entry = top(s);
    let cost = total_cost(s, t, &entry, c);
    let mut objects: L::Queue<Opt<L>, 64> = Default::default();
    if c.left > 0 {
        let mut fewer = cost;
        fewer.generic -= 1;
        let funds = Funds::of(s, t, c.player)?;
        // Candidates are listed in the candidate order, which a `Choice` counts in.
        for slot in s.in_candidate_order(collapse(s, benders(s, c.player))) {
            let rest = Bitboard::single(slot);
            if waterbend_payable(s, t, &funds, c.player, fewer, c.left - 1, rest) {
                objects.push(Opt::Object(slot))?;
            }
        }
    }
    // With only generic symbols, activating a mana source keeps at least as much payment
    // available as it did untapped. Activate before tapping any permanent as a bender.
    let manual_mana = s.profile().taps_mana == Some(c.player)
        && c.bent == 0
        && cost.symbols.iter().all(|&n| n == 0);
    if objects.is_empty() && !manual_mana {
        return Ok(out);
    }
    if mana::payable(s, t, c.player, cost, Bitboard::EMPTY)? {
        out.push(Opt::Done)?;
    }
    for &o in objects.iter() {
        out.push(o)?;
    }
    // Append the mana choices so existing candidate indices hold.
    if manual_mana {
        mana::waterbend_mana_options(s, t, c.player, &mut out);
    }
    if out.as_slice() == [Opt::Done] {
        return Ok(Default::default());
    }
    Ok(out)
}

/// Pays the mana of the total cost: by plan, or one source at a time under manual payment.
fn pay_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
) -> Result<(), Full> {
    let entry = top(s);
    let cost = total_cost(s, t, &entry, &c);
    match s.profile().payment {
        Payment::Manual => {
            c.remaining = cost;
            if cost.is_zero() {
                return after_mana(s, t, sink, c);
            }
            c.step = CastStep::Manual;
            s.ask(Pending::Casting(c));
            Ok(())
        }
        payment => {
            let plans = mana::plans(
                s,
                t,
                sink,
                c.player,
                cost,
                Bitboard::EMPTY,
                payment == Payment::Greedy,
            )?;
            debug_assert!(!plans.is_empty(), "a spell offered is payable");
            if plans.len() > 1 {
                c.step = CastStep::Plan;
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            if let Some(plan) = plans.as_slice().first() {
                mana::pay(s, t, sink, c.player, cost, Bitboard::EMPTY, plan)?;
            }
            after_mana(s, t, sink, c)
        }
    }
}

/// Mana production and spending have completed before choosing cards for an exile cost.
/// [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "Mana abilities must be
/// activated before costs are paid." CR 601.2h permits spending that mana before exiling
/// cards; it does not permit activating mana abilities after paying the exile cost. Keeping
/// the unfinished cast pending also keeps session cancellation valid.
fn after_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
) -> Result<(), Full> {
    let entry = top(s);
    if entry.is_spell() {
        let face = t.def(entry.card).face(entry.face);
        if let Some(cost) = ExileCost::of(face, entry.costs) {
            c.step = CastStep::Exile;
            c.left = cost.count();
            c.graveyard_from = 0;
            s.ask(Pending::Casting(c));
            return Ok(());
        }
    }
    finish(s, t, sink, c)
}

/// The spell becomes cast, or the ability activated.
///
/// [CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i): "Any abilities that
/// trigger when a spell is cast or put onto the stack trigger at this time. If the spell's
/// controller had priority before casting it, they get priority." CR 608.2g: after a spell
/// cast during a resolution, "no player receives priority"; the resolution it was cast in is
/// over by now, so the active player receives priority (CR 117.3b).
fn finish<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting,
) -> Result<(), Full> {
    s.settle_question();
    let active = s.turn().active();
    s.give_priority(if c.resolving { active } else { c.player });
    let entry = top(s);
    if let Some(spell) = entry.spell() {
        let face = t.def(entry.card).face(entry.face);
        for (i, option) in face.casting.iter().enumerate() {
            if entry.costs.contains(CastingIx::new(i as u8)) {
                crate::coverage::hit(option);
            }
        }
        event::settle(
            s,
            t,
            sink,
            Event::Cast {
                spell,
                controller: entry.controller,
                targets: entry.targets.clone(),
                x: entry.x,
                costs: entry.costs,
                flashback: flashback::paid(s, spell),
            },
        )?;
    } else {
        sink.push(ability_event(&entry));
    }
    Ok(())
}

/// The [`Event::Ability`] that says `entry`, an ability on the stack. It goes straight to the
/// sink: no trigger listens to it and it changes nothing.
pub(super) fn ability_event<L: Layout>(entry: &StackEntry<L>) -> Event<L> {
    Event::Ability {
        source: entry.object,
        controller: entry.controller,
        card: entry.card,
        face: entry.face,
        ability: entry.ability.expect("an ability's entry"),
        targets: entry.targets.clone(),
        x: entry.x,
    }
}

/// The X values the payment can still cover, from the least allowed.
fn x_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let entry = top(s);
    let face = t.def(entry.card).face(entry.face);
    let least = if entry.is_spell() {
        x_range_start(face, entry.costs, c.free).unwrap_or(0)
    } else {
        0
    };
    let exclude = spent_by_entry(t, &entry);
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    for x in least..(least.saturating_add(64)) {
        let mut probe = entry.clone();
        probe.x = x;
        if !mana::payable(s, t, c.player, total_cost(s, t, &probe, c), exclude)? {
            break;
        }
        out.push(Opt::Number(x))?;
    }
    Ok(out)
}

/// The open question of the machine.
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: &Casting,
) -> Result<Decision<L>, Full> {
    let options = |kind: DecisionKind, list: L::Queue<Opt<L>, 64>, prompt: Prompt| Decision {
        player: c.player,
        kind,
        candidates: Candidates::Options(list),
        prompt,
        about: Bitboard::EMPTY,
    };
    // The targets of one spec are one prompt, which says how many are still to choose.
    let target = Prompt::new(Purpose::Target).most(c.left);
    Ok(match c.step {
        CastStep::X => options(
            DecisionKind::X,
            x_options(s, t, c)?,
            Prompt::new(Purpose::X),
        ),
        CastStep::TargetCount => {
            let entry = top(s);
            let spec = &specs(t, &entry)[c.spec.index()];
            let most = target_options(s, t, c).count().min(u32::from(spec.count));
            let mut list: L::Queue<Opt<L>, 64> = Default::default();
            for n in 0..=most {
                list.push(Opt::Number(n as u8))?;
            }
            let count = Prompt::new(Purpose::Target)
                .most(spec.count)
                .part(Part::Count);
            options(DecisionKind::TargetCount, list, count)
        }
        CastStep::Target => match target_options(s, t, c) {
            TargetSet::Objects(b) => Decision {
                player: c.player,
                kind: DecisionKind::Target,
                candidates: Candidates::Objects(collapse(s, b)),
                prompt: target,
                about: Bitboard::EMPTY,
            },
            TargetSet::Players(p) => {
                let mut list: L::Queue<Opt<L>, 64> = Default::default();
                // Candidates are listed by seat, which a `Choice` counts in.
                for player in s.players_in_seat_order(p) {
                    list.push(Opt::Player(player))?;
                }
                options(DecisionKind::Target, list, target)
            }
        },
        CastStep::Plan => {
            let entry = top(s);
            let cost = total_cost(s, t, &entry, c);
            let plans = mana::plans(s, t, sink, c.player, cost, Bitboard::EMPTY, false)?;
            let mut list: L::Queue<Opt<L>, 64> = Default::default();
            for i in 0..plans.len() {
                list.push(Opt::Plan(index::<L>(i)))?;
            }
            let plan = Prompt::new(Purpose::Mana).cost(cost).part(Part::Plan);
            options(DecisionKind::PaymentPlan, list, plan)
        }
        CastStep::Crew => {
            let entry = top(s);
            Decision {
                player: c.player,
                kind: DecisionKind::ChooseObject,
                candidates: Candidates::Objects(collapse(
                    s,
                    crew_candidates(s, c.player, source_of(&entry).expect("a Vehicle to crew")),
                )),
                // The crew cost's power, and what of it is still to tap.
                prompt: Prompt::new(Purpose::Crew)
                    .count(entry_ability(t, &entry).map_or(0, |a| a.cost.crew))
                    .most(c.left),
                about: Bitboard::EMPTY,
            }
        }
        CastStep::Manual => options(
            DecisionKind::ManaPayment,
            mana::manual_options(s, t, c.player, c.remaining, Bitboard::EMPTY)?,
            Prompt::new(Purpose::Mana).cost(c.remaining),
        ),
        CastStep::Waterbend => options(
            DecisionKind::ManaPayment,
            waterbend_options(s, t, c)?,
            Prompt::new(Purpose::Waterbend).cost(total_cost(s, t, &top(s), c)),
        ),
        // The cards lie in the zone the cost exiles from, the hand or the graveyard.
        CastStep::Exile => Decision {
            player: c.player,
            kind: DecisionKind::ChooseObject,
            candidates: Candidates::Objects(exile_choices(s, t, c)),
            prompt: Prompt::new(Purpose::ExileCost).most(c.left),
            about: Bitboard::EMPTY,
        },
    })
}

/// Applies an answer to the machine's question: an option, or an object for a target.
pub(crate) fn perform<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting,
    option: Option<Opt<L>>,
    object: Option<L::Slot>,
) -> Result<(), Full> {
    match c.step {
        CastStep::X => {
            let Some(Opt::Number(x)) = option else {
                unreachable!("an X answer");
            };
            s.announce_x(x);
            c.step = CastStep::TargetCount;
        }
        CastStep::TargetCount => {
            let Some(Opt::Number(n)) = option else {
                unreachable!("a target count");
            };
            c.left = n;
            c.step = CastStep::Target;
        }
        CastStep::Target => {
            let target = match (option, object) {
                (Some(Opt::Player(player)), _) => Target::player(c.spec, player),
                (_, Some(slot)) => Target::object(c.spec, s.object_ref(slot)),
                _ => unreachable!("a target"),
            };
            s.announce_target(target)?;
            c.left -= 1;
        }
        CastStep::Plan => {
            let Some(Opt::Plan(i)) = option else {
                unreachable!("a payment plan");
            };
            let entry = top(s);
            let cost = total_cost(s, t, &entry, &c);
            let plans = mana::plans(s, t, sink, c.player, cost, Bitboard::EMPTY, false)?;
            mana::pay(
                s,
                t,
                sink,
                c.player,
                cost,
                Bitboard::EMPTY,
                &plans.as_slice()[Into::<usize>::into(i)],
            )?;
            return after_mana(s, t, sink, c);
        }
        CastStep::Crew => {
            // CR 702.122b: "A creature 'crews a Vehicle' when it's tapped to pay the cost to
            // activate a Vehicle's crew ability."
            let slot = object.expect("a creature to crew with");
            let power = power_of(s, slot);
            event::set_tapped(s, t, sink, s.object_ref(slot), true)?;
            c.left = c.left.saturating_sub(power);
            if c.left == 0 {
                return begin_payment(s, t, sink, c);
            }
        }
        CastStep::Waterbend => {
            // CR 701.67a: "you may tap an untapped artifact or creature you control rather
            // than pay that mana."
            if let Some(Opt::Mana { slot, color }) = option {
                mana::activate(s, t, sink, slot, color)?;
                if waterbend_options(s, t, &c)?.is_empty() {
                    return pay_mana(s, t, sink, c);
                }
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            let Some(Opt::Object(slot)) = option else {
                return pay_mana(s, t, sink, c);
            };
            event::set_tapped(s, t, sink, s.object_ref(slot), true)?;
            c.bent += 1;
            c.left -= 1;
            if waterbend_options(s, t, &c)?.is_empty() {
                return pay_mana(s, t, sink, c);
            }
            s.ask(Pending::Casting(c));
            return Ok(());
        }
        CastStep::Exile => {
            // CR 601.2h: "The player pays the total cost." The cards go from the hand or the
            // graveyard to exile as part of it.
            let slot = object.expect("a card to exile");
            if let Some(i) = s.graveyard(c.player).iter().position(|&g| g == slot) {
                // The cards above it move down one, so the next pick starts here.
                c.graveyard_from = i as u8;
            }
            event::move_one(s, t, sink, slot, Zone::Exile, Reason::Exile)?;
            c.left -= 1;
            if c.left > 0 {
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            return finish(s, t, sink, c);
        }
        CastStep::Manual => {
            let option = option.expect("a manual payment");
            c.remaining = mana::manual_pay(s, t, sink, c.player, c.remaining, option)?;
            if c.remaining.is_zero() {
                return after_mana(s, t, sink, c);
            }
        }
    }
    advance(s, t, sink, c)
}
