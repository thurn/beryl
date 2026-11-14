//! Paying the parts of a cost that are not mana (CR 601.2h): what is done to the object the
//! cost is on, and the one step in which a player chooses the objects a part is paid with.
//!
//! [CR 118.1](https://yawgatog.com/resources/magic-rules/#R1181): "To pay a cost, a player
//! carries out the instructions specified by the spell, ability, or effect that contains that
//! cost." A cost's parts are data ([`CostPart`]), and a card table holds only the parts paid
//! here (`beryl_kernel`'s `payable`), so every other arm is unreachable.

use crate::bitboard::Bitboard;
use crate::card::{
    Castings, Chosen, CostAction, CostPart, Face, ManaCost, Measure, Prompt, Purpose,
};
use crate::counters;
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::engine::{collapse, collapse_in_order};
use crate::event::{self, Entry, Reason, Sink};
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::life;
use crate::mana::{self, Funds};
use crate::state::{
    CastKind, CastStep, Casting, CounterKind, PayingObjects, Pending, StackEntry, StackItem, State,
    Zone, ZoneKey,
};
use crate::table::GameTable;

use super::costs::total_cost;
use super::steps::{finish, pay_mana, top};
use super::timing::entry_ability_of;

/// The part at `index` of the cost of the spell or ability `entry`, counting its parts that
/// are not mana from 0 in the order printed.
fn part_at<L: Layout>(
    t: &GameTable,
    entry: &StackEntry<L>,
    index: u8,
) -> Option<&'static CostPart> {
    let index = usize::from(index);
    let StackItem::Spell {
        card, face, costs, ..
    } = entry.item
    else {
        return entry_ability_of(t, entry).cost().parts().get(index);
    };
    let face = t.def(card).face(face);
    // Most spells cost mana alone.
    if face.additional_cost.is_empty() && costs.is_empty() {
        return None;
    }
    face.cost_parts(costs).nth(index)
}

/// The object the cost of `entry` is on: the spell's card, or the ability's source.
fn itself<L: Layout>(entry: &StackEntry<L>) -> L::Slot {
    let object = entry
        .object()
        .expect("a spell's card or an activated ability's source");
    object.slot
}

/// The part being paid with objects, and what it does to them.
fn part_being_paid<L: Layout>(
    t: &GameTable,
    entry: &StackEntry<L>,
    c: &Casting<L>,
) -> (CostAction, &'static Chosen) {
    match part_at(t, entry, step(c).part) {
        Some(CostPart::Chosen(action, chosen)) => (*action, chosen),
        _ => unreachable!("a part paid with objects"),
    }
}

/// Where the casting at [`CastStep::Objects`], the step every function here but [`pay`] runs
/// in, has got to.
fn step<L: Layout>(c: &Casting<L>) -> PayingObjects<L> {
    match c.step {
        CastStep::Objects(paying) => paying,
        _ => unreachable!("a casting paying with objects"),
    }
}

/// The part of an ability's cost paid rather than generic mana, with its place among the
/// parts and the most mana it pays ([`Measure::ForGeneric`]). A card table holds no cost
/// with two.
pub(super) fn substitute(
    parts: &'static [CostPart],
) -> Option<(u8, CostAction, &'static Chosen, u8)> {
    parts.iter().zip(0..).find_map(|(part, index)| match part {
        CostPart::Chosen(action, chosen) => match chosen.measure {
            Measure::ForGeneric(n) => Some((index, *action, chosen, n)),
            Measure::Count(_) | Measure::TotalPower(_) => None,
        },
        _ => None,
    })
}

