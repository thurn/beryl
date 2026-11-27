//! What a pool and its sources can pay: the funds a cost is checked against.
//!
//! Implements: none. Checks whether the pool and available sources can cover a cost.

use crate::bitboard::Bitboard;
use crate::card::{Hybrid, ManaCost};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::State;
use crate::table::{Capability, GameTable};

use super::remembered::{self, Paying};
use super::sources::{available, capacity, controlled, sacrifice_alternative, source};
use super::{
    CLASSES,
    units::{Groups, Units},
};

/// Everything a player could pay mana with now, found once for the many payability checks of
/// one decision: their pool and untapped sources, as units. What a source makes doesn't
/// depend on which others a check excludes, so [`Funds::pays`] answers as [`payable`] does,
/// taking an excluded source's units back out of the whole.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Funds<L: Layout = Small> {
    player: Seat,
    /// The sources' slots.
    slots: Bitboard<L::Slot>,
    /// What the pool and all the sources make.
    units: Units,
    /// The same, grouped, for the checks that exclude none.
    all: Groups,
}

impl<L: Layout> Funds<L> {
    pub(crate) fn of(s: &State<L>, t: &GameTable, player: Seat) -> Result<Funds<L>, Full> {
        let mut units = Units::NONE;
        for (k, &n) in s.player(player).mana().amounts_u8().iter().enumerate() {
            units.add(1 << k, u16::from(n));
        }
        let mut slots = Bitboard::EMPTY;
        // The classes [`Sources::of`] would form, so that more than it holds is `Full` here
        // too.
        let mut classes: L::Queue<_, CLASSES> = Default::default();
        let alternatives = t.can(Capability::SacrificeAlternative);
        for (slot, d) in controlled(s, player) {
            if let Some((kinds, amount, other, consumed)) = source(s, t, player, slot, d) {
                slots.insert(slot);
                // A source that may also sacrifice itself for mana: its twin class too.
                let alternative = (!consumed)
                    .then(|| sacrifice_alternative(s, t, player, slot, alternatives))
                    .flatten();
                if let Some((kinds, amount)) = alternative {
                    classes.push((kinds, amount, true, Some(s.definition(slot).key())))?;
                }
                let (most, n) = capacity(kinds, amount, alternative);
                units.add(most, u16::from(n));
                // Sources a plan consumes are a class per card.
                let card = consumed.then(|| s.definition(slot).key());
                let class = (kinds, amount, other, card);
                if other || !classes.iter().any(|&c| c == class) {
                    classes.push(class)?;
                }
            }
        }
        Ok(Funds {
            player,
            slots,
            units,
            all: units.groups(),
        })
    }

    #[inline]
    pub(super) fn player(&self) -> Seat {
        self.player
    }

    /// Whether the player can pay `cost` with none of `exclude` for no spell, as [`payable`]
    /// does with [`Paying::NOTHING`]. `s` is the state the funds were found in.
    pub(crate) fn pays(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
    ) -> bool {
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        let excluded = exclude & self.slots;
        if excluded.is_empty() {
            return self.all.pays(cost);
        }
        self.without_sources(s, t, excluded).pays(cost)
    }

    #[inline]
    fn without_sources(&self, s: &State<L>, t: &GameTable, excluded: Bitboard<L::Slot>) -> Units {
        let alternatives = t.can(Capability::SacrificeAlternative);
        excluded.fold(self.units, |mut units, slot| {
            let (kinds, amount, _, consumed) =
                source(s, t, self.player, slot, s.derived(slot)).expect("a source");
            let alternative = (!consumed)
                .then(|| sacrifice_alternative(s, t, self.player, slot, alternatives))
                .flatten();
            let (kinds, amount) = capacity(kinds, amount, alternative);
            units.take(kinds, u16::from(amount));
            units
        })
    }

