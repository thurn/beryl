//! Tapping and paying: carrying out a plan or a manual choice.
//!
//! Implements: CR 605.3

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, ChosenKind, CostAction, FaceIx, INTRINSIC_MANA, ManaColor, ManaCost,
    ObjectAbility, Types,
};
use crate::decision::{Candidates, OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Table};
use crate::replacement::event::{self, Sink};
use crate::state::{AbilityRef, Pending, PendingTrigger, State, TriggeredAbility, Zone, ZoneKey};
use crate::table::{Capability, GameTable, is_instant_mana, is_sacrificed, is_simple};
use beryl_kernel::chosen::ChosenValue;

use super::pool::{helps, pay_unit};
use super::remembered::{self, Paying};
use super::solver::{Plan, lowest_kind, spent_by_class};
use super::sources::{Class, Sources, allowed, controlled, source};
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
) -> Result<(), Full> {
    sink.together();
    let paid = pay_whole(s, t, sink, player, cost, exclude, plan, paying);
    sink.apart();
    paid
}

/// Where a unit of a payment comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum From {
    /// Floating mana.
    #[default]
    Pool,
    /// The mana that remembers at this place in the pool.
    Remembered(u8),
    /// A source's unrestricted mana ability.
    Tap,
    /// A source's restricted mana ability.
    Restricted,
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
) -> Result<(), Full> {
    let sources = Sources::of(s, t, player, exclude, paying)?;
    if sources.has_memory() {
        return pay_with_memory(s, t, sink, player, cost, plan, paying, &sources);
    }
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
            s.mana_payment_sources(c.members, plan.taps[i])?
        };
        if c.amount == 1 {
            for &slot in tapped.iter() {
                units.push((c.kinds, Some(slot), 1))?;
            }
        } else {
            // Spread the spent units as `spent_by_class` counted them.
            let mut rest = spent[i];
            for (j, &slot) in tapped.iter().enumerate() {
                let later =
                    u32::try_from(tapped.len() - j - 1).expect("at most a class of sources");
                let here = (rest - later).min(u32::from(c.amount));
                units.push((c.kinds, Some(slot), u8::try_from(here).unwrap_or(c.amount)))?;
                rest -= here;
            }
        }
    }
    let mut kinds = Default::default();
    assign::<L, false>(cost, units.as_slice(), &mut kinds)?;
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