/// The objects `player` may pay a part with: of those in the part's zone that are the
/// player's, the ones its filter accepts, without `itself`, the object the cost is on, where
/// the part says "other". A spell is never among them: its caster moves it to the stack
/// before paying (CR 601.2a), so it can't pay for itself.
///
/// On the battlefield they are permanents the player controls, and untapped ones for a part
/// that taps them ([CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183): "a
/// permanent that's already tapped can't be tapped to pay a cost"). Summoning sickness doesn't
/// stop them: tapping them is a cost of this ability, not an ability of theirs with {T}
/// (CR 302.6). Elsewhere they are cards
/// ([CR 111.6](https://yawgatog.com/resources/magic-rules/#R1116): "A token isn't a card"): a
/// sacrificed token remains in a graveyard until state-based actions are checked.
pub(super) fn candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    itself: L::Slot,
    action: CostAction,
    chosen: &Chosen,
) -> Bitboard<L::Slot> {
    match chosen.zone {
        Zone::Battlefield => s
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| {
                !(chosen.other && slot == itself)
                    && s.controller(slot) == player
                    && chosen.filter.matches(s, t, slot, player)
                    && !(action == CostAction::Tap && s.object(slot).is_tapped())
            })
            .collect(),
        zone => {
            let mut cards = s.zone(ZoneKey::Of(player, zone));
            cards.remove(itself);
            cards.filter(|slot| {
                s.card_behind(slot).is_some() && chosen.filter.matches(s, t, slot, player)
            })
        }
    }
}

/// A creature's power counted toward a total; less than 0 counts as 0.
fn power_of<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    u8::try_from(s.derived(slot).power.max(0)).unwrap_or(0)
}

/// Whether `player` has the objects to pay a part with
/// ([CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183)): "A player can't pay a
/// cost without having the necessary resources to pay it fully." A part paid rather than
/// generic mana is always payable: its player may pay the mana.
pub(super) fn payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    itself: L::Slot,
    action: CostAction,
    chosen: &Chosen,
) -> bool {
    match chosen.measure {
        Measure::Count(n) if chosen.zone != Zone::Battlefield => {
            let mut cards = s.zone(ZoneKey::Of(player, chosen.zone));
            cards.remove(itself);
            cards.at_least(usize::from(n), |slot| {
                s.card_behind(slot).is_some() && chosen.filter.matches(s, t, slot, player)
            })
        }
        Measure::Count(n) => {
            candidates(s, t, player, itself, action, chosen).count() >= u32::from(n)
        }
        Measure::TotalPower(n) => {
            candidates(s, t, player, itself, action, chosen)
                .sum(|slot| u32::from(power_of(s, slot)))
                >= u32::from(n)
        }
        Measure::ForGeneric(_) => true,
    }
}

/// Whether `player` has the objects to pay every part of the cost of an ability of the object
/// in `slot` that is paid with objects.
pub(super) fn ability_payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    parts: &'static [CostPart],
) -> bool {
    parts.iter().all(|part| match part {
        CostPart::Chosen(action, chosen) => payable(s, t, player, slot, *action, chosen),
        _ => true,
    })
}

/// Whether `player` can pay every payment of life among `parts`
/// ([CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194)): "If a cost or effect allows
/// a player to pay an amount of life greater than 0, the player may do so only if their life
/// total is greater than or equal to the amount of the payment."
pub(super) fn life_payable<L: Layout>(s: &State<L>, player: Seat, parts: &[CostPart]) -> bool {
    let life = s.player(player).life();
    parts.iter().all(|part| match part {
        CostPart::Life(n) => life >= i16::from(*n),
        _ => true,
    })
}

/// Whether `player` has the objects to pay every part of the cost of casting `face` of the
/// card in `slot` with the options `costs`.
pub(super) fn spell_payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    face: &Face,
    costs: Castings,
) -> bool {
    // Most faces are cast with no cost but mana.
    if face.additional_cost.is_empty() && costs.is_empty() {
        return true;
    }
    face.cost_parts(costs).all(|part| match part {
        CostPart::Chosen(action, chosen) => payable(s, t, player, slot, *action, chosen),
        _ => unreachable!("the card table refuses a spell's cost the engine cannot pay"),
    })
}

