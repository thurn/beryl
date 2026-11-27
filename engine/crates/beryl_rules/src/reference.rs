//! Slow reference implementations of the engine's fast paths, which fuzzing and the scenario
//! runner check the state against at every decision (architecture §17.4). Each answers the
//! question its fast path answers from first principles, sharing as little of its code as it
//! can: a disagreement is an engine bug, never a rules outcome.
//!
//! - The layer results and the caches found with them, against a recompute from scratch.
//! - The abilities each permanent was granted, against the static abilities and effects that
//!   grant them, without the layers' table of grants ([`grants`]).
//! - Payability: the prefilter, the cached funds and the solver's plans, against a matching
//!   of each mana symbol to a unit of mana from the pool or a mana ability read off the cards.
//! - In a game whose cards have convoke or improvise, whether each such spell in a hand can be
//!   paid, against every set of the creatures or artifacts it could tap ([`objects`]).
//! - In a game whose cards have priced mana abilities or products of several terms, the cached
//!   funds and the solver's plans against every sequence of mana abilities ([`sequences`]).
//!
//! Implements: none. Checks optimized implementations against reference calculations.

use crate::bitboard::Bitboard;
use crate::card::{AbilityKind, CostAction, CostPart, Keywords, ManaCost, Types};
use crate::continuous::layers;
use crate::filter::Evaluate;
use crate::fixed::Fixed;
use crate::ids::{Seat, SlotId};
use crate::invariants::{At, Violation};
use crate::layout::{Layout, Queue, Table};
use crate::mana::{self, Funds, Paying};
use crate::replacement::event::Sink;
use crate::state::State;
use crate::table::{Capability, GameTable};
use beryl_kernel::chosen::ChosenValue;
use beryl_kernel::mana_memory::RememberedMana;

mod grants;
mod objects;
mod sequences;

#[cfg(test)]
pub(crate) use objects::reference_pays_with_objects;
pub(crate) use sequences::reference_sequences;

fn check(ok: bool, what: &'static str, at: Option<At>) -> Result<(), Violation> {
    if ok {
        Ok(())
    } else {
        Err(Violation { what, at })
    }
}

/// Costs every check tries besides those of the cards in play: generic amounts, each kind of
/// symbol alone, the pairs a two-color deck's spells ask for, and hybrid symbols alone, beside
/// a colored symbol, and in four pairs that share a color.
const PROBES: &[ManaCost] = &[
    crate::card::mana("{W/U}{W/U}"),
    crate::card::mana("{1}{W/B}{U}"),
    crate::card::mana("{G/W}{G/U}{B/G}{R/G}"),
    ManaCost::new(1, [0; 6]),
    ManaCost::new(2, [0; 6]),
    ManaCost::new(4, [0; 6]),
    ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
    ManaCost::new(0, [0, 1, 0, 0, 0, 0]),
    ManaCost::new(0, [0, 0, 1, 0, 0, 0]),
    ManaCost::new(0, [0, 0, 0, 1, 0, 0]),
    ManaCost::new(0, [0, 0, 0, 0, 1, 0]),
    ManaCost::new(0, [0, 0, 0, 0, 0, 1]),
    ManaCost::new(0, [1, 1, 0, 0, 0, 0]),
    ManaCost::new(1, [2, 0, 0, 0, 0, 0]),
    ManaCost::new(1, [0, 2, 0, 0, 0, 0]),
    ManaCost::new(2, [1, 1, 0, 0, 0, 0]),
];

/// Checks the engine's fast paths against their reference implementations. It holds
/// wherever [`State::check_invariants`] does, and checks nothing once the game is over.
pub fn check_references<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    if s.outcome().is_some() || s.is_derived_stale() {
        return Ok(());
    }
    check_layers(s, t)?;
    grants::check_grants(s, t)?;
    // A check of each player alone: the order decides only which violation is reported.
    for p in s.players().in_seat_order() {
        check_payments(s, t, p)?;
    }
    Ok(())
}