/// [`pay_whole`] where the plan may spend mana that remembers or tap a source for restricted
/// mana ([`Sources::has_memory`]): each unit says where it comes from, so that restricted mana
/// is spent as such and gives the spell its rider.
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
) -> Result<(), Full> {
    let total = cost.total(0);
    let must: u32 = plan.spend.iter().map(|&n| u32::from(n)).sum::<u32>()
        + plan.remembered.count_ones()
        + sources
            .classes()
            .iter()
            .zip(plan.taps.iter())
            .filter(|(c, _)| c.amount == 1)
            .map(|(_, &n)| u32::from(n))
            .sum::<u32>();
    let spent = spent_by_class(sources, plan, total - must);
    // The units spent: (kinds, source slot or none for floating mana, how many), and where
    // each comes from.
    let mut units: L::Queue<(u8, Option<L::Slot>, u8), 64> = Default::default();
    let mut from: L::Queue<From, 64> = Default::default();
    for (k, &n) in plan.spend.iter().enumerate() {
        for _ in 0..n {
            units.push((1 << k, None, 1))?;
            from.push(From::Pool)?;
        }
    }
    for (i, &(at, kind, _)) in sources.remembered().iter().enumerate() {
        if plan.remembered & (1 << i) != 0 {
            units.push((1 << kind, None, 1))?;
            from.push(From::Remembered(at))?;
        }
    }
    let human = s.profile().taps_mana == Some(player);
    for (i, c) in sources.classes().iter().enumerate() {
        let tapped: L::Queue<L::Slot, 64> = if human {
            human_taps(s, c, plan.taps[i])?
        } else {
            s.mana_payment_sources(c.members, plan.taps[i])?
        };
        let how = if c.restricted {
            From::Restricted
        } else {
            From::Tap
        };
        if c.amount == 1 {
            for &slot in tapped.iter() {
                units.push((c.kinds, Some(slot), 1))?;
                from.push(how)?;
            }
        } else {
            // Spread the spent units as `spent_by_class` counted them.
            let mut rest = spent[i];
            for (j, &slot) in tapped.iter().enumerate() {
                let later =
                    u32::try_from(tapped.len() - j - 1).expect("at most a class of sources");
                let here = (rest - later).min(u32::from(c.amount));
                units.push((c.kinds, Some(slot), u8::try_from(here).unwrap_or(c.amount)))?;
                from.push(how)?;
                rest -= here;
            }
        }
    }
    let mut kinds = Default::default();
    assign::<L, true>(cost, units.as_slice(), &mut kinds)?;
    // The mana that remembers first, from the last in the pool, so that the places of the
    // rest hold; a restricted source's mana is the last in the pool as it is spent.
    for &how in from.iter().rev() {
        if let From::Remembered(at) = how {
            remembered::spend(s, t, player, usize::from(at), paying)?;
        }
    }
    for ((&(unit_kinds, source, n), &kind), &how) in units.iter().zip(kinds.iter()).zip(from.iter())
    {
        let color = ManaColor::ALL[kind.unwrap_or_else(|| lowest_kind(unit_kinds))];
        match (how, source) {
            (From::Remembered(_), _) => {}
            (From::Restricted, Some(slot)) => {
                activate_as::<L, true>(s, t, sink, slot, color)?;
                let last = s.player(player).remembered_mana().len() - 1;
                remembered::spend(s, t, player, last, paying)?;
            }
            (_, Some(slot)) => {
                activate_as::<L, false>(s, t, sink, slot, color)?;
                s.spend_mana(player, color, n);
            }
            (_, None) => s.spend_mana(player, color, n),
        }
    }
    debug_assert_eq!(s.player(player).mana().amounts(), plan.left);
    Ok(())
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
/// `MEMORY` names [`pay_with_memory`]'s copy apart from [`pay_whole`]'s, so that each is
/// inlined into its one caller.
fn assign<L: Layout, const MEMORY: bool>(
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
///
/// It taps for an unrestricted mana ability that makes the mana if it has one, and for its
/// restricted mana ability otherwise ([`remembered`]), whose mana remembers its restriction.
pub(crate) fn activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    color: ManaColor,
) -> Result<(), Full> {
    activate_as::<L, false>(s, t, sink, slot, color)
}

/// [`activate`], for the source's restricted mana ability if `RESTRICTED`.
fn activate_as<L: Layout, const RESTRICTED: bool>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    color: ManaColor,
) -> Result<(), Full> {
    let restricted = RESTRICTED;
    let (card, face) = s.definition(slot).record(t);
    // The card table refuses a card that grants a mana ability (`TableError::Granted`), so
    // the one found is the permanent's own, or, of a merged permanent, that of a card under
    // it (see CR 702.140e), printed on that card's face.
    let printed = s.abilities(t, slot).find_map(|(ability, a)| match ability {
        ObjectAbility::Own(i)
            if !restricted
                && (is_simple(a) || is_sacrificed(a) || is_instant_mana(a))
                && a.produces().includes(color)
                && allowed(s, t, a, s.controller(slot), &s.words_of(slot)) =>
        {
            Some((card, face, i, a))
        }
        ObjectAbility::Merged(..) if !restricted => merged_mana(s, t, slot, ability, color),
        ObjectAbility::Own(_) | ObjectAbility::Granted(_) | ObjectAbility::Merged(..) => None,
    });
    if printed.is_none()
        && (restricted || s.intrinsic_mana(t, slot) & (1 << color.index()) == 0)
        && let Some((_, ability)) = remembered::makes(s, t, slot, color)
    {
        let source = s.object_ref(slot);
        // Its only cost is {T} (see CR 602.2b, see CR 601.2h).
        event::set_tapped(s, t, sink, source, true)?;
        return remembered::add(s, t, sink, source, ability, color);
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
    let source = s.object_ref(slot);
    let controller = s.controller(slot);
    // The costs come first (see [CR 602.2b](https://yawgatog.com/resources/magic-rules/#R6022b)):
    // activating follows casting's steps, among them
    // see [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
    // total cost." The hand is discarded before the source is sacrificed: the player pays a
    // cost's parts in any order, and the order changes nothing here.
    if ability.cost().does(CostAction::Tap) {
        event::set_tapped(s, t, sink, source, true)?;
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
            controller,
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
    let usable = (is_simple(a) || is_sacrificed(a) || is_instant_mana(a))
        && a.produces().includes(color)
        && allowed(s, t, a, s.controller(slot), &s.words_of(slot));
    usable.then_some((card, face, i, a))
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
            out.push(Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            })?;
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
    paying: Paying<L>,
) -> Result<ManaCost, Full> {
    let made = s.player(player).remembered_mana().len();
    let color = match option {
        Opt::Pool(color) => color,
        Opt::Mana { slot, color } => {
            sink.together();
            let activated = activate(s, t, sink, slot, color);
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
        | Opt::Trigger(_)
        | Opt::Yes
        | Opt::No
        | Opt::Done
        | Opt::Attack(_)
        | Opt::Defender(_)
        | Opt::Block(_)
        | Opt::Lethal(_)
        | Opt::Cast { .. }
        | Opt::Activate { .. }
        | Opt::Number(_)
        | Opt::Plan(_)
        | Opt::Player(_)
        | Opt::Ability(_)
        | Opt::Object(_)
        | Opt::TurnFaceUp(_) => unreachable!("a manual payment option"),
    };
    let pool = s.player(player).remembered_mana().len();
    if pool > made {
        // A restricted ability made it: the mana that remembers, last in the pool.
        remembered::spend(s, t, player, pool - 1, paying)?;
    } else if s.player(player).mana()[color] > 0 {
        s.spend_mana(player, color, 1);
    } else {
        let accepted = remembered::accepted_entries(s, t, player, paying);
        let &(at, _) = (accepted.iter())
            .find(|&&(_, kind)| usize::from(kind) == color.index())
            .expect("mana of the kind an option names");
        remembered::spend(s, t, player, usize::from(at), paying)?;
    }
    pay_unit(&mut remaining, color.index());
    Ok(remaining)
}

/// Whether the class with the same member as the restricted class `c` makes `kind`: then that
/// class's option names the permanent tapped for it.
fn classes_make<L: Layout>(sources: &Sources<L>, c: &Class<L>, kind: usize) -> bool {
    c.twin()
        .is_some_and(|twin| sources.classes()[usize::from(twin)].kinds & (1 << kind) != 0)
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
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) {
    let sources: Bitboard<L::Slot> = controlled(s, player)
        .filter(|&(slot, d)| source(s, t, player, slot, d).is_some())
        .map(|(slot, _)| slot)
        .collect();
    let sources = crate::engine::collapse(s, sources);
    add_activations(s, t, player, out, sources);
}

/// The mana abilities `player` may activate only while they have priority
/// ([`is_instant_mana`]), one option for each, for each set of interchangeable permanents
/// (§7.2). The mana solver never activates such an ability, so it is offered here whatever
/// the profile: "Activate only as an instant" lets it be activated at priority alone
/// (see [CR 117.1a]): "A player may cast an instant spell any time they have priority." A game
/// whose cards have none pays one test of the table.
///
/// [CR 117.1a]: https://yawgatog.com/resources/magic-rules/#R1171a
pub(crate) fn instant_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    if !t.can(Capability::InstantMana) {
        return Ok(());
    }
    list_instant_options(s, t, player, out)
}

/// [`instant_options`] in a game whose cards have such an ability. Out of line.
#[inline(never)]
fn list_instant_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let instant = |slot: L::Slot| {
        let words = s.words_of(slot);
        s.abilities(t, slot).filter(move |&(ability, a)| {
            matches!(ability, ObjectAbility::Own(_) | ObjectAbility::Merged(..))
                && is_instant_mana(a)
                && !(a.cost().does(CostAction::Tap) && s.object(slot).is_tapped())
                && allowed(s, t, a, player, &words)
        })
    };
    let objects: Bitboard<L::Slot> = (s.battlefield().iter().copied())
        .filter(|&slot| s.controller(slot) == player && instant(slot).next().is_some())
        .collect();
    Candidates::<L>::extend_options(out, crate::engine::collapse(s, objects), |out, slot| {
        for (ability, _) in instant(slot) {
            out.push(Opt::Activate { slot, ability })?;
        }
        Ok(())
    })
}

