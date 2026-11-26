//! Tapping and paying: carrying out a plan or a manual choice.
//!
//! Implements: CR 605.3

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, CostAction, FaceIx, Hybrid, INTRINSIC_MANA, ManaColor, ManaCost,
    ObjectAbility, Types,
};
use crate::decision::{OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Table};
use crate::replacement::event::{self, Sink};
use crate::state::{
    AbilityRef, ManaAmounts, PendingTrigger, State, TriggeredAbility, Zone, ZoneKey,
};
use crate::table::{Capability, GameTable, is_instant_mana, is_sacrificed, is_simple};
use beryl_kernel::reference::Permanent;

use super::pool::{helps, pay_unit};
use super::remembered::{self, Paying};
use super::solver::{Plan, fixed_units, lowest_kind, spent_by_class};
use super::sources::{Class, Sources, allowed, alternative_ability};
use super::{KINDS, Taps};

/// Pays `cost` by `plan`: taps its sources for the mana it needs, sacrificing those whose
/// ability says so, and spends it.
///
/// See [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "If the total cost
/// includes a mana payment, the player then has a chance to activate mana abilities ... Mana
/// abilities must be activated before costs are paid."
///
/// See [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. ... Partial payments are not allowed." So the events of the payment, each
/// source's tap and the mana it makes, are kept as one group: the cost is paid or it is not.
///
/// Returns how much of each kind of mana it spent, by [`ManaColor`] index.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    plan: &Plan<L>,
    paying: Paying<L>,
) -> Result<ManaAmounts, Full> {
    sink.together();
    let paid = pay_whole(s, t, sink, player, cost, exclude, plan, paying);
    sink.apart();
    paid
}

/// One unit of a payment: `n` mana, all of one kind among `kinds`, from `from`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Unit<S> {
    kinds: u8,
    n: u8,
    from: From<S>,
}

/// Where a unit of a payment comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum From<S> {
    /// Floating mana.
    #[default]
    Pool,
    /// The mana that remembers at this place in the pool.
    Remembered(u8),
    /// This source's unrestricted mana ability.
    Tap(S),
    /// This source's mana ability that sacrifices it, of a class the plan consumes.
    Consumed(S),
    /// This source's restricted mana ability.
    Restricted(S),
}

/// [`pay`], inside the group it opens.
#[allow(clippy::too_many_arguments)]
fn pay_whole<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    plan: &Plan<L>,
    paying: Paying<L>,
) -> Result<ManaAmounts, Full> {
    let sources = Sources::of(s, t, player, exclude, paying)?;
    if sources.has_memory() {
        return pay_with_memory(s, t, sink, player, cost, plan, paying, &sources);
    }
    pay_units::<L, false>(s, t, sink, player, cost, plan, paying, &sources)
}

/// [`pay_whole`] where the plan may spend mana that remembers or tap a source for restricted
/// mana ([`Sources::has_memory`]). Out of line: only a game whose cards make such mana has one.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn pay_with_memory<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    plan: &Plan<L>,
    paying: Paying<L>,
    sources: &Sources<L>,
) -> Result<ManaAmounts, Full> {
    pay_units::<L, true>(s, t, sink, player, cost, plan, paying, sources)
}

