//! What outlets could add to a payment: mana abilities whose cost sacrifices or taps a
//! permanent their controller chooses ([`outlet_part`]), as "Sacrifice an artifact: Add
//! {C}{C}". The solver does not plan them, since which permanent is given up is a choice with
//! its own consequences, but whether a cost can be paid must count them, so that a spell
//! payable only by sacrificing to an outlet is offered.
//!
//! The search here tries every order of outlet activations over the permanents they could
//! take, each paying its own cost (see [CR 605.3a]): "A player may activate an activated mana
//! ability ... whenever they are casting a spell or activating an ability that requires a mana
//! payment". After each, the rest is checked as [`super::pays_from_units`] checks it, with
//! the permanents used so far left out. A permanent is taken at most once. A permanent may be
//! tapped for its own mana before it is sacrificed, since a tapped permanent can still be
//! sacrificed. An outlet may be sacrificed to itself, which ends its use, and one with {T} in
//! its cost is used once.
//!
//! Implements: none. Counts outlet mana in the payability check of CR 605.3a.
//!
//! [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a

use std::collections::BTreeSet;

use crate::bitboard::Bitboard;
use crate::card::{Ability, CostAction, ManaCost, ObjectAbility};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::State;
use crate::table::{GameTable, outlet_part};

use super::super::outlet::outlet_candidates;
use super::super::remembered::Paying;
use super::super::sources::{Sources, allowed, can_tap, controlled};
use super::super::units::Units;
use super::{each_units_payment, pays_from_units};

/// Every kind of mana, W U B R G and C: a unit that pays any symbol.
const EVERY_KIND: u8 = 0b11_1111;

/// One outlet mana ability that a player could activate now, and the permanents it could take.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Outlet<L: Layout> {
    /// The permanent whose ability it is.
    pub(crate) slot: L::Slot,
    pub(crate) ability: ObjectAbility,
    pub(crate) printed: &'static Ability,
    /// Whether its cost has {T}, which taps the outlet itself.
    taps_self: bool,
    /// Whether its cost sacrifices the permanent chosen, or taps it.
    action: CostAction,
    /// The permanents its cost may take now.
    pub(crate) fodder: Bitboard<L::Slot>,
}

impl<L: Layout> Outlet<L> {
    /// The kinds of mana the ability may add, a bit per [`crate::card::ManaColor`]: one kind,
    /// or any color, chosen as it is activated.
    pub(crate) fn kinds(&self) -> u8 {
        self.printed.produces().colors
    }

    /// Whether the outlet sacrifices the permanent it takes, rather than tapping it.
    pub(crate) fn sacrifices(&self) -> bool {
        self.action == CostAction::Sacrifice
    }
}

/// The outlet mana abilities `player` could activate now on the permanents they control but
/// `exclude`, each with the permanents but `exclude` that its cost may take. Empty in a game
/// whose cards have none. Out of line: asked only in a game with priced mana abilities.
#[cold]
#[inline(never)]
pub(crate) fn outlets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard<L::Slot>,
) -> Vec<Outlet<L>> {
    let mut out = Vec::new();
    for (slot, d) in controlled(s, player) {
        if exclude.contains(slot) {
            continue;
        }
        for (ability, a) in s.abilities(t, slot) {
            let Some((action, chosen)) = outlet_part(a) else {
                continue;
            };
            let taps_self = a.cost().does(CostAction::Tap);
            if (taps_self && !can_tap(s, player, slot, d))
                || !allowed(s, t, a, player, &s.ability_words(slot, ability))
            {
                continue;
            }
            let fodder = outlet_candidates(s, t, player, slot, a, action, &chosen) & !exclude;
            if fodder.is_empty() {
                continue;
            }
            out.push(Outlet {
                slot,
                ability,
                printed: a,
                taps_self,
                action,
                fodder,
            });
        }
    }
    out
}

/// Whether `player` can pay `cost` with none of `exclude` for what `paying` is for, activating
/// outlets in some order as well as their other mana abilities. `false` at once where they
/// control no outlet that could take a permanent.
#[cold]
#[inline(never)]
pub(in crate::mana) fn pays_through_outlets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> Result<bool, Full> {
    let outlets = outlets(s, t, player, exclude);
    if outlets.is_empty() {
        return Ok(false);
    }
    let mut pool = Units::NONE;
    for (k, &n) in s.player(player).mana().amounts_u8().iter().enumerate() {
        pool.add(1 << k, u16::from(n));
    }
    let search = Search {
        s,
        t,
        player,
        cost,
        exclude,
        paying,
        sources: Sources::of(s, t, player, exclude, paying)?,
        outlets: &outlets,
    };
    // No order of activations makes more than every permanent an outlet could take tapped by
    // the outlet that taps one for the most mana and then sacrificed to the one that sacrifices
    // one for the most, each unit any kind, beside all their own mana.
    let mut most = pool;
    let fodder = outlets
        .iter()
        .fold(Bitboard::EMPTY, |all, o| all | o.fodder);
    for slot in fodder.sorted(|slot| slot) {
        for action in [CostAction::Tap, CostAction::Sacrifice] {
            let best = (outlets.iter())
                .filter(|o| o.action == action && o.fodder.contains(slot))
                .map(|o| o.printed.produces().amount)
                .max()
                .unwrap_or(0);
            most.add(EVERY_KIND, u16::from(best));
        }
    }
    if !search.rest_pays(most, Units::NONE, Bitboard::EMPTY)? {
        return Ok(false);
    }
    let mut seen = BTreeSet::new();
    search.dfs(
        Bitboard::EMPTY,
        Bitboard::EMPTY,
        pool,
        Units::NONE,
        &mut seen,
    )
}