/// Activates the mana ability `ability` of the permanent in `slot`, one activated only as an
/// instant ([`is_instant_mana`]): at once if it makes one kind of mana, or once its
/// controller has chosen the color of "any one color" ([`Pending::ManaColor`]). That color
/// is chosen as the ability resolves, after its costs are paid, but here it is asked before
/// anything is paid: what paying shows changes none of the colors the player may choose.
pub(crate) fn begin_instant<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Result<(), Full> {
    let kinds = s
        .abilities(t, slot)
        .find(|&(a, _)| a == ability)
        .map(|(_, a)| a.produces().colors)
        .expect("an ability offered");
    // See CR 117.3c: "If a player has priority when they ... activate an ability ..., that player
    // receives priority afterward."
    s.reset_passes();
    if kinds.is_power_of_two() {
        let color = ManaColor::ALL[lowest_kind(kinds)];
        return activate(s, t, sink, slot, color);
    }
    s.ask(Pending::ManaColor { slot, ability });
    Ok(())
}

/// Activates the mana ability [`Pending::ManaColor`] asked about, of the permanent in `slot`,
/// for the color the answer `opt` names.
pub(crate) fn activate_chosen<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    opt: Option<Opt<L>>,
) -> Result<(), Full> {
    let ChosenValue::Color(colors) = crate::chosen::value(ChosenKind::Color, opt) else {
        unreachable!("a color among a color's candidates");
    };
    activate(s, t, sink, slot, ManaColor::ALL[lowest_kind(colors.0)])
}

