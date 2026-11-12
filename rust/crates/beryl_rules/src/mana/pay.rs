//! Tapping and paying: carrying out a plan or a manual choice.

use crate::bitboard::Bitboard;
use crate::card::{CostAction, ManaColor, ManaCost, Types};
use crate::decision::Opt;
use crate::event::{self, Sink};
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Table};
use crate::state::{PendingTrigger, State, Zone};
use crate::table::{GameTable, is_sacrificed, is_simple};

use super::pool::{helps, pay_unit};
use super::solver::{Plan, lowest_kind, spent_by_class};
use super::sources::{Class, Sources, allowed, controlled, source};
use super::{KINDS, Taps};

/// Pays `cost` by `plan`: taps its sources for the mana it needs, sacrificing those whose
/// ability says so, and spends it.
///
/// [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "If the total cost
/// includes a mana payment, the player then has a chance to activate mana abilities ... Mana
/// abilities must be activated before costs are paid."
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. ... Partial payments are not allowed." So the events of the payment, each
/// source's tap and the mana it makes, are kept as one group: the cost is paid or it is not.
pub(crate) fn pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    plan: &Plan<L>,
) -> Result<(), Full> {
    sink.together();
    let paid = pay_whole(s, t, sink, player, cost, exclude, plan);
    sink.apart();
    paid
}

/// [`pay`], inside the group it opens.
fn pay_whole<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    plan: &Plan<L>,
) -> Result<(), Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    let total = cost.total(0);
    let must: u32 = plan.spend.iter().map(|&n| u32::from(n)).sum::<u32>()
        + sources
            .classes()
            .iter()
            .zip(plan.taps.iter())
            .filter(|(c, _)| c.amount == 1)
            .map(|(_, &n)| u32::from(n))
            .sum::<u32>();
    let spent = spent_by_class(&sources, plan, total - must);
    // The units spent: (kinds, source slot or none for floating mana, how many).
    let mut units: L::Queue<(u8, Option<L::Slot>, u8), 64> = Default::default();
    for (k, &n) in plan.spend.iter().enumerate() {
        for _ in 0..n {
            units.push((1 << k, None, 1))?;
        }
    }
    let human = s.profile().taps_mana == Some(player);
    for (i, c) in sources.classes().iter().enumerate() {
        let tapped: L::Queue<L::Slot, 64> = if human {
            human_taps(s, c, plan.taps[i])?
        } else {
            by_card(s, c, plan.taps[i])?
        };
        if c.amount == 1 {
            for &slot in tapped.iter() {
                units.push((c.kinds, Some(slot), 1))?;
            }
        } else {
            // Spread the spent units as `spent_by_class` counted them.
            let mut rest = spent[i];
            for (j, &slot) in tapped.iter().enumerate() {
                let later = (tapped.len() - j - 1) as u32;
                let here = (rest - later).min(u32::from(c.amount));
                units.push((c.kinds, Some(slot), u8::try_from(here).unwrap_or(c.amount)))?;
                rest -= here;
            }
        }
    }
    let mut kinds = Default::default();
    assign::<L>(cost, units.as_slice(), &mut kinds)?;
    for (&(unit_kinds, source, n), &kind) in units.iter().zip(kinds.iter()) {
        let kind = kind.unwrap_or_else(|| lowest_kind(unit_kinds));
        if let Some(slot) = source {
            activate(s, t, sink, slot, ManaColor::ALL[kind])?;
        }
        s.spend_mana(player, ManaColor::ALL[kind], n);
    }
    debug_assert_eq!(s.player(player).mana().amounts(), plan.left);
    Ok(())
}

/// The first `n` members of class `c` in the order a payment taps them: by card, so which of
/// a class's cards it taps never depends on their slots, which the game's randomness chose as
/// it dealt the cards to them; copies of one card are alike to it.
fn by_card<L: Layout>(s: &State<L>, c: &Class<L>, n: u8) -> Result<L::Queue<L::Slot, 64>, Full> {
    let mut v: L::Queue<L::Slot, 64> = Default::default();
    let first = s.in_candidate_order(c.members).next();
    let first = first.map(|m| s.copiable(m).key());
    if c.members.all(|m| Some(s.copiable(m).key()) == first) {
        for slot in s.in_candidate_order(c.members).take(usize::from(n)) {
            v.push(slot)?;
        }
        return Ok(v);
    }
    for slot in s.in_candidate_order(c.members) {
        v.push(slot)?;
    }
    v.as_mut_slice().sort_by_key(|&slot| s.copiable(slot).key());
    while v.len() > usize::from(n) {
        v.pop();
    }
    Ok(v)
}