/// The layer results, the listeners, the players' rules and the knowledge the layers give,
/// against a recompute from base
/// (see [CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131)):
/// "The values of an object's characteristics are determined by starting with the actual
/// object ... Then all applicable continuous effects are applied in a series of layers".
/// A chokepoint that changes a layer input without marking the results dirty leaves them
/// stale, which this sees and the dirty flag cannot.
fn check_layers<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    let mut fresh = s.clone();
    fresh.forget_derived();
    // A recompute with no room for an effect stalls the game, which is then replayed in a
    // larger layout: there is no result to hold it to.
    if layers::refresh(&mut fresh, t, &mut Sink::default()).is_err() {
        return Ok(());
    }
    for ((slot, was), (_, now)) in s
        .permanents_with_derived()
        .zip(fresh.permanents_with_derived())
    {
        check(
            now == was,
            "layer results differ from a fresh recompute",
            Some(At::Slot(slot.index())),
        )?;
    }
    check(
        fresh.grant_rows() == s.grant_rows(),
        "the table of grants differs from a fresh recompute",
        None,
    )?;
    check(
        fresh.listening() == s.listening(),
        "the trigger listeners differ from a fresh recompute",
        None,
    )?;
    check(
        fresh.replacing() == s.replacing(),
        "the replacement kinds differ from a fresh recompute",
        None,
    )?;
    // The players in any order: each is checked alone.
    for player in s.players().in_seat_order() {
        check(
            fresh.rules_for(player) == s.rules_for(player),
            "the rules static abilities change differ from a fresh recompute",
            Some(At::Player(player)),
        )?;
    }
    check(
        fresh.knowledge() == s.knowledge(),
        "knowledge differs from a fresh layer recompute",
        None,
    )
}

/// Whether `player` can pay each cost of the cards they hold and control, and some
/// probes, as the prefilter, the cached funds and the solver say, against
/// [`reference_pays`].
fn check_payments<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Result<(), Violation> {
    let Some(units) = reference_units(s, t, player, Paying::NOTHING, Bitboard::EMPTY) else {
        return Ok(());
    };
    let at = Some(At::Player(player));
    let hand = s.hand_by_definition(player);
    let hand = hand
        .iter()
        .filter_map(|&slot| s.card_def(t, slot))
        .flat_map(|def| def.faces.iter().map(|f| f.cost));
    let abilities = s
        .battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == player)
        .flat_map(|&slot| {
            s.abilities(t, slot)
                .filter(|&(_, a)| a.kind() == AbilityKind::Activated)
                .map(|(_, a)| a.cost().mana)
        });
    let Ok(funds) = Funds::of(s, t, player) else {
        return Ok(());
    };
    let rich = t.can_either(Capability::PricedMana, Capability::ManaChanges);
    for cost in PROBES.iter().copied().chain(hand).chain(abilities) {
        // X is announced before payment, as 0 here.
        let cost = ManaCost { x: 0, ..cost };
        if rich {
            check_sequences(s, t, player, &funds, cost)?;
            continue;
        }
        let expected = reference_pays(units.as_slice(), cost);
        let none = Bitboard::EMPTY;
        let Ok(prefilter) = mana::payable(s, t, player, cost, none, mana::Paying::NOTHING) else {
            continue;
        };
        check(
            prefilter == expected,
            "the payability prefilter disagrees with the reference",
            at,
        )?;
        check(
            funds.pays(s, t, cost, none) == expected,
            "the cached funds disagree with the reference",
            at,
        )?;
        for greedy in [false, true] {
            let plans = mana::plans(
                s,
                t,
                &mut Sink::default(),
                player,
                cost,
                none,
                greedy,
                mana::Paying::NOTHING,
            );
            if let Ok(plans) = plans {
                check(
                    plans.is_empty() != expected,
                    "the payment solver disagrees with the reference",
                    at,
                )?;
            }
        }
    }
    check_spell_payments(s, t, player, &funds)?;
    check_ability_payments(s, t, player, &funds, units.as_slice())?;
    if t.can(Capability::PaysWithObjects) {
        objects::check_objects(s, t, player)?;
    }
    Ok(())
}