/// Taps and spends what `plan` pays `cost` with. With `MEMORY`, each unit says where it comes
/// from, so that restricted mana is spent as such and gives the spell its rider; without it,
/// the plan spends no such mana and none is looked for.
#[inline]
#[allow(clippy::too_many_arguments)]
fn pay_units<L: Layout, const MEMORY: bool>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    plan: &Plan<L>,
    paying: Paying<L>,
    sources: &Sources<L>,
) -> Result<ManaAmounts, Full> {
    let total = cost.total(0);
    let remembered = if MEMORY {
        plan.remembered.count_ones()
    } else {
        0
    };
    let must: u32 = plan.spend.iter().map(|&n| u32::from(n)).sum::<u32>()
        + remembered
        + sources
            .classes()
            .iter()
            .zip(plan.taps.iter())
            .filter(|(c, _)| c.amount == 1)
            .map(|(_, &n)| u32::from(n))
            .sum::<u32>();
    let fixed = fixed_units::<L, MEMORY>(sources, plan);
    let (spent, _) = spent_by_class(sources, plan, total - must, cost, fixed);
    let mut units: L::Queue<Unit<L::Slot>, 64> = Default::default();
    for (k, &n) in plan.spend.iter().enumerate() {
        for _ in 0..n {
            units.push(Unit {
                kinds: 1 << k,
                n: 1,
                from: From::Pool,
            })?;
        }
    }
    if MEMORY {
        for (i, &(at, kind, _)) in sources.remembered().iter().enumerate() {
            if plan.remembered & (1 << i) != 0 {
                units.push(Unit {
                    kinds: 1 << kind,
                    n: 1,
                    from: From::Remembered(at),
                })?;
            }
        }
    }
    let human = s.profile().taps_mana == Some(player);
    for (i, c) in sources.classes().iter().enumerate() {
        let tapped: L::Queue<L::Slot, 64> = if human {
            human_taps(s, c, plan.taps[i])?
        } else {
            s.mana_payment_sources(c.members, plan.taps[i])?
        };
        let from = |slot| match (MEMORY && c.restricted, c.consumed) {
            (true, _) => From::Restricted(slot),
            (false, true) => From::Consumed(slot),
            (false, false) => From::Tap(slot),
        };
        if c.amount == 1 {
            for &slot in tapped.iter() {
                let (kinds, from) = (c.kinds, from(slot));
                units.push(Unit { kinds, n: 1, from })?;
            }
        } else {
            // Spread the spent units as `spent_by_class` counted them.
            let mut rest = spent[i];
            for (j, &slot) in tapped.iter().enumerate() {
                let later =
                    u32::try_from(tapped.len() - j - 1).expect("at most a class of sources");
                let here = (rest - later).min(u32::from(c.amount));
                let n = u8::try_from(here).unwrap_or(c.amount);
                let (kinds, from) = (c.kinds, from(slot));
                units.push(Unit { kinds, n, from })?;
                rest -= here;
            }
        }
    }
    let mut kinds = Default::default();
    assign::<L, MEMORY>(cost, units.as_slice(), &mut kinds)?;
    if MEMORY {
        // The mana that remembers first, from the last in the pool, so that the places of the
        // rest hold; a restricted source's mana is the last in the pool as it is spent.
        for unit in units.iter().rev() {
            if let From::Remembered(at) = unit.from {
                remembered::spend(s, t, player, usize::from(at), paying)?;
            }
        }
    }
    let mut spent = [0; KINDS];
    for (unit, &kind) in units.iter().zip(kinds.iter()) {
        let kind = kind.unwrap_or_else(|| lowest_kind(unit.kinds));
        let color = ManaColor::ALL[kind];
        spent[kind] += unit.n;
        match unit.from {
            From::Remembered(_) => {}
            From::Restricted(slot) => {
                activate_as::<L, true>(s, t, sink, tapped_source(s, slot), color, false)?;
                let last = s.player(player).remembered_mana().len() - 1;
                remembered::spend(s, t, player, last, paying)?;
            }
            From::Tap(slot) | From::Consumed(slot) => {
                let consume = matches!(unit.from, From::Consumed(_));
                activate_consuming(s, t, sink, tapped_source(s, slot), color, consume)?;
                s.spend_mana(player, color, unit.n);
            }
            From::Pool => s.spend_mana(player, color, unit.n),
        }
    }
    debug_assert_eq!(s.player(player).mana().amounts(), plan.left);
    Ok(spent)
}

/// The source in `slot`, which a payment's plan or option names from the battlefield as it is.
fn tapped_source<L: Layout>(s: &State<L>, slot: L::Slot) -> Permanent<L> {
    s.permanent_at(slot)
        .expect("a mana source on the battlefield")
}

/// The `n` members of class `c` the human's payment taps: its creatures last, free to attack
/// or block as long as they can ([`Outlook`]).
#[cold]
#[inline(never)]
fn human_taps<L: Layout>(s: &State<L>, c: &Class<L>, n: u8) -> Result<L::Queue<L::Slot, 64>, Full> {
    let mut v = s.mana_payment_sources(c.members, c.count())?;
    v.as_mut_slice()
        .sort_by_key(|&m| s.derived(m).types.intersects(Types::CREATURE));
    while v.len() > usize::from(n) {
        v.pop();
    }
    Ok(v)
}