/// The `n` members of class `c` the human's payment taps: its creatures last, free to attack
/// or block as long as they can ([`Outlook`]).
#[cold]
#[inline(never)]
fn human_taps<L: Layout>(s: &State<L>, c: &Class<L>, n: u8) -> Result<L::Queue<L::Slot, 64>, Full> {
    let mut v = by_card(s, c, c.count())?;
    v.as_mut_slice()
        .sort_by_key(|&m| s.derived(m).types.intersects(Types::CREATURE));
    while v.len() > usize::from(n) {
        v.pop();
    }
    Ok(v)
}

/// One augmenting-path step of [`assign`]: finds a unit slot for symbol `sym`.
fn augment<S: SlotId>(
    sym: usize,
    symbols: &[usize],
    slots: &[usize],
    units: &[(u8, Option<S>, u8)],
    owner: &mut [Option<usize>],
    seen: &mut [bool],
) -> bool {
    for (j, &u) in slots.iter().enumerate() {
        if units[u].0 & (1 << symbols[sym]) == 0 || seen[j] {
            continue;
        }
        seen[j] = true;
        let free = match owner[j] {
            None => true,
            Some(other) => augment(other, symbols, slots, units, owner, seen),
        };
        if free {
            owner[j] = Some(sym);
            return true;
        }
    }
    false
}

/// Assigns the colored and colorless symbols of `cost` to units, each unit `(kinds, _, n)`
/// paying `n` symbols of one kind. `kinds` gets the kind each unit is spent as, or `None` where
/// it pays only generic mana. Kuhn's augmenting paths over the few symbols a cost has.
fn assign<L: Layout>(
    cost: ManaCost,
    units: &[(u8, Option<L::Slot>, u8)],
    kinds: &mut L::Queue<Option<usize>, 64>,
) -> Result<(), Full> {
    let mut symbols: L::Queue<usize, 64> = Default::default();
    for (k, &n) in cost.symbols.iter().enumerate() {
        for _ in 0..n {
            symbols.push(k)?;
        }
    }
    // A unit of several mana pays as many symbols, all of one kind: split it into slots.
    let mut slots: L::Queue<usize, 64> = Default::default();
    for (u, &(_, _, n)) in units.iter().enumerate() {
        for _ in 0..n {
            slots.push(u)?;
        }
    }
    let mut owner: L::Queue<Option<usize>, 64> = Default::default();
    let mut seen: L::Queue<bool, 64> = Default::default();
    for _ in 0..slots.len() {
        owner.push(None)?;
        seen.push(false)?;
    }
    for sym in 0..symbols.len() {
        seen.as_mut_slice().fill(false);
        let matched = augment(
            sym,
            symbols.as_slice(),
            slots.as_slice(),
            units,
            owner.as_mut_slice(),
            seen.as_mut_slice(),
        );
        debug_assert!(matched, "a plan the solver accepted pays every symbol");
    }
    for u in 0..units.len() {
        let kind = slots
            .iter()
            .zip(owner.iter())
            .find(|&(&slot_unit, o)| slot_unit == u && o.is_some())
            .map(|(_, o)| symbols.as_slice()[o.unwrap()]);
        kinds.push(kind)?;
    }
    Ok(())
}

/// Activates a source's mana ability for one kind of mana: taps it and adds the mana.
///
/// [CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b): "An activated mana
/// ability doesn't go on the stack, so it can't be targeted, countered, or otherwise
/// responded to. Rather, it resolves immediately after it is activated."
pub(crate) fn activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    color: ManaColor,
) -> Result<(), Full> {
    let (card, face) = s.copiable(slot).record(t);
    let (index, ability) = (s.copiable(slot).face(t).abilities)
        .iter()
        .enumerate()
        .find(|(_, a)| {
            (is_simple(a) || is_sacrificed(a))
                && a.produces().includes(color)
                && allowed(s, t, a, s.controller(slot))
        })
        .expect("a mana ability that makes the chosen mana");
    let source = s.object_ref(slot);
    let controller = s.controller(slot);
    // The costs come first ([CR 602.2b](https://yawgatog.com/resources/magic-rules/#R6022b)):
    // activating follows casting's steps, among them
    // [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
    // total cost."
    event::set_tapped(s, t, sink, source, true)?;
    if ability.cost().does(CostAction::Sacrifice) {
        event::move_one(s, t, sink, slot, Zone::Graveyard, event::Reason::Sacrifice)?;
    }
    let activation = || PendingTrigger {
        source: Some(source),
        controller,
        ability: crate::card::AbilityIx::new(index as u8),
        card,
        face,
        ..PendingTrigger::default()
    };
    crate::interpret::mana_ability(s, t, sink, activation, ability, color)
}