/// Whether `player` can pay `cost`, paying up to `most` of its generic mana with one of
/// `objects` each rather than with mana (CR 701.67a), and with none of `exclude`. An object
/// that makes no mana now pays as much as it ever can, so those pay first; a mana source
/// pays at least as much tapped for mana, so it never needs to.
pub(super) fn payable_with_substitutes<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    funds: &Funds<L>,
    mut cost: ManaCost,
    most: u8,
    objects: Bitboard<L::Slot>,
    mut exclude: Bitboard<L::Slot>,
) -> bool {
    let free = objects & !exclude & !funds.sources(exclude);
    // Any of them do: each pays one generic mana, and none is a source the funds count.
    for slot in s
        .in_candidate_order(free)
        .take(usize::from(most.min(cost.generic)))
    {
        cost.generic -= 1;
        exclude.insert(slot);
    }
    funds.pays(s, t, cost, exclude)
}

/// Does `action` to the source of the ability `entry`, as the part of its cost that says so.
fn pay_with_source<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    action: CostAction,
) -> Result<(), Full> {
    let source = top(s).object().expect("an activated ability's source");
    match action {
        CostAction::Tap => event::set_tapped(s, t, sink, source, true),
        CostAction::Sacrifice => {
            event::move_one(s, t, sink, source.slot, Zone::Graveyard, Reason::Sacrifice)
        }
        CostAction::Discard => {
            event::move_one(s, t, sink, source.slot, Zone::Graveyard, Reason::Discard)
        }
        CostAction::RemoveCounter(kind) => {
            counters::remove(&mut Game::new(s, t, sink), source, kind, 1)
        }
        CostAction::Exile | CostAction::Reveal => {
            unreachable!("the card table refuses a cost the engine cannot pay")
        }
    }
}

/// Pays the parts of the cost from the part at `from` on, in the order printed, stopping at
/// one its player chooses objects for ([`CastStep::Objects`]); then turns to what follows
/// them. An ability's parts are paid before its mana and a spell's after it, so a spell's
/// caster sees the mana spent before choosing the cards it also costs.
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. First, they pay all costs that don't involve random elements or moving objects
/// from the library to a public zone, in any order." The solver never taps a source that pays
/// with itself (`spent_by_entry`), so paying an ability's parts first changes nothing.
pub(super) fn pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
    from: u8,
) -> Result<(), Full> {
    // Paying changes the state, so each read of the entry borrows it anew.
    let mut index = from;
    while let Some(part) = part_at(t, top(s), index) {
        match *part {
            CostPart::This(action) => pay_with_source(s, t, sink, action)?,
            CostPart::Loyalty(n) => {
                // CR 606.4: "The cost to activate a loyalty ability of a permanent is to put
                // on or remove from that permanent a certain number of loyalty counters".
                let source = top(s).object().expect("an activated ability's source");
                let (game, amount) = (&mut Game::new(s, t, sink), n.unsigned_abs());
                if n > 0 {
                    counters::add(game, source, CounterKind::Loyalty, amount)?;
                } else if n < 0 {
                    counters::remove(game, source, CounterKind::Loyalty, amount)?;
                }
            }
            CostPart::Chosen(_, chosen) => match chosen.measure {
                Measure::Count(n) | Measure::TotalPower(n) => {
                    c.step = CastStep::Objects(PayingObjects::start(index, n));
                    s.ask(Pending::Casting(c));
                    return Ok(());
                }
                // Paid as the mana is, after every other part ([`pay_substitutes`]).
                Measure::ForGeneric(_) => {}
            },
            CostPart::Life(n) => {
                // CR 119.4: "If a player pays life, the payment is subtracted from their life
                // total; in other words, the player loses that much life". The player is the one activating the ability.
                let player = top(s).controller();
                life::lose(&mut Game::new(s, t, sink), player, n)?;
            }
            CostPart::OneOf(_) => {
                unreachable!("the card table refuses a cost the engine cannot pay")
            }
        }
        index += 1;
    }
    match c.kind {
        CastKind::Activation => pay_substitutes(s, t, sink, c),
        _ => finish(s, t, sink, c),
    }
}

/// Turns to an ability's mana: first the part of its cost that may be paid rather than
/// generic mana, if it has one and there is a choice to make, then the mana itself.
fn pay_substitutes<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
) -> Result<(), Full> {
    let entry = top(s);
    if let Some((part, _, _, most)) = substitute(entry_ability_of(t, entry).cost().parts()) {
        let left = most.min(total_cost(s, t, entry, &c).generic);
        c.step = CastStep::Objects(PayingObjects::start(part, left));
        if left > 0 && !substitute_options(s, t, &c)?.is_empty() {
            s.ask(Pending::Casting(c));
            return Ok(());
        }
    }
    pay_mana(s, t, sink, c)
}