    /// Whether the player can pay `cost` with none of `exclude` for what `paying` is for:
    /// [`Funds::pays`], or, where that fails for a spell in a game whose cards make restricted
    /// mana, [`Funds::pays_restricted`], or, where both fail in a game whose cards have priced
    /// mana abilities or products of several terms, the rich path's answer
    /// (`mana::rich::pays`). Restricted mana and those sources only add to what pays.
    #[inline]
    pub(crate) fn pays_for(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
    ) -> bool {
        self.pays(s, t, cost, exclude)
            || (paying.reads_restrictions(t) && self.pays_restricted(s, t, cost, exclude, paying))
            || (t.can_either(Capability::PricedMana, Capability::ManaChanges)
                && super::rich::pays(self, s, t, cost, exclude, paying))
    }

    /// [`Funds::pays_for`] for casting `face` of the card in `slot`, with nothing excluded:
    /// restricted mana that the spell accepts pays too, beside priced mana abilities.
    #[cold]
    #[inline(never)]
    pub(crate) fn pays_casting(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        slot: L::Slot,
        face: crate::card::FaceIx,
    ) -> bool {
        let paying = Paying::casting(s, t, slot, face);
        self.pays_for(s, t, cost, Bitboard::EMPTY, paying)
    }

    /// [`Funds::pays`] for a spell, in a game whose cards make restricted mana: a source of
    /// restricted mana the spell's restriction accepts is one unit of the kinds of either of
    /// its mana abilities, and the mana that remembers in the pool that it accepts adds its
    /// units.
    #[cold]
    #[inline(never)]
    pub(crate) fn pays_restricted(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
    ) -> bool {
        // More sources of restricted mana than a [`Small`] payment holds: say it may be paid,
        // so that the payment's own scan finds the layout `Full` and the game goes on in
        // `Large`.
        let Ok(restricted) = remembered::restricted_sources(s, t, self.player) else {
            return true;
        };
        self.pays_restricted_with(s, t, cost, exclude, paying, restricted.as_slice())
    }

    /// [`Funds::pays_restricted`] with the player's restricted sources already found for this
    /// decision.
    #[cold]
    #[inline(never)]
    pub(crate) fn pays_restricted_with(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
        restricted: &[remembered::Restricted<L>],
    ) -> bool {
        let entries = remembered::accepted_entries(s, t, self.player, paying);
        let any_source = restricted
            .iter()
            .any(|r| !exclude.contains(r.slot) && paying.accepts(t, r.restriction, r.chosen_type));
        if !any_source && entries.is_empty() {
            return false;
        }
        self.restricted_units(s, t, exclude, paying, restricted)
            .pays(cost)
    }

    /// The units of [`Funds::pays_restricted_with`]: the pool and the sources but `exclude`,
    /// with the restricted sources and the mana that remembers that `paying` accepts.
    fn restricted_units(
        &self,
        s: &State<L>,
        t: &GameTable,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
        restricted: &[remembered::Restricted<L>],
    ) -> Units {
        let entries = remembered::accepted_entries(s, t, self.player, paying);
        let mut u = self.without_sources(s, t, exclude & self.slots);
        for r in restricted {
            if exclude.contains(r.slot) || !paying.accepts(t, r.restriction, r.chosen_type) {
                continue;
            }
            match source(s, t, self.player, r.slot, s.derived(r.slot)) {
                Some((kinds, 1, _, _)) => {
                    u.take(kinds, 1);
                    u.add(kinds | r.kinds, 1);
                }
                _ => u.add(r.kinds, 1),
            }
        }
        for &(_, kind, _) in entries.iter() {
            u.add(1 << kind, 1);
        }
        u
    }

