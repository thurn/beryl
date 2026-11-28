//! The reference for payments with outlets: mana abilities whose cost sacrifices or taps a
//! permanent their controller chooses. Every order of every mana ability the player could
//! activate, outlets included, each taking any permanent its cost accepts, is tried on a
//! simulated board of the player's permanents, each untapped, tapped or gone, and the cost is
//! matched against the pool as [`super::reference_pays`] matches it (see [CR 605.3a]): "A
//! player may activate an activated mana ability ... whenever they are casting a spell or
//! activating an ability that requires a mana payment". It reads each cost off the cards and
//! shares no code with the engine's outlet search (`mana::rich`'s outlets).
//!
//! Implements: none. Checks payability with outlets by brute force.
//!
//! [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a

use std::collections::BTreeSet;

use crate::card::{
    Ability, AbilityKind, Chosen, CostAction, CostPart, Keywords, ManaCost, Timing, Types,
};
use crate::filter::Evaluate;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::mana::Paying;
use crate::state::{State, Zone};
use crate::table::GameTable;

use super::sequences::{Activation, activation_on, live_mana_changes, payments};
use super::{UNITS, controls, reference_pays, spendable};

/// The most permanents of the player's the brute force simulates: past it, it says nothing.
const MOST_PERMANENTS: usize = 10;
/// The most positions it visits before it gives up and says nothing.
const MOST_POSITIONS: usize = 200_000;

/// Whether `a` is a mana ability whose cost chooses a permanent to pay it: read off its cost,
/// as the engine's own test of an outlet is not used here.
pub(super) fn is_outlet(a: &Ability) -> bool {
    a.kind() == AbilityKind::Mana
        && (a.cost().parts().iter()).any(|part| matches!(part, CostPart::Chosen(..)))
}

/// An outlet as its cost reads: whether it taps its own permanent, what it does to the one
/// chosen and which permanents it accepts, and what it adds.
struct OutletCost {
    taps_itself: bool,
    action: CostAction,
    chosen: Chosen,
    kinds: u8,
    amount: u8,
}

/// One of the player's permanents: whether its {T} abilities may be activated, the mana
/// abilities with {T} it has, each with whether it sacrifices the permanent, and its outlets.
struct Permanent<L: Layout> {
    slot: L::Slot,
    can_tap: bool,
    taps: Vec<(Activation, bool)>,
    outlets: Vec<OutletCost>,
}

/// Whether some outlet `player` controls could take a permanent: whether
/// [`reference_outlets`] is the reference for their payments.
pub(crate) fn controls_outlet<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> bool {
    s.battlefield().iter().any(|&slot| {
        s.controller(slot) == player && s.abilities(t, slot).any(|(_, a)| is_outlet(a))
    })
}

/// Whether `player` can pay `cost` (X announced) for what `paying` is for by activating any of
/// their mana abilities, outlets included, in any order. `None` where the board is more than
/// the brute force tries, or has an outlet or a mana ability it does not read.
pub(crate) fn reference_outlets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    paying: Paying<L>,
) -> Option<bool> {
    let mut permanents: Vec<Permanent<L>> = Vec::new();
    for &slot in s.battlefield() {
        if s.controller(slot) != player {
            continue;
        }
        permanents.push(permanent(s, t, player, paying, slot)?);
    }
    if permanents.len() > MOST_PERMANENTS {
        return None;
    }
    let mut pool = Vec::new();
    for (k, &n) in s.player(player).mana().amounts_u8().iter().enumerate() {
        pool.extend(std::iter::repeat_n(1u8 << k, usize::from(n)));
    }
    let status: Vec<u8> = (permanents.iter())
        .map(|p| u8::from(s.object(p.slot).is_tapped()))
        .collect();
    let mut search = Brute {
        s,
        t,
        player,
        cost,
        permanents: &permanents,
        seen: BTreeSet::new(),
    };
    search.search(&status, pool, Vec::new())
}

/// The permanent in `slot` as the brute force reads it: `None` for a mana ability it does not
/// read.
fn permanent<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    paying: Paying<L>,
    slot: L::Slot,
) -> Option<Permanent<L>> {
    let (d, o) = (s.derived(slot), s.object(slot));
    let can_tap = !(d.types.intersects(Types::CREATURE)
        && o.is_summoning_sick()
        && !d.keywords.contains(Keywords::HASTE));
    let (extra_terms, type_produced, multiplier) = live_mana_changes(s, t, slot)?;
    let mut taps = Vec::new();
    let intrinsic = s.intrinsic_mana(t, slot);
    for k in (0..6).map(|k| 1u8 << k).filter(|k| intrinsic & k != 0) {
        let mut terms = vec![(k, multiplier, false)];
        terms.extend_from_slice(&extra_terms);
        if type_produced > 0 {
            terms.push((k, type_produced, false));
        }
        taps.push((Activation::of(ManaCost::NONE, &terms), false));
    }
    let mut outlets = Vec::new();
    let words = s.words_of(slot);
    for (_, a) in s.abilities(t, slot) {
        if a.kind() != AbilityKind::Mana
            || a.timing() == Timing::AsInstant
            || !spendable(s, t, paying, slot, a)
            || !controls(s, t, player, a, &words)
        {
            continue;
        }
        if is_outlet(a) {
            outlets.push(outlet_cost(a)?);
            continue;
        }
        let sacrifices = (a.cost().parts().iter())
            .any(|part| matches!(part, CostPart::This(CostAction::Sacrifice)));
        let activation = activation_on(s, t, slot, a, &extra_terms, type_produced, multiplier)?;
        taps.push((activation, sacrifices));
    }
    Some(Permanent {
        slot,
        can_tap,
        taps,
        outlets,
    })
}

