//! Slow reference implementations of the engine's fast paths, which fuzzing and the scenario
//! runner check the state against at every decision (architecture §17.4). Each answers the
//! question its fast path answers from first principles, sharing as little of its code as it
//! can: a disagreement is an engine bug, never a rules outcome.
//!
//! - The layer results and the caches found with them, against a recompute from scratch.
//! - Payability: the prefilter, the cached funds and the solver's plans, against a matching
//!   of each mana symbol to a unit of mana from the pool or a mana ability read off the cards.

use crate::bitboard::Bitboard;
use crate::card::{AbilityKind, Cost, ManaCost, Types};
use crate::event::Sink;
use crate::ids::{Seat, Slot};
use crate::invariants::{At, Violation};
use crate::layers;
use crate::layout::{Layout, Queue, Table};
use crate::mana::{self, Funds};
use crate::state::{ObjectFlags, State, Zone};
use crate::table::GameTable;
use crate::trigger::face;

fn check(ok: bool, what: &'static str, at: Option<At>) -> Result<(), Violation> {
    if ok {
        Ok(())
    } else {
        Err(Violation { what, at })
    }
}

/// Costs every check tries besides those of the cards in play: generic amounts, each kind of
/// symbol alone, and the pairs a two-color deck's spells ask for.
const PROBES: &[ManaCost] = &[
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

impl<L: Layout> State<L> {
    /// Checks the engine's fast paths against their reference implementations. It holds
    /// wherever [`State::check_invariants`] does, and checks nothing once the game is over.
    pub fn check_references(&self, t: &GameTable) -> Result<(), Violation> {
        if self.outcome.is_some() || self.flags.derived_dirty {
            return Ok(());
        }
        self.check_layers(t)?;
        for p in Seat::BOTH {
            self.check_payments(t, p)?;
        }
        Ok(())
    }

    /// The layer results, the listeners and the knowledge the layers give, against a
    /// recompute from base ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131)):
    /// "The values of an object's characteristics are determined by starting with the actual
    /// object ... Then all applicable continuous effects are applied in a series of layers".
    /// A chokepoint that changes a layer input without marking the results dirty leaves them
    /// stale, which this sees and the dirty flag cannot.
    fn check_layers(&self, t: &GameTable) -> Result<(), Violation> {
        let mut fresh = self.clone();
        fresh.flags.derived_dirty = true;
        layers::refresh(&mut fresh, t, &mut Sink::default());
        let n = self.battlefield.len();
        for i in 0..n {
            check(
                fresh.derived[i] == self.derived[i],
                "layer results differ from a fresh recompute",
                Some(At::Slot(self.battlefield.as_slice()[i])),
            )?;
        }
        check(
            fresh.listening == self.listening,
            "the trigger listeners differ from a fresh recompute",
            None,
        )?;
        check(
            fresh.knowledge == self.knowledge,
            "knowledge differs from a fresh layer recompute",
            None,
        )
    }

    /// Whether `player` can pay each cost of the cards they hold and control, and some
    /// probes, as the prefilter, the cached funds and the solver say, against
    /// [`reference_pays`].
    fn check_payments(&self, t: &GameTable, player: Seat) -> Result<(), Violation> {
        let Some(units) = reference_units(self, t, player) else {
            return Ok(());
        };
        let at = Some(At::Player(player));
        let mut costs: Vec<ManaCost> = PROBES.to_vec();
        for slot in self.zone(player, Zone::Hand).iter() {
            costs.extend(t.def(self.card(slot)).faces.iter().map(|f| f.cost));
        }
        for &slot in self.battlefield() {
            if self.controller(slot) == player {
                costs.extend(
                    face(self, t, slot)
                        .abilities
                        .iter()
                        .filter(|a| a.kind == AbilityKind::Activated)
                        .map(|a| a.cost.mana),
                );
            }
        }
        let Ok(funds) = Funds::of(self, t, player) else {
            return Ok(());
        };
        for cost in costs {
            // X is announced before payment, as 0 here.
            let cost = ManaCost { x: 0, ..cost };
            let expected = reference_pays(&units, cost);
            let none = Bitboard::EMPTY;
            let Ok(prefilter) = mana::payable(self, t, player, cost, none) else {
                continue;
            };
            check(
                prefilter == expected,
                "the payability prefilter disagrees with the reference",
                at,
            )?;
            check(
                funds.pays(self, t, cost, none) == expected,
                "the cached funds disagree with the reference",
                at,
            )?;
            for greedy in [false, true] {
                let plans = mana::plans(self, t, &mut Sink::default(), player, cost, none, greedy);
                if let Ok(plans) = plans {
                    check(
                        plans.is_empty() != expected,
                        "the payment solver disagrees with the reference",
                        at,
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// The units of mana `player` could make now, each as the set of kinds it may be: their
/// floating mana, and one tap of each untapped permanent they control with a mana ability
/// whose only costs are {T} and perhaps sacrificing it, read from its face up. `None` where a
/// permanent's mana abilities make different amounts, which units cannot say.
///
/// [CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a): "An activated ability is
/// a mana ability if it meets all of the following criteria: it doesn't require a target, it
/// could add mana to a player's mana pool when it resolves, and it's not a loyalty ability."
/// [CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026): "A creature's activated
/// ability with the tap symbol or the untap symbol in its activation cost can't be activated
/// unless the creature has been under its controller's control continuously since their most
/// recent turn began."
fn reference_units<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Option<Vec<u8>> {
    let mut units = Vec::new();
    for (k, &n) in s.player(player).mana.amounts().iter().enumerate() {
        units.extend(std::iter::repeat_n(1u8 << k, usize::from(n)));
    }
    for &slot in s.battlefield() {
        let d = s.derived(slot);
        let o = s.object(slot);
        if d.controller != player || o.flags.has(ObjectFlags::TAPPED) {
            continue;
        }
        if d.types.intersects(Types::CREATURE) && o.flags.has(ObjectFlags::SUMMONING_SICK) {
            continue;
        }
        let mut kinds = 0u8;
        let mut amount = None;
        for a in face(s, t, slot).abilities {
            let tap_only = Cost {
                tap: true,
                sacrifice_self: a.cost.sacrifice_self,
                ..Cost::NONE
            };
            if a.kind != AbilityKind::Mana || a.cost != tap_only || !controls(s, t, player, a) {
                continue;
            }
            if amount.is_some_and(|n| n != a.produces.amount) {
                return None;
            }
            amount = Some(a.produces.amount);
            kinds |= a.produces.colors;
        }
        if let Some(n) = amount.filter(|_| kinds != 0) {
            units.extend(std::iter::repeat_n(kinds, usize::from(n)));
        }
    }
    Some(units)
}

/// Whether `player` meets a mana ability's "Activate only if you control" condition
/// ([CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)).
fn controls<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    a: &crate::card::Ability,
) -> bool {
    a.only_if_you_control.is_none_or(|f| {
        s.battlefield()
            .iter()
            .any(|&slot: &Slot| s.controller(slot) == player && f.matches(s, t, slot, player))
    })
}

/// Whether `units` pay `cost`: each colored or colorless symbol matched to a distinct unit
/// that may be its kind (augmenting paths), and enough units left for the generic part.
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the total
/// cost. ... Partial payments are not allowed."
fn reference_pays(units: &[u8], cost: ManaCost) -> bool {
    let symbols: Vec<u8> = (0..6)
        .flat_map(|k| std::iter::repeat_n(1u8 << k, usize::from(cost.symbols[k])))
        .collect();
    if units.len() < symbols.len() + usize::from(cost.generic) {
        return false;
    }
    let mut paying: Vec<Option<usize>> = vec![None; units.len()];
    for symbol in 0..symbols.len() {
        let mut seen = vec![false; units.len()];
        if !augment(symbol, &symbols, units, &mut paying, &mut seen) {
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
    use crate::card::ManaCost;

    const W: u8 = 1;
    const U: u8 = 2;
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
}