/// The fixed inputs of one [`pays_through_outlets`].
struct Search<'a, L: Layout> {
    s: &'a State<L>,
    t: &'a GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
    /// The sources with nothing used, for what a permanent makes when it is tapped for its
    /// own mana before it is sacrificed.
    sources: Sources<L>,
    outlets: &'a [Outlet<L>],
}

/// A position of the search: the permanents sacrificed, those tapped, the mana that may pay a
/// price or the cost, and the mana that pays only the cost.
type Position<L> = (
    Vec<<L as Layout>::Slot>,
    Vec<<L as Layout>::Slot>,
    Units,
    Units,
);

impl<L: Layout> Search<'_, L> {
    /// Whether the cost is paid from `pool` and `restricted` and the other mana abilities of
    /// the permanents not in `used`.
    fn rest_pays(
        &self,
        pool: Units,
        restricted: Units,
        used: Bitboard<L::Slot>,
    ) -> Result<bool, Full> {
        let sources = Sources::of(
            self.s,
            self.t,
            self.player,
            self.exclude | used,
            self.paying,
        )?;
        Ok(pays_from_units(pool, &sources, restricted, self.cost))
    }

    fn dfs(
        &self,
        gone: Bitboard<L::Slot>,
        tapped: Bitboard<L::Slot>,
        pool: Units,
        restricted: Units,
        seen: &mut BTreeSet<Position<L>>,
    ) -> Result<bool, Full> {
        let position = (
            gone.sorted(|slot| slot),
            tapped.sorted(|slot| slot),
            pool,
            restricted,
        );
        if !seen.insert(position) {
            return Ok(false);
        }
        if self.rest_pays(pool, restricted, gone | tapped)? {
            return Ok(true);
        }
        for o in self.outlets {
            if gone.contains(o.slot) || (o.taps_self && tapped.contains(o.slot)) {
                continue;
            }
            let mut able = o.fodder & !gone;
            if o.action == CostAction::Tap {
                able &= !tapped;
            }
            let takes = collapse(self.s, able & !tapped) | collapse(self.s, able & tapped);
            for x in takes.sorted(|slot| slot) {
                let mut next_gone = gone;
                let mut next_tapped = tapped;
                if o.action == CostAction::Sacrifice {
                    next_gone.insert(x);
                } else {
                    next_tapped.insert(x);
                }
                if o.taps_self {
                    next_tapped.insert(o.slot);
                }
                // Tapped for its own mana first, where it can be: only before it is
                // sacrificed, never when the outlet's own {T} taps it.
                let first = o.action == CostAction::Sacrifice
                    && !tapped.contains(x)
                    && !(o.taps_self && x == o.slot);
                let mut starts = vec![(pool, restricted)];
                if first {
                    self.own_mana(x, pool, restricted, &mut starts);
                }
                for (p, r) in starts {
                    let mut found = false;
                    for p in Self::outlet_mana(o, p) {
                        if self.dfs(next_gone, next_tapped, p, r, seen)? {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Adds to `out` each way the permanent in `slot` could add its own mana before it is
    /// sacrificed: a source's one tap, unless tapping it sacrifices it or its mana pays only a
    /// spell, or one activation of a mana ability the classes cannot say, its price paid from
    /// `pool`.
    fn own_mana(
        &self,
        slot: L::Slot,
        pool: Units,
        restricted: Units,
        out: &mut Vec<(Units, Units)>,
    ) {
        if let Some(c) =
            (self.sources.classes().iter()).find(|c| !c.is_twin() && c.members.contains(slot))
        {
            if !c.is_consumed() && !c.is_restricted() {
                let mut p = pool;
                p.add(c.kinds, u16::from(c.amount));
                out.push((p, restricted));
            }
            return;
        }
        let Some(extra) = (self.sources.extras.iter()).find(|e| e.members.contains(slot)) else {
            return;
        };
        for act in extra.activations.iter().filter(|a| !a.consumed) {
            each_units_payment(pool, act.price, &mut |left| {
                act.each_units_split(left, restricted, &mut |p, r| {
                    out.push((p, r));
                    true
                })
            });
        }
    }

    /// The pools after `o` adds its mana to `pool`: one for each kind it could add, where its
    /// mana is all one kind chosen from several.
    fn outlet_mana(o: &Outlet<L>, pool: Units) -> Vec<Units> {
        let product = o.printed.produces();
        let amount = u16::from(product.amount);
        if product.colors.is_power_of_two() || product.amount == 1 {
            let mut p = pool;
            p.add(product.colors, amount);
            return vec![p];
        }
        (0..6)
            .map(|k| 1u8 << k)
            .filter(|k| product.colors & k != 0)
            .map(|k| {
                let mut p = pool;
                p.add(k, amount);
                p
            })
            .collect()
    }
}