/// One augmenting-path step of [`assign`]: finds a unit slot for symbol `sym`, each symbol
/// the kinds that may pay it.
fn augment<S: SlotId>(
    sym: usize,
    symbols: &[u8],
    slots: &[usize],
    units: &[Unit<S>],
    owner: &mut [Option<usize>],
    seen: &mut [bool],
) -> bool {
    for (j, &u) in slots.iter().enumerate() {
        if units[u].kinds & symbols[sym] == 0 || seen[j] {
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

/// Assigns the colored, colorless and hybrid symbols of `cost` to units, each unit paying
/// its `n` symbols of one kind. `kinds` gets the kind each unit is spent
/// as, or `None` where it pays only generic mana. A unit that pays a hybrid symbol is spent as
/// one of the symbol's colors that it makes (see CR 107.4e). Kuhn's augmenting paths over the
/// few symbols a cost has. `MEMORY` names [`pay_with_memory`]'s copy apart from
/// [`pay_whole`]'s, so that each is inlined into its one caller.
fn assign<L: Layout, const MEMORY: bool>(
    cost: ManaCost,
    units: &[Unit<L::Slot>],
    kinds: &mut L::Queue<Option<usize>, 64>,
) -> Result<(), Full> {
    // Each symbol as the kinds that may pay it.
    let mut symbols: L::Queue<u8, 64> = Default::default();
    for (k, &n) in cost.symbols.iter().enumerate() {
        for _ in 0..n {
            symbols.push(1 << k)?;
        }
    }
    if !cost.hybrid.is_empty() {
        push_hybrid::<L>(cost.hybrid, &mut symbols)?;
    }
    // A unit of several mana pays as many symbols, all of one kind: split it into slots.
    let mut slots: L::Queue<usize, 64> = Default::default();
    for (u, unit) in units.iter().enumerate() {
        for _ in 0..unit.n {
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
    for (u, unit) in units.iter().enumerate() {
        let kind = slots
            .iter()
            .zip(owner.iter())
            .find(|&(&slot_unit, o)| slot_unit == u && o.is_some())
            .map(|(_, o)| lowest_kind(unit.kinds & symbols.as_slice()[o.unwrap()]));
        kinds.push(kind)?;
    }
    Ok(())
}

/// Pushes each hybrid symbol of a cost onto [`assign`]'s symbols, as the kinds of both its
/// halves. Out of line: a cost with none reads only that it has none.
#[cold]
#[inline(never)]
fn push_hybrid<L: Layout>(hybrid: Hybrid, symbols: &mut L::Queue<u8, 64>) -> Result<(), Full> {
    for (pair, n) in hybrid.pairs() {
        for _ in 0..n {
            symbols.push(pair)?;
        }
    }
    Ok(())
}

/// Activates a source's mana ability for one kind of mana: taps it and adds the mana.
///
/// [CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b): "An activated mana
/// ability doesn't go on the stack, so it can't be targeted, countered, or otherwise
/// responded to. Rather, it resolves immediately after it is activated."
///
/// It taps for an unrestricted mana ability that makes the mana if it has one, and for its
/// restricted mana ability otherwise ([`remembered`]), whose mana remembers its restriction.
/// Of its unrestricted mana abilities, it activates one that doesn't sacrifice it if it has
/// one ([`activate_consuming`]).
pub(crate) fn activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    color: ManaColor,
) -> Result<(), Full> {
    activate_consuming(s, t, sink, permanent, color, false)
}

/// [`activate`], for a mana ability that sacrifices the source ([`is_sacrificed`]) if
/// `consume` and it has one that makes the mana, and for one that doesn't otherwise if it has
/// one: the source's controller chooses which of its mana abilities to activate.
pub(crate) fn activate_consuming<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    color: ManaColor,
    consume: bool,
) -> Result<(), Full> {
    activate_as::<L, false>(s, t, sink, permanent, color, consume)
}

/// [`activate_consuming`], for the source's restricted mana ability if `RESTRICTED`.
fn activate_as<L: Layout, const RESTRICTED: bool>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    color: ManaColor,
    consume: bool,
) -> Result<(), Full> {
    let restricted = RESTRICTED;
    let slot = permanent.slot();
    let (card, face) = s.definition(slot).record(t);
    // The card table refuses a card that grants a mana ability (`TableError::Granted`), so
    // the one found is the permanent's own, or, of a merged permanent, that of a card under
    // it (see CR 702.140e), printed on that card's face.
    let find = |preferred: bool| {
        s.abilities(t, slot)
            .filter(move |&(_, a)| !preferred || is_sacrificed(a) == consume)
            .find_map(|(ability, a)| match ability {
                ObjectAbility::Own(i) if !restricted && makes_now(s, t, slot, a, color) => {
                    Some((card, face, i, a))
                }
                ObjectAbility::Merged(..) if !restricted => merged_mana(s, t, slot, ability, color),
                ObjectAbility::Own(_) | ObjectAbility::Granted(_) | ObjectAbility::Merged(..) => {
                    None
                }
            })
    };
    // The ability `consume` names first, then any other: a Treasure has only the one that
    // sacrifices it. A game with no source of both kinds has nothing to prefer.
    let printed = if t.can(Capability::SacrificeAlternative) {
        find(true).or_else(|| find(false))
    } else {
        find(false)
    };
    if printed.is_none()
        && (restricted || s.intrinsic_mana(t, slot) & (1 << color.index()) == 0)
        && let Some((_, ability)) = remembered::makes(s, t, permanent, color)
    {
        // Its only cost is {T} (see CR 602.2b, see CR 601.2h).
        event::set_tapped(s, t, sink, permanent, true)?;
        return remembered::add(s, t, sink, permanent, ability, color);
    }
    // See [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): a land of a basic land
    // type "has the intrinsic ability “{T}: Add [mana symbol],” even if the text box doesn't
    // actually contain that text". It is no ability of the face, so its activation names
    // none: the ability adds its mana with no resolution that would read one.
    let (card, face, index, ability) = printed
        .or_else(|| {
            (s.intrinsic_mana(t, slot) & (1 << color.index()) != 0).then(|| {
                let intrinsic = &INTRINSIC_MANA[color.index()];
                (card, face, AbilityIx::new(u8::MAX), intrinsic)
            })
        })
        .expect("a mana ability that makes the chosen mana");
    let source = permanent.into();
    let controller = s.controller(slot);
    // The costs come first (see [CR 602.2b](https://yawgatog.com/resources/magic-rules/#R6022b)):
    // activating follows casting's steps, among them
    // see [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
    // total cost." The hand is discarded before the source is sacrificed: the player pays a
    // cost's parts in any order, and the order changes nothing here.
    if ability.cost().does(CostAction::Tap) {
        event::set_tapped(s, t, sink, permanent, true)?;
    }
    if ability.cost().discards_hand() {
        let hand = s.zone(ZoneKey::Of(controller, Zone::Hand));
        if !hand.is_empty() {
            let entry = event::Entry::default();
            let reason = event::Reason::Discard;
            event::move_batch(s, t, sink, hand, Zone::Graveyard, reason, entry)?;
        }
    }
    if ability.cost().does(CostAction::Sacrifice) {
        event::move_one(s, t, sink, slot, Zone::Graveyard, event::Reason::Sacrifice)?;
    }
    let activation = || PendingTrigger {
        of: AbilityRef {
            source: Some(source),
            controller: controller.into(),
            ability: TriggeredAbility::own(index),
            card,
            face,
        },
        ..PendingTrigger::default()
    };
    crate::interpret::mana_ability(s, t, sink, activation, ability, color)
}

/// The card, face, index and ability of `ability`, an ability of a card under the merged
/// permanent in `slot` ([`ObjectAbility::Merged`]), if it is a mana ability that makes
/// `color` now. Out of line: only a game with a card that merges has one.
#[cold]
#[inline(never)]
fn merged_mana<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
    color: ManaColor,
) -> Option<(L::Card, FaceIx, AbilityIx, &'static Ability)> {
    let (card, face, i, a) = s.merged_ability(t, slot, ability)?;
    makes_now(s, t, slot, a, color).then_some((card, face, i, a))
}

/// Whether `a`, an unrestricted mana ability of the permanent in `slot`, makes `color` and its
/// controller may activate it now ([`allowed`]).
fn makes_now<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    a: &Ability,
    color: ManaColor,
) -> bool {
    (is_simple(a) || is_sacrificed(a) || is_instant_mana(a))
        && a.produces().includes(color)
        && allowed(s, t, a, s.controller(slot), &s.words_of(slot))
}