    /// Whether the player can pay `cost` for what `paying` is for, with none of `exclude`,
    /// tapping any of `objects` rather than paying one mana each (convoke, see CR 702.51a;
    /// improvise, see CR 702.126a). `kinds` gives the kinds of mana an object may pay that way, a
    /// bit per [`crate::card::ManaColor`]: a creature's colors for convoke, none (generic
    /// mana only) for a colorless creature or for improvise's artifacts. No object pays a {C}
    /// symbol, since none of them is colorless mana.
    ///
    /// An object that is no mana source is one more unit of its kinds. One that is a plain
    /// source of one mana is one unit of its mana's kinds or its own, since it either taps
    /// for mana or pays in its place, never both (see [CR 118.3]: "a permanent that's
    /// already tapped can't be tapped to pay a cost"). Any other source, one that makes more than one
    /// mana, may sacrifice itself for a different amount, or makes restricted mana, is tried
    /// both ways. Past [`BOTH_WAYS`] of those, the rest count as mana sources only, so that
    /// the answer may say no to a cost a payment could make, never yes to one it cannot. Where
    /// that says no in a game whose cards have priced mana abilities or change mana abilities,
    /// the rich path answers (`mana::rich::pays_with_objects`).
    ///
    /// [CR 118.3]: https://yawgatog.com/resources/magic-rules/#R1183
    #[cold]
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pays_with_objects(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
        objects: Bitboard<L::Slot>,
        kinds: impl Fn(L::Slot) -> u8,
    ) -> bool {
        // More restricted sources than a small layout holds: the payment's own scan finds
        // the layout full, and the game goes on in a larger one. In a game whose cards make
        // no restricted mana the list is empty.
        let Ok(restricted) = remembered::restricted_sources(s, t, self.player) else {
            return true;
        };
        let reads = paying.reads_restrictions(t);
        let restricted = restricted.as_slice();
        let accepted = |slot: L::Slot| {
            reads
                && restricted
                    .iter()
                    .any(|r| r.slot == slot && paying.accepts(t, r.restriction, r.chosen_type))
        };
        let alternatives = t.can(Capability::SacrificeAlternative);
        let objects = objects & !exclude;
        // Objects that are one unit of their own kinds or of their mana's: a plain source of
        // one mana, which makes the same mana whether or not a sacrifice alternative is
        // counted (a consumed source has none).
        let merged = objects.filter(|slot| {
            self.slots.contains(slot)
                && !accepted(slot)
                && matches!(
                    source(s, t, self.player, slot, s.derived(slot)),
                    Some((_, 1, _, consumed))
                        if consumed
                            || sacrifice_alternative(s, t, self.player, slot, alternatives)
                                .is_none()
                )
        });
        let free = objects.filter(|slot| !self.slots.contains(slot) && !accepted(slot));
        let both = objects & !merged & !free;
        let (both, len) = both.fold(
            ([(L::Slot::default(), 0u8); BOTH_WAYS], 0usize),
            |(mut list, len), slot| {
                if len < BOTH_WAYS {
                    list[len] = (slot, kinds(slot));
                    (list, len + 1)
                } else {
                    (list, len)
                }
            },
        );
        let both = &both[..len];
        let plain = (0u32..1 << len).any(|tapped| {
            let mut out = exclude;
            for (i, &(slot, _)) in both.iter().enumerate() {
                if tapped & (1 << i) != 0 {
                    out.insert(slot);
                }
            }
            let mut u = if reads {
                self.restricted_units(s, t, out, paying, restricted)
            } else {
                self.without_sources(s, t, out & self.slots)
            };
            for (i, &(_, k)) in both.iter().enumerate() {
                if tapped & (1 << i) != 0 {
                    u.add(k, 1);
                }
            }
            let u = free.fold(u, |mut u, slot| {
                u.add(kinds(slot), 1);
                u
            });
            let u = merged.fold(u, |mut u, slot| {
                let (made, _, _, _) =
                    source(s, t, self.player, slot, s.derived(slot)).expect("a source");
                u.take(made, 1);
                u.add(made | kinds(slot), 1);
                u
            });
            u.pays(cost)
        });
        // Under Mana Flare, or with priced mana abilities, the units above cannot say what the
        // sources make: the rich path tries the payment in sequence.
        plain
            || (t.can_either(Capability::PricedMana, Capability::ManaChanges)
                && super::rich::pays_with_objects(
                    s,
                    t,
                    self.player,
                    cost,
                    exclude,
                    paying,
                    objects,
                    &kinds,
                ))
    }