/// The answers while paying generic mana with objects (CR 701.67a): tap one more, while the
/// rest stays payable, or pay what is left with mana, when it can be. Interchangeable ones
/// are one answer (§7.2). The human can also activate a mana ability
/// ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g)): "the player then has a
/// chance to activate mana abilities". Empty when there is no choice to make.
fn substitute_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    let entry = top(s);
    let cost = total_cost(s, t, entry, c);
    let mut objects: L::Queue<Opt<L>, 64> = Default::default();
    let left = step(c).left;
    if left > 0 {
        let (action, chosen) = part_being_paid(t, entry, c);
        let able = candidates(s, t, c.player, itself(entry), action, chosen);
        let mut fewer = cost;
        fewer.generic -= 1;
        let funds = Funds::of(s, t, c.player)?;
        // Candidates are listed in the candidate order, which a `Choice` counts in.
        for slot in s.in_candidate_order(collapse(s, able)) {
            let rest = Bitboard::single(slot);
            if payable_with_substitutes(s, t, &funds, fewer, left - 1, able, rest) {
                objects.push(Opt::Object(slot))?;
            }
        }
    }
    // With only generic symbols, activating a mana source keeps at least as much payment
    // available as it did untapped. Activate before tapping any permanent to pay.
    let manual_mana = s.profile().taps_mana == Some(c.player)
        && c.substituted == 0
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

/// The objects the step may choose next, one of each set of interchangeable ones (§7.2).
/// From a graveyard: the cards from position `graveyard_from` up that leave enough above
/// them for the `left - 1` still to choose after, the lowest of interchangeable ones, which
/// leaves every choice the others would. So every set of cards is one sequence of answers,
/// and a graveyard with no more cards than the cost takes asks nothing.
fn choices<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    c: &Casting<L>,
) -> Bitboard<L::Slot> {
    let (action, chosen) = part_being_paid(t, entry, c);
    let PayingObjects {
        graveyard_from,
        chosen: picked,
        left,
        ..
    } = step(c);
    let able = candidates(s, t, c.player, itself(entry), action, chosen) & !picked;
    match (chosen.measure, chosen.zone) {
        (Measure::Count(_), Zone::Graveyard) => {
            let graveyard = s.graveyard(c.player);
            let last = graveyard
                .iter()
                .enumerate()
                .rev()
                .filter(|&(_, &slot)| able.contains(slot))
                .nth(usize::from(left) - 1)
                .expect("enough cards to pay the cost")
                .0;
            collapse_in_order(
                s,
                graveyard[usize::from(graveyard_from)..=last]
                    .iter()
                    .copied()
                    .filter(|&slot| able.contains(slot)),
            )
        }
        _ => collapse(s, able),
    }
}

/// What a player is asked as they choose the objects of a part: the prompts that exist are
/// crew's, a waterbend cost's and a card to exile.
fn purpose(action: CostAction, chosen: &Chosen) -> Purpose {
    match (action, chosen.measure) {
        (CostAction::Tap, Measure::TotalPower(_)) => Purpose::Crew,
        (CostAction::Tap, Measure::ForGeneric(_)) => Purpose::Waterbend,
        (CostAction::Exile, Measure::Count(_)) => Purpose::ExileCost,
        _ => unreachable!("the card table refuses a cost the engine cannot pay"),
    }
}