/// Manual payment (the `manual_mana` flag): the floating mana and the sources, one per
/// class, whose next unit pays toward `remaining` and leaves the rest payable.
///
/// Floating mana of a kind is one option, whether it is unrestricted or mana that remembers
/// the payment accepts: [`manual_pay`] spends the unrestricted first.
pub(crate) fn manual_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    remaining: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let sources = Sources::of(s, t, player, exclude, paying)?;
    let pool = s.player(player).mana().amounts();
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for kind in 0..KINDS {
        if !helps(remaining, kind) {
            continue;
        }
        let mut rest = remaining;
        pay_unit(&mut rest, kind);
        let mut after = pool;
        let mut spent = 0;
        if pool[kind] > 0 {
            after[kind] -= 1;
        } else if let Some(i) =
            (sources.remembered().iter()).position(|&(_, k, _)| usize::from(k) == kind)
        {
            spent = 1 << i;
        } else {
            continue;
        }
        if sources
            .units_left(after, &Table::filled(0), spent)
            .pays(rest)
        {
            out.push(Opt::Pool(ManaColor::ALL[kind]))?;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        // A class's manual option keeps its existing representative for recorded choices.
        let slot = c.members.min(|slot| slot).expect("a class has a member");
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
            // A source with a restricted class names it by its kind: [`activate`] taps for
            // the unrestricted ability if it makes the kind.
            if (c.restricted && classes_make(&sources, c, kind))
                || !sources.units(after, &taps).pays(rest)
            {
                continue;
            }
            // A source that may also sacrifice itself for mana names that ability: its
            // option for the kind is its other mana ability's.
            if c.is_twin() && c.consumed {
                let ability = alternative_ability(s, t, slot).expect("a twin's ability");
                out.push(Opt::Activate { slot, ability })?;
                break;
            }
            out.push(Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            })?;
        }
    }
    Ok(out)
}