    /// The untapped sources that can pay for `paying`, including sources without an
    /// unrestricted mana ability. The same source cannot make mana and pay in its place
    /// (see [CR 701.67a]): "you may tap an untapped artifact or creature you control rather than
    /// pay that mana."
    ///
    /// [CR 701.67a]: https://yawgatog.com/resources/magic-rules/#R70167a
    #[inline]
    pub(crate) fn sources_for(
        &self,
        s: &State<L>,
        t: &GameTable,
        paying: Paying<L>,
    ) -> Result<Bitboard<L::Slot>, Full> {
        let mut sources = self.slots;
        if paying.reads_restrictions(t) {
            for source in remembered::restricted_sources(s, t, self.player)?.iter() {
                if paying.accepts(t, source.restriction, source.chosen_type) {
                    sources.insert(source.slot);
                }
            }
        }
        Ok(sources)
    }
}

/// The most objects [`Funds::pays_with_objects`] tries both as mana sources and as payment in
/// their place, which is `2^BOTH_WAYS` checks.
const BOTH_WAYS: usize = 10;

/// The payability prefilter: whether `player` can pay `cost` for what `paying` is for at all.
pub(crate) fn payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> Result<bool, Full> {
    if !t.can_either(Capability::PricedMana, Capability::ManaChanges) {
        return Ok(available(s, t, player, exclude, paying)?.pays(cost));
    }
    let sources = super::sources::Sources::of(s, t, player, exclude, paying)?;
    if sources
        .units(s.player(player).mana().amounts_u8(), &Table::filled(0))
        .pays(cost)
    {
        return Ok(true);
    }
    Ok(super::rich::pays_with_sources(s, player, cost, &sources))
}

/// Pays one unit toward `remaining`: a colored or colorless symbol of its kind if one is
/// left, then a hybrid symbol with a half of its kind, generic mana otherwise. Paying a symbol
/// of the unit's own kind first is never worse, and a hybrid symbol before generic mana
/// neither: generic mana takes any unit. Between two hybrid pairs that share the kind, it pays
/// the first in [`crate::card::Hybrid::PAIRS`] order.
pub(super) fn pay_unit(remaining: &mut ManaCost, kind: usize) {
    if remaining.symbols[kind] > 0 {
        remaining.symbols[kind] -= 1;
    } else if let Some(pair) = hybrid_pair(*remaining, kind) {
        remaining.hybrid = remaining.hybrid.without_one(pair);
    } else {
        debug_assert!(remaining.generic > 0);
        remaining.generic -= 1;
    }
}

/// Whether a unit of `kind` can pay part of `remaining`.
pub(super) fn helps(remaining: ManaCost, kind: usize) -> bool {
    remaining.symbols[kind] > 0 || remaining.generic > 0 || hybrid_pair(remaining, kind).is_some()
}

/// The first hybrid pair of `remaining` with a half of `kind` (see CR 107.4e).
#[inline]
fn hybrid_pair(remaining: ManaCost, kind: usize) -> Option<u8> {
    if remaining.hybrid.is_empty() {
        None
    } else {
        first_pair_with(remaining.hybrid, kind)
    }
}

/// [`hybrid_pair`] of a cost with hybrid symbols, out of line.
#[cold]
#[inline(never)]
fn first_pair_with(hybrid: Hybrid, kind: usize) -> Option<u8> {
    hybrid
        .pairs()
        .map(|(pair, _)| pair)
        .find(|&pair| pair & (1 << kind) != 0)
}