/// The question of [`CastStep::Objects`].
pub(super) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
) -> Result<Decision<L>, Full> {
    let entry = top(s);
    let (action, chosen) = part_being_paid(t, entry, c);
    let purpose = purpose(action, chosen);
    let left = step(c).left;
    Ok(match chosen.measure {
        // Each object pays one generic mana, or the rest is paid with mana.
        Measure::ForGeneric(_) => Decision {
            player: c.player,
            kind: DecisionKind::ManaPayment,
            candidates: Candidates::Options(substitute_options(s, t, c)?),
            prompt: Prompt::new(purpose).cost(total_cost(s, t, entry, c)),
            about: Bitboard::EMPTY,
        },
        // The total the part asks for, and what of it is still to pay.
        Measure::TotalPower(n) => Decision {
            player: c.player,
            kind: DecisionKind::ChooseObject,
            candidates: Candidates::Objects(choices(s, t, entry, c)),
            prompt: Prompt::new(purpose).count(n).most(left),
            about: Bitboard::EMPTY,
        },
        // The cards lie in the zone the part takes them from, the hand or the graveyard.
        Measure::Count(_) => Decision {
            player: c.player,
            kind: DecisionKind::ChooseObject,
            candidates: Candidates::Objects(choices(s, t, entry, c)),
            prompt: Prompt::new(purpose).most(left),
            about: Bitboard::EMPTY,
        },
    })
}

/// Does the part's action to the object chosen for it.
fn pay_with<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    action: CostAction,
    slot: L::Slot,
) -> Result<(), Full> {
    match action {
        CostAction::Tap => event::set_tapped(s, t, sink, s.object_ref(slot), true),
        _ => unreachable!("the card table refuses a cost the engine cannot pay"),
    }
}

/// Does the part's action to all the objects chosen for it, at once.
fn pay_with_all<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    action: CostAction,
    objects: Bitboard<L::Slot>,
) -> Result<(), Full> {
    match action {
        CostAction::Exile => event::move_batch(
            s,
            t,
            sink,
            objects,
            Zone::Exile,
            Reason::Exile,
            Entry::default(),
        ),
        _ => unreachable!("the card table refuses a cost the engine cannot pay"),
    }
}

/// Applies an answer to [`CastStep::Objects`]: the object chosen pays, and the part is asked
/// again until it is paid.
pub(super) fn perform<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
    option: Option<Opt<L>>,
    object: Option<L::Slot>,
) -> Result<(), Full> {
    let (action, chosen) = part_being_paid(t, top(s), &c);
    let mut paying = step(&c);
    match chosen.measure {
        Measure::ForGeneric(_) => {
            // CR 701.67a: "you may tap an untapped artifact or creature you control rather
            // than pay that mana."
            match option {
                Some(Opt::Mana { slot, color }) => mana::activate(s, t, sink, slot, color)?,
                Some(Opt::Object(slot)) => {
                    pay_with(s, t, sink, action, slot)?;
                    c.substituted += 1;
                    paying.left -= 1;
                }
                _ => return pay_mana(s, t, sink, c),
            }
            c.step = CastStep::Objects(paying);
            if substitute_options(s, t, &c)?.is_empty() {
                return pay_mana(s, t, sink, c);
            }
            s.ask(Pending::Casting(c));
            return Ok(());
        }
        Measure::TotalPower(_) => {
            // CR 702.122b: "A creature 'crews a Vehicle' when it's tapped to pay the cost to
            // activate a Vehicle's crew ability."
            let slot = object.expect("an object to pay with");
            let power = power_of(s, slot);
            pay_with(s, t, sink, action, slot)?;
            paying.left = paying.left.saturating_sub(power);
        }
        Measure::Count(_) => {
            // CR 601.2h: "The player pays the total cost." The cards stay where they are
            // until the last is chosen, then go from the hand or the graveyard to exile at
            // once ([`pay_with_all`]).
            let slot = object.expect("an object to pay with");
            if let Some(i) = s.graveyard(c.player).iter().position(|&g| g == slot) {
                // The next pick starts above it.
                paying.graveyard_from = i as u8 + 1;
            }
            paying.chosen.insert(slot);
            paying.left -= 1;
            if paying.left == 0 {
                pay_with_all(s, t, sink, action, paying.chosen)?;
                paying.chosen = Bitboard::EMPTY;
            }
        }
    }
    if paying.left == 0 && !matches!(chosen.measure, Measure::ForGeneric(_)) {
        return pay(s, t, sink, c, paying.part + 1);
    }
    c.step = CastStep::Objects(paying);
    s.ask(Pending::Casting(c));
    Ok(())
}