/// Manual payment (the `manual_mana` flag): the floating mana and the sources, one per
/// class, whose next unit pays toward `remaining` and leaves the rest payable.
pub(crate) fn manual_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    remaining: ManaCost,
    exclude: Bitboard<L::Slot>,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    let pool = s.player(player).mana().amounts();
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    for kind in 0..KINDS {
        if pool[kind] == 0 || !helps(remaining, kind) {
            continue;
        }
        let mut rest = remaining;
        pay_unit(&mut rest, kind);
        let mut after = pool;
        after[kind] -= 1;
        if sources.units(after, &Table::filled(0)).pays(rest) {
            out.push(Opt::Pool(ManaColor::ALL[kind]))?;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        // A class's members are alike to a payment: its first in candidate order names it.
        let mut members = s.in_candidate_order(c.members);
        let slot = members.next().expect("a class has a member");
        let mut taps: Taps<L> = Table::filled(0);
        taps[i] = 1;
        for kind in (0..KINDS).filter(|&k| c.kinds & (1 << k) != 0) {
            if !helps(remaining, kind) {
                continue;
            }
            let mut rest = remaining;
            pay_unit(&mut rest, kind);
            let mut after = pool;
            after[kind] += c.amount - 1;
            if sources.units(after, &taps).pays(rest) {
                out.push(Opt::Mana {
                    slot,
                    color: ManaColor::ALL[kind],
                })?;
            }
        }
    }
    Ok(out)
}

/// Performs a manual payment option, returning what remains to pay. The source's tap and
/// its mana are kept as one group, a part of the payment a player makes a source at a time:
/// what they pay for (`Cause::entry`) says which payment each is of.
pub(crate) fn manual_pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    mut remaining: ManaCost,
    option: Opt<L>,
) -> Result<ManaCost, Full> {
    let color = match option {
        Opt::Pool(color) => color,
        Opt::Mana { slot, color } => {
            sink.together();
            let activated = activate(s, t, sink, slot, color);
            sink.apart();
            activated?;
            color
        }
        _ => unreachable!("a manual payment option"),
    };
    s.spend_mana(player, color, 1);
    pay_unit(&mut remaining, color.index());
    Ok(remaining)
}

/// The standalone mana activations offered at priority, under manual payment or to the player
/// [`crate::Profile::taps_mana`] names
/// ([CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a)): "A player may activate
/// an activated mana ability whenever they have priority". Otherwise tapping ahead of a
/// payment is dominated by tapping during it (§10), so nothing is offered.
///
/// One per kind of mana each untapped source makes, of each set of interchangeable objects
/// (§7.2), not of each class of sources: a player taps the permanent they mean, an Island
/// rather than the Mox Sapphire that makes the same mana. A list these would fill takes as
/// many as fit: tapping ahead is never needed, as a payment taps for itself.
pub(crate) fn standalone_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, 64>,
) {
    let sources: Bitboard<L::Slot> = controlled(s, player)
        .filter(|&(slot, d)| source(s, t, player, slot, d).is_some())
        .map(|(slot, _)| slot)
        .collect();
    let sources = crate::engine::collapse(s, sources);
    add_activations(s, t, player, out, sources);
}

/// Exact sources the human can activate before paying Waterbend, even when a twin
/// would make the same mana. [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g):
/// "the player then has a chance to activate mana abilities".
pub(crate) fn waterbend_mana_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, 64>,
) {
    let sources = controlled(s, player).map(|(slot, _)| slot).collect();
    add_activations(s, t, player, out, sources);
}

fn add_activations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, 64>,
    sources: Bitboard<L::Slot>,
) {
    // Candidates are listed in the candidate order, which a `Choice` counts in.
    for slot in s.in_candidate_order(sources) {
        let Some((kinds, ..)) = source(s, t, player, slot, s.derived(slot)) else {
            continue;
        };
        for kind in (0..KINDS).filter(|&k| kinds & (1 << k) != 0) {
            let option = Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            };
            if out.push(option).is_err() {
                return;
            }
        }
    }
}