/// Whether `ability` of the permanent in `slot` is a mana ability activated only as an
/// instant, which [`begin_instant`] activates rather than the casting steps.
pub(crate) fn is_instant<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    t.can(Capability::InstantMana)
        && s.abilities(t, slot)
            .any(|(a, printed)| a == ability && is_instant_mana(printed))
}

/// Exact sources the human can activate before paying Waterbend, even when a twin
/// would make the same mana. see [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g):
/// "the player then has a chance to activate mana abilities".
pub(crate) fn waterbend_mana_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) {
    let sources = controlled(s, player).map(|(slot, _)| slot).collect();
    add_activations(s, t, player, out, sources);
}

fn add_activations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
    sources: Bitboard<L::Slot>,
) {
    let restricted = remembered::restricted_sources(s, t, player).unwrap_or_default();
    // Tapping ahead is optional, so a full list keeps the options it already holds.
    let _full = Candidates::<L>::extend_options(out, sources, |out, slot| {
        // Its restricted mana too: the mana remembers what it may be spent on.
        let more = (restricted.iter().find(|r| r.slot == slot)).map_or(0, |r| r.kinds);
        let Some(kinds) = source(s, t, player, slot, s.derived(slot))
            .map(|(kinds, ..)| kinds | more)
            .or((more != 0).then_some(more))
        else {
            return Ok(());
        };
        for kind in (0..KINDS).filter(|&k| kinds & (1 << k) != 0) {
            let option = Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            };
            out.push(option)?;
        }
        Ok(())
    });
}