/// An outlet's cost: `None` for one with mana in its cost, a part besides {T} and one
/// permanent chosen on the battlefield, or mana of several kinds at once.
fn outlet_cost(a: &Ability) -> Option<OutletCost> {
    if !a.cost().mana.is_zero() {
        return None;
    }
    let mut taps_itself = false;
    let mut chosen = None;
    for part in a.cost().parts() {
        if *part == CostPart::This(CostAction::Tap) {
            taps_itself = true;
        } else if let CostPart::Chosen(action @ (CostAction::Sacrifice | CostAction::Tap), c) =
            *part
            && c.zone == Zone::Battlefield
            && c.measure == crate::card::Measure::Count(1)
            && chosen.is_none()
        {
            chosen = Some((action, c));
        } else {
            return None;
        }
    }
    let (action, chosen) = chosen?;
    let product = a.produces();
    Some(OutletCost {
        taps_itself,
        action,
        chosen,
        kinds: product.colors,
        amount: product.amount,
    })
}

/// The fixed inputs of one [`reference_outlets`] and the positions it has seen.
struct Brute<'a, L: Layout> {
    s: &'a State<L>,
    t: &'a GameTable,
    player: Seat,
    cost: ManaCost,
    permanents: &'a [Permanent<L>],
    seen: BTreeSet<(Vec<u8>, Vec<u8>, Vec<u8>)>,
}

/// A permanent's state on the simulated board.
const UNTAPPED: u8 = 0;
const TAPPED: u8 = 1;
const GONE: u8 = 2;

impl<L: Layout> Brute<'_, L> {
    /// Whether the cost is paid from `pool` and `spare`, which pays only the cost, or after one
    /// more mana ability, with the permanents as `status` says.
    fn search(&mut self, status: &[u8], mut pool: Vec<u8>, mut spare: Vec<u8>) -> Option<bool> {
        if pool.len() + spare.len() > UNITS || self.seen.len() > MOST_POSITIONS {
            return None;
        }
        if reference_pays(&[pool.as_slice(), &spare].concat(), self.cost) {
            return Some(true);
        }
        pool.sort_unstable();
        spare.sort_unstable();
        if !self
            .seen
            .insert((status.to_vec(), pool.clone(), spare.clone()))
        {
            return Some(false);
        }
        let mut unknown = false;
        let mut try_next =
            |this: &mut Self, status: Vec<u8>, pool: Vec<u8>, spare: Vec<u8>| match this
                .search(&status, pool, spare)
            {
                Some(true) => true,
                Some(false) => false,
                None => {
                    unknown = true;
                    false
                }
            };
        let permanents = self.permanents;
        for (i, p) in permanents.iter().enumerate() {
            // Its own mana abilities with {T}.
            if status[i] == UNTAPPED && p.can_tap {
                for (activation, sacrifices) in &p.taps {
                    for left in payments(&pool, activation.price) {
                        for (made, only_cost) in &activation.makes {
                            let mut next = status.to_vec();
                            next[i] = if *sacrifices { GONE } else { TAPPED };
                            let mut p2 = left.clone();
                            p2.extend(made);
                            let mut s2 = spare.clone();
                            s2.extend(only_cost);
                            if try_next(self, next, p2, s2) {
                                return Some(true);
                            }
                        }
                    }
                }
            }
            // Its outlets, each taking any permanent its cost accepts.
            if status[i] == GONE {
                continue;
            }
            for outlet in &p.outlets {
                if outlet.taps_itself && (status[i] != UNTAPPED || !p.can_tap) {
                    continue;
                }
                for j in 0..permanents.len() {
                    if !self.accepts(outlet, i, j, status) {
                        continue;
                    }
                    let mut next = status.to_vec();
                    if outlet.taps_itself {
                        next[i] = TAPPED;
                    }
                    next[j] = if outlet.action == CostAction::Sacrifice {
                        GONE
                    } else {
                        TAPPED
                    };
                    for made in made_by(outlet) {
                        let mut p2 = pool.clone();
                        p2.extend(made);
                        if try_next(self, next.clone(), p2, spare.clone()) {
                            return Some(true);
                        }
                    }
                }
            }
        }
        (!unknown).then_some(false)
    }

    /// Whether `outlet`, of the permanent at place `i`, may take the permanent at place `j`.
    fn accepts(&self, outlet: &OutletCost, i: usize, j: usize, status: &[u8]) -> bool {
        let (outlet_slot, slot) = (self.permanents[i].slot, self.permanents[j].slot);
        let words = self.s.words_of(outlet_slot);
        status[j] != GONE
            && !(outlet.chosen.other && i == j)
            && !(outlet.action == CostAction::Tap
                && (status[j] != UNTAPPED || (outlet.taps_itself && i == j)))
            && outlet
                .chosen
                .filter
                .matches(self.s, self.t, slot, self.player, &words)
    }
}

/// Each list of units `outlet` may add: its amount of one kind, or of each of its kinds where it
/// adds several of one kind chosen, or one unit of any of them.
fn made_by(outlet: &OutletCost) -> Vec<Vec<u8>> {
    let amount = usize::from(outlet.amount);
    if outlet.kinds.is_power_of_two() || outlet.amount == 1 {
        return vec![vec![outlet.kinds; amount]];
    }
    (0..6)
        .map(|k| 1u8 << k)
        .filter(|k| outlet.kinds & k != 0)
        .map(|k| vec![k; amount])
        .collect()
}