/// Performs a manual payment option, returning what remains to pay and the kind of the mana
/// it paid with. The source's tap and its mana are kept as one group, a part of the payment a
/// player makes a source at a time: what they pay for (`Cause::entry`) says which payment each
/// is of.
pub(crate) fn manual_pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    mut remaining: ManaCost,
    option: Opt<L>,
    paying: Paying<L>,
) -> Result<(ManaCost, ManaColor), Full> {
    let made = s.player(player).remembered_mana().len();
    let color = match option {
        Opt::Pool(color) => color,
        Opt::Mana { slot, color } => {
            sink.together();
            let activated = activate(s, t, sink, tapped_source(s, slot), color);
            sink.apart();
            activated?;
            color
        }
        // A source's mana ability that sacrifices it ([`manual_options`]): its first kind
        // that pays part of what remains.
        Opt::Activate { slot, ability } => {
            let kinds = (s.abilities(t, slot).find(|&(a, _)| a == ability))
                .map(|(_, a)| a.produces().colors)
                .expect("an ability offered");
            let kind = (0..KINDS)
                .find(|&k| kinds & (1 << k) != 0 && helps(remaining, k))
                .unwrap_or_else(|| lowest_kind(kinds));
            let color = ManaColor::ALL[kind];
            sink.together();
            let activated = activate_consuming(s, t, sink, tapped_source(s, slot), color, true);
            sink.apart();
            activated?;
            color
        }
        Opt::Pass
        | Opt::PlayLand(_)
        | Opt::Play
        | Opt::Draw
        | Opt::Keep
        | Opt::Mulligan
        | Opt::FreeMulligan
        | Opt::Trigger(_)
        | Opt::Yes
        | Opt::No
        | Opt::Done
        | Opt::Attack { .. }
        | Opt::Defender(_)
        | Opt::Block(_)
        | Opt::Lethal(_)
        | Opt::Cast { .. }
        | Opt::Number(_)
        | Opt::Plan(_)
        | Opt::Player(_)
        | Opt::Ability(_)
        | Opt::Object(_)
        | Opt::TurnFaceUp(_)
        | Opt::Companion => unreachable!("a manual payment option"),
    };
    let pool = s.player(player).remembered_mana().len();
    if pool > made {
        // A restricted ability made it: the mana that remembers, last in the pool.
        remembered::spend(s, t, player, pool - 1, paying)?;
    } else if s.player(player).mana()[color] > 0 {
        s.spend_mana(player, color, 1);
    } else {
        let accepted = remembered::accepted_entries(s, t, player, paying);
        let &(at, ..) = (accepted.iter())
            .find(|&&(_, kind, _)| usize::from(kind) == color.index())
            .expect("mana of the kind an option names");
        remembered::spend(s, t, player, usize::from(at), paying)?;
    }
    pay_unit(&mut remaining, color.index());
    Ok((remaining, color))
}

/// Whether the class with the same member as the restricted class `c` makes `kind`: then that
/// class's option names the permanent tapped for it.
fn classes_make<L: Layout>(sources: &Sources<L>, c: &Class<L>, kind: usize) -> bool {
    c.twin()
        .is_some_and(|twin| sources.classes()[usize::from(twin)].kinds & (1 << kind) != 0)
}