/// Whether `player` can pay `cost` for no spell, as the cached funds and the solver's plans
/// say, against every sequence of their mana abilities ([`reference_sequences`]): in a game
/// whose cards have priced mana abilities or products of several terms, which only a payment
/// that activates them in order can use.
fn check_sequences<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    funds: &Funds<L>,
    cost: ManaCost,
) -> Result<(), Violation> {
    let Some(expected) = reference_sequences(s, t, player, cost) else {
        return Ok(());
    };
    let at = Some(At::Player(player));
    let none = Bitboard::EMPTY;
    if let Ok(prefilter) = mana::payable(s, t, player, cost, none, Paying::NOTHING) {
        check(
            prefilter == expected,
            "the payability prefilter disagrees with the sequence reference",
            at,
        )?;
    }
    check(
        funds.pays_for(s, t, cost, none, Paying::NOTHING) == expected,
        "the cached funds disagree with the sequence reference",
        at,
    )?;
    for greedy in [false, true] {
        let plans = mana::plans(
            s,
            t,
            &mut Sink::default(),
            player,
            cost,
            none,
            greedy,
            Paying::NOTHING,
        );
        if let Ok(plans) = plans {
            check(
                plans.is_empty() != expected,
                "the payment solver disagrees with the sequence reference",
                at,
            )?;
        }
    }
    Ok(())
}

/// Whether `player` can pay `cost` (X announced) for no spell from their pool and one tap of
/// each of their sources, as [`reference_pays`] matches it: `None` where [`reference_units`]
/// cannot say the units.
#[cfg(test)]
pub(crate) fn reference_payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
) -> Option<bool> {
    let units = reference_units(s, t, player, Paying::NOTHING, Bitboard::EMPTY)?;
    Some(reference_pays(units.as_slice(), cost))
}

/// Whether `player` can pay the mana cost of each card in their hand for casting it, as the
/// prefilter, the cached funds and the solver say, against [`reference_pays`] with the
/// restricted mana that spell accepts among the units (see [CR 106.6]): "Some spells or
/// abilities that produce mana restrict how that mana can be spent". Only in a game whose
/// cards make restricted mana: in any other, a payment for a spell is one for nothing, which
/// [`check_payments`] checks.
///
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face a reference cast pays for"
)]
fn check_spell_payments<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    funds: &Funds<L>,
) -> Result<(), Violation> {
    if !t.can(Capability::RemembersMana) {
        return Ok(());
    }
    let at = Some(At::Player(player));
    let none = Bitboard::EMPTY;
    for &slot in s.hand_by_definition(player).iter() {
        let Some(def) = s.card_def(t, slot) else {
            continue;
        };
        for face in def.face_ixs() {
            let paying = Paying::casting(s, t, slot, face);
            let Some(units) = reference_units(s, t, player, paying, Bitboard::EMPTY) else {
                continue;
            };
            // X is announced before payment, as 0 here.
            let cost = ManaCost {
                x: 0,
                ..def.face(face).cost
            };
            let expected = reference_pays(units.as_slice(), cost);
            // More sources of restricted mana than a small payment holds: the game goes on in
            // a larger layout, and the funds answer yes until it does.
            let Ok(prefilter) = mana::payable(s, t, player, cost, none, paying) else {
                continue;
            };
            check(
                prefilter == expected,
                "the payability prefilter disagrees with the reference for a spell",
                at,
            )?;
            check(
                funds.pays_for(s, t, cost, none, paying) == expected,
                "the cached funds disagree with the reference for a spell",
                at,
            )?;
            for greedy in [false, true] {
                let plans = mana::plans(
                    s,
                    t,
                    &mut Sink::default(),
                    player,
                    cost,
                    none,
                    greedy,
                    paying,
                );
                if let Ok(plans) = plans {
                    check(
                        plans.is_empty() != expected,
                        "the payment solver disagrees with the reference for a spell",
                        at,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Whether `player` can pay the mana cost of each activated ability on a permanent they control,
/// as the prefilter, the cached funds and the solver say, against [`reference_pays`] when a
/// spend-as-any-color rule covers the permanent. Only in a game whose cards have such a rule: in
/// any other, every activation's cost is unchanged and [`check_payments`] checks it.
fn check_ability_payments<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    funds: &Funds<L>,
    units: &[u8],
) -> Result<(), Violation> {
    if t.rules_of(crate::state::RuleKind::SpendAsAnyColor)
        .is_empty()
    {
        return Ok(());
    }
    let at = Some(At::Player(player));
    let none = Bitboard::EMPTY;
    for &slot in s.battlefield() {
        if s.controller(slot) != player {
            continue;
        }
        let ref_any = reference_spends_as_any_color(s, t, player, slot);
        let eng_any = crate::continuous::rule_change::spends_as_any_color(s, t, player, slot);
        check(
            eng_any == ref_any,
            "spend-as-any-color disagrees with the reference",
            at,
        )?;
        for (_, a) in s.abilities(t, slot) {
            if a.kind() != AbilityKind::Activated {
                continue;
            }
            let base = ManaCost {
                x: 0,
                ..a.cost().mana
            };
            let cost = if ref_any { base.as_any_color() } else { base };
            let expected = reference_pays(units, cost);
            let Ok(prefilter) = mana::payable(s, t, player, cost, none, Paying::NOTHING) else {
                continue;
            };
            check(
                prefilter == expected,
                "the payability prefilter disagrees with the reference for an ability",
                at,
            )?;
            check(
                funds.pays(s, t, cost, none) == expected,
                "the cached funds disagree with the reference for an ability",
                at,
            )?;
            for greedy in [false, true] {
                let plans = mana::plans(
                    s,
                    t,
                    &mut Sink::default(),
                    player,
                    cost,
                    none,
                    greedy,
                    Paying::NOTHING,
                );
                if let Ok(plans) = plans {
                    check(
                        plans.is_empty() != expected,
                        "the payment solver disagrees with the reference for an ability",
                        at,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Whether any permanent on the battlefield has a static ability letting `player` spend mana
/// as though it were mana of any color to activate abilities of `source`, read directly from
/// the permanents' abilities without [`State::rules_for`].
fn reference_spends_as_any_color<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    source: L::Slot,
) -> bool {
    use crate::filter::Evaluate;
    s.battlefield().iter().any(|&maker| {
        let controller = s.controller(maker);
        let words = s.words_of(maker);
        s.abilities(t, maker).any(|(_, a)| {
            if a.kind() != AbilityKind::Static {
                return false;
            }
            let Some(rule) = a.rule() else {
                return false;
            };
            let beryl_card::Governed::SpendAsAnyColor(filter) = rule.what else {
                return false;
            };
            rule.when == beryl_card::When::AsUsual
                && crate::filter::is_for(&rule.players, player, controller)
                && filter.matches(s, t, source, controller, &words)
        })
    })
}

/// The most units of mana the reference matches: the debug builds of `next` and `apply` check
/// it (§17.4), so it allocates nothing, as the hot path does not.
const UNITS: usize = 64;

/// Units of mana, each as the set of kinds it may be.
type Units = Fixed<u8, UNITS>;

/// The units of mana `player` could spend now on what `paying` is for, each as the set of
/// kinds it may be: their floating mana, and one tap of each untapped permanent they control
/// with a mana ability whose only costs are {T} and perhaps sacrificing it, read from its face
/// up, but those in `exclude`. Mana whose spending is restricted counts only where its restriction accepts the spell
/// being paid for (see [CR 106.6]), so never for a cost of no spell. `None` where a
/// permanent's mana abilities make different amounts, which units cannot say, or where there
/// are more than [`UNITS`].
///
/// See [CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a): "An activated ability is
/// a mana ability if it meets all of the following criteria: it doesn't require a target ...,
/// it could add mana to a player's mana pool when it resolves, it's not a loyalty ability ...,
/// and its cost and effect don't move any card to or from a library."
/// See [CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026): "A creature's activated
/// ability with the tap symbol or the untap symbol in its activation cost can't be activated
/// unless the creature has been under its controller's control continuously since their most
/// recent turn began."
///
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
fn reference_units<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    paying: Paying<L>,
    exclude: Bitboard<L::Slot>,
) -> Option<Units> {
    let mut units = Units::new();
    for (k, &n) in s.player(player).mana().amounts().iter().enumerate() {
        for _ in 0..n {
            units.push(1u8 << k).ok()?;
        }
    }
    for mana in s.player(player).remembered_mana() {
        if paying.reads_restrictions(t) && paying.accepts(t, mana.restriction, mana.chosen_type) {
            units
                .push(crate::card::ManaSet::one(mana.color).colors)
                .ok()?;
        }
    }
    for &slot in s.battlefield() {
        let d = s.derived(slot);
        let o = s.object(slot);
        if d.controller != player || o.is_tapped() || exclude.contains(slot) {
            continue;
        }
        if d.types.intersects(Types::CREATURE)
            && o.is_summoning_sick()
            && !d.keywords.contains(Keywords::HASTE)
        {
            continue;
        }
        // The intrinsic mana abilities of its basic land types make one mana each (see CR 305.6).
        let mut kinds = s.intrinsic_mana(t, slot);
        let mut amount = (kinds != 0).then_some(1);
        // A permanent that lost the ability has none to tap (see CR 613.1f).
        for (_, a) in s.abilities(t, slot) {
            // {T}, or {T} and sacrificing it, and nothing else.
            let tap_only = a.cost().mana.is_zero()
                && a.cost().does(CostAction::Tap)
                && a.cost().parts().iter().all(|part| {
                    matches!(
                        part,
                        CostPart::This(CostAction::Tap | CostAction::Sacrifice)
                    )
                });
            // Restricted mana pays only for what its restriction accepts (see CR 106.6).
            if a.kind() != AbilityKind::Mana
                || !tap_only
                || !spendable(s, t, paying, slot, a)
                || !controls(s, t, player, a, &s.words_of(slot))
            {
                continue;
            }
            if let Some(n) = amount.filter(|&n| n != a.produces().amount) {
                // Mana abilities that make different amounts of the same kinds, as "{T}: Add
                // {U}." and "{T}, Sacrifice this land: Add {U}{U}.": the most one of them
                // makes, as its controller chooses which to activate (see CR 605.3a).
                if kinds != a.produces().colors {
                    return None;
                }
                amount = Some(n.max(a.produces().amount));
                continue;
            }
            amount = Some(a.produces().amount);
            kinds |= a.produces().colors;
        }
        if let Some(n) = amount.filter(|_| kinds != 0) {
            for _ in 0..n {
                units.push(kinds).ok()?;
            }
        }
    }
    Some(units)
}

/// Whether the mana that the mana ability `a` of the permanent in `slot` makes may be spent on
/// what `paying` is for: any mana but restricted mana, and restricted mana that its
/// restriction accepts, as read of the creature type the permanent chose
/// (see [CR 106.6](https://yawgatog.com/resources/magic-rules/#R1066)).
fn spendable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    paying: Paying<L>,
    slot: L::Slot,
    a: &crate::card::Ability,
) -> bool {
    let Some(restriction) = a.spend() else {
        return true;
    };
    let chosen_type = match s.chosen(slot) {
        Some(ChosenValue::CreatureType(Some(n))) => n,
        _ => RememberedMana::<L>::NO_TYPE,
    };
    paying.reads_restrictions(t)
        && t.restriction_id(restriction)
            .is_some_and(|id| paying.accepts(t, id, chosen_type))
}

/// Whether `player` meets a mana ability's "Activate only if you control" condition
/// (see [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)).
fn controls<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    a: &crate::card::Ability,
    words: &crate::card::Words,
) -> bool {
    a.only_if_you_control().is_none_or(|f| {
        s.battlefield().iter().any(|&slot: &L::Slot| {
            s.controller(slot) == player && f.matches(s, t, slot, player, words)
        })
    })
}

/// Whether `units` pay `cost`: each colored or colorless symbol matched to a distinct unit
/// that may be its kind (augmenting paths), and enough units left for the generic part.
///
/// See [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the total
/// cost. ... Partial payments are not allowed."
fn reference_pays(units: &[u8], cost: ManaCost) -> bool {
    let count = usize::try_from(cost.symbol_count()).expect("a cost's symbols fit a usize");
    if units.len() < count + usize::from(cost.generic) {
        return false;
    }
    // At most `units.len()` symbols, so they fit. A hybrid symbol is the kinds of both its
    // halves (see CR 107.4e).
    let mut symbols = Units::new();
    for (k, &n) in cost.symbols.iter().enumerate() {
        for _ in 0..n {
            symbols.push_bounded(1u8 << k);
        }
    }
    for (pair, n) in cost.hybrid.pairs() {
        for _ in 0..n {
            symbols.push_bounded(pair);
        }
    }
    let mut paying = [None; UNITS];
    for symbol in 0..symbols.len() {
        let mut seen = [false; UNITS];
        if !augment(
            symbol,
            symbols.as_slice(),
            units,
            &mut paying[..units.len()],
            &mut seen[..units.len()],
        ) {
            return false;
        }
    }
    true
}

/// Finds a unit for `symbol`, moving the symbols already matched to other units as needed.
fn augment(
    symbol: usize,
    symbols: &[u8],
    units: &[u8],
    paying: &mut [Option<usize>],
    seen: &mut [bool],
) -> bool {
    for u in 0..units.len() {
        if units[u] & symbols[symbol] == 0 || seen[u] {
            continue;
        }
        seen[u] = true;
        if paying[u].is_none_or(|other| augment(other, symbols, units, paying, seen)) {
            paying[u] = Some(symbol);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::reference_pays;
    use crate::card::{ManaCost, mana};

    const W: u8 = 1;
    const U: u8 = 2;
    const B: u8 = 4;
    const C: u8 = 1 << 5;

    #[test]
    fn the_reference_matches_symbols_to_units() {
        let wu = W | U;
        // A Tundra pays {W} or {U}, not both.
        assert!(reference_pays(&[wu], ManaCost::new(0, [1, 0, 0, 0, 0, 0])));
        assert!(!reference_pays(&[wu], ManaCost::new(0, [1, 1, 0, 0, 0, 0])));
        // Tundra and Island pay {W}{U} only with the Tundra on {W}.
        assert!(reference_pays(
            &[wu, U],
            ManaCost::new(0, [1, 1, 0, 0, 0, 0])
        ));
        // Colorless mana pays {C} and generic, never a colored symbol.
        assert!(reference_pays(
            &[C, C],
            ManaCost::new(1, [0, 0, 0, 0, 0, 1])
        ));
        assert!(!reference_pays(
            &[C, C],
            ManaCost::new(0, [0, 1, 0, 0, 0, 0])
        ));
        assert!(!reference_pays(&[U], ManaCost::new(0, [0, 0, 0, 0, 0, 1])));
        assert!(!reference_pays(
            &[U, U],
            ManaCost::new(1, [0, 2, 0, 0, 0, 0])
        ));
    }

    #[test]
    fn the_reference_pays_a_hybrid_symbol_with_either_half() {
        let wb = mana("{W/B}{U}");
        assert!(reference_pays(&[W, U], wb));
        assert!(reference_pays(&[B, U], wb));
        assert!(!reference_pays(&[U, U], wb));
        // A Tundra pays {U} beside a Plains, or {W/B} beside an Island.
        assert!(reference_pays(&[W | U, W], wb));
        assert!(reference_pays(&[W | U, U], wb));
        assert!(!reference_pays(&[W | U, B | U], mana("{W/B}{W/B}{U}")));
    }
}
