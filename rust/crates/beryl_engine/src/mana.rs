//! Mana: mana abilities, the payability prefilter and the payment solver (architecture §10,
//! decision D7).
//!
//! [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "When an effect instructs a
//! player to add mana, that mana goes into a player's mana pool. From there, it can be used to
//! pay costs immediately, or it can stay in the player's mana pool as unspent mana."
//!
//! The solver works over **sources**: permanents whose mana abilities cost only {T} and do
//! nothing but add mana, the lands and rocks of the cube, and those whose mana ability costs
//! {T} and sacrificing them, the Treasures. Sources that make the same mana and have no other
//! ability are interchangeable, so they form a **class**, and a plan says how many of each
//! class it taps and how much floating mana of each kind it spends. Plans that leave the same
//! mana floating and sacrifice as many sources differ only in which sources they tap: the
//! solver picks one. Plans that leave different mana floating are a decision, and so is
//! sacrificing more, when it keeps more later costs payable (§10).

use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityKind, Cost, Face, ManaColor, ManaCost, Types};
use crate::decision::Opt;
use crate::effect::EffectCtx;
use crate::event::{self, Sink};
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat, Slot};
use crate::layout::{Layout, Queue, Table};
use crate::state::{Derived, ObjectFlags, PendingTrigger, State, Zone};
use crate::table::GameTable;

/// The kinds of mana, W U B R G and C, as [`ManaColor`] indices.
const KINDS: usize = 6;
/// Classes of interchangeable sources one solve considers.
const CLASSES: usize = 16;

/// Mana available to pay with, counted by the set of kinds each unit can be: a floating
/// {W} is one unit of `{W}`, an untapped Tundra one unit of `{W, U}`, a Mana Crypt two units
/// of `{C}`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Units {
    by_kinds: [u16; 1 << KINDS],
    /// A bit per set of kinds with a unit, so reading the units skips the empty sets.
    some: u64,
}

impl Default for Units {
    fn default() -> Self {
        Units::NONE
    }
}

impl Units {
    const NONE: Units = Units {
        by_kinds: [0; 1 << KINDS],
        some: 0,
    };

    pub(crate) fn add(&mut self, kinds: u8, n: u16) {
        let k = &mut self.by_kinds[usize::from(kinds)];
        *k = k.saturating_add(n);
        if *k > 0 {
            self.some |= 1 << kinds;
        }
    }

    /// Takes back `n` units that [`Units::add`] added. Units never reach the cap that adding
    /// saturates at: a player holds far fewer sources and floating mana.
    fn take(&mut self, kinds: u8, n: u16) {
        let k = &mut self.by_kinds[usize::from(kinds)];
        debug_assert!(*k >= n && *k < u16::MAX, "units taken that were added");
        *k -= n;
        if *k == 0 {
            self.some &= !(1 << kinds);
        }
    }

    /// The sets of kinds with units, and how many each has, in order of the sets.
    fn nonempty(&self) -> impl Iterator<Item = (u8, u16)> + '_ {
        let mut some = self.some;
        std::iter::from_fn(move || {
            if some == 0 {
                return None;
            }
            let kinds = some.trailing_zeros() as u8;
            some &= some - 1;
            Some((kinds, self.by_kinds[usize::from(kinds)]))
        })
    }

    fn total(&self) -> u32 {
        self.nonempty().map(|(_, n)| u32::from(n)).sum()
    }

    /// Whether these units can pay `cost` (X already announced).
    ///
    /// Colored and colorless symbols need units of their kind
    /// ([CR 107.4a](https://yawgatog.com/resources/magic-rules/#R1074a)): "Colored mana in
    /// costs can be paid only with the appropriate color of mana."; generic symbols take
    /// any. That is a bipartite matching, and by Hall's theorem it exists exactly when every
    /// set of symbol kinds has at least as many units able to pay one of them as it has
    /// symbols, and the units cover the total. A source that makes several mana of one kind
    /// it chooses counts as several independent units, which the cube's colorless rocks are.
    pub(crate) fn pays(&self, cost: ManaCost) -> bool {
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        if self.total() < cost.total(0) {
            return false;
        }
        let present = (0..KINDS)
            .filter(|&k| cost.symbols[k] > 0)
            .fold(0u8, |m, k| m | (1 << k));
        let mut mask = present;
        while mask != 0 {
            let need: u32 = (0..KINDS)
                .filter(|&k| mask & (1 << k) != 0)
                .map(|k| u32::from(cost.symbols[k]))
                .sum();
            let cover: u32 = self
                .nonempty()
                .filter(|&(kinds, _)| kinds & mask != 0)
                .map(|(_, n)| u32::from(n))
                .sum();
            if cover < need {
                return false;
            }
            mask = (mask - 1) & present;
        }
        true
    }

    /// The units grouped by their set of kinds, the form [`Units::pays`] checks.
    fn groups(&self) -> Groups {
        let mut g = Groups {
            total: self.total(),
            kinds: [0; 1 << KINDS],
            units: [0; 1 << KINDS],
            len: 0,
        };
        for (kinds, n) in self.nonempty() {
            g.kinds[g.len] = kinds;
            g.units[g.len] = n;
            g.len += 1;
        }
        g
    }
}

/// Units grouped by their set of kinds, with their total: what [`Units::pays`] reads, kept by
/// [`Funds`] for the many checks of one decision.
#[derive(Clone, Copy, Debug)]
struct Groups {
    total: u32,
    /// The first `len` sets of kinds with units, and how many units each has.
    kinds: [u8; 1 << KINDS],
    units: [u16; 1 << KINDS],
    len: usize,
}

impl Groups {
    /// [`Units::pays`].
    fn pays(&self, cost: ManaCost) -> bool {
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        if self.total < cost.total(0) {
            return false;
        }
        // Only sets of kinds the cost has symbols of can fail the condition: adding a kind
        // with no symbols adds no need.
        let present = (0..KINDS)
            .filter(|&k| cost.symbols[k] > 0)
            .fold(0u8, |m, k| m | (1 << k));
        let groups = || self.kinds[..self.len].iter().zip(&self.units[..self.len]);
        let mut mask = present;
        while mask != 0 {
            let need: u32 = (0..KINDS)
                .filter(|&k| mask & (1 << k) != 0)
                .map(|k| u32::from(cost.symbols[k]))
                .sum();
            let cover: u32 = groups()
                .filter(|&(&kinds, _)| kinds & mask != 0)
                .map(|(_, &n)| u32::from(n))
                .sum();
            if cover < need {
                return false;
            }
            mask = (mask - 1) & present;
        }
        true
    }
}

/// Interchangeable sources: the same mana, and nothing else to do with them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Class {
    /// The kinds of mana each makes, a bit per [`ManaColor`].
    kinds: u8,
    amount: u8,
    /// The flexibility rank (§10): fewer kinds first, then sources with no other ability,
    /// and sources a plan consumes last. Lower ranks are tapped first.
    rank: u8,
    members: Bitboard,
    /// Whether tapping one for mana also sacrifices it: a plan's taps of this class are the
    /// permanents it consumes (§10).
    consumed: bool,
}

impl Class {
    fn count(&self) -> u8 {
        self.members.count() as u8
    }
}

/// A player's untapped sources, by class in rank order.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Sources {
    classes: Fixed<Class, CLASSES>,
}

/// Whether an ability is one the solver taps for: a mana ability whose whole cost is {T}.
fn is_simple(a: &Ability) -> bool {
    a.kind == AbilityKind::Mana && a.cost == Cost::TAP
}

/// Whether `player` may activate a mana ability now: one restricted to "Activate only if you
/// control" a kind of permanent needs one on the battlefield
/// ([CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)): "A player can't begin to
/// activate an ability that's prohibited from being activated."
fn allowed<L: Layout>(s: &State<L>, t: &GameTable, a: &Ability, player: Seat) -> bool {
    a.only_if_you_control.is_none_or(|f| {
        s.battlefield()
            .iter()
            .any(|&slot| s.controller(slot) == player && f.matches(s, t, slot, player))
    })
}

/// Whether an ability is a mana ability whose whole cost is {T} and sacrificing its source,
/// as a Treasure's ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)):
/// "{T}, Sacrifice this token: Add one mana of any color." Spending the permanent is a
/// resource a plan consumes (§10): the solver taps such a source only when the cost needs it,
/// or when sacrificing it keeps more later costs payable.
fn is_sacrificed(a: &Ability) -> bool {
    a.kind == AbilityKind::Mana
        && a.cost
            == Cost {
                sacrifice_self: true,
                ..Cost::TAP
            }
}

/// The rank of every class a plan consumes, above any other source's: kinds count for at
/// most five colors and {C}, two ranks each.
const CONSUMED_RANK: u8 = 2 * KINDS as u8 + 2;

/// What a face makes as a source, found once per game ([`GameTable::facts`]) for the
/// many faces [`source`] reads: the cube's permanents are mostly not sources, or sources whose
/// mana depends on nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManaFace {
    /// No mana ability the solver taps for: never a source.
    Not,
    /// Mana abilities the solver taps for, none limited to a kind of permanent you control:
    /// the kinds they make, how much, and whether the face has another ability.
    Plain { kinds: u8, amount: u8, other: bool },
    /// Anything else, which [`source`] reads from the face.
    Read,
}

impl ManaFace {
    pub(crate) fn of(face: &Face) -> ManaFace {
        let simple = || face.abilities.iter().filter(|a| is_simple(a));
        if !face
            .abilities
            .iter()
            .any(|a| is_simple(a) || is_sacrificed(a))
        {
            return ManaFace::Not;
        }
        if simple().next().is_none() || simple().any(|a| a.only_if_you_control.is_some()) {
            return ManaFace::Read;
        }
        let kinds = simple().fold(0, |k, a| k | a.produces.colors);
        let amount = simple().next_back().map_or(0, |a| a.produces.amount);
        if kinds == 0 {
            return ManaFace::Read;
        }
        ManaFace::Plain {
            kinds,
            amount,
            other: has_other_ability(face),
        }
    }
}

/// Whether a face has an ability other than those the solver taps for, which tapping it for
/// mana would cost. Abilities that work only from a hand, such as cycling, don't count.
fn has_other_ability(face: &Face) -> bool {
    face.abilities.iter().any(|a| {
        matches!(a.kind, AbilityKind::Activated | AbilityKind::Mana)
            && !is_simple(a)
            && !a.cost.discard_self
    })
}

/// The permanents `player` controls, in battlefield order: read from their derived
/// characteristics, which are at their places in the battlefield list.
fn controlled<L: Layout>(
    s: &State<L>,
    player: Seat,
) -> impl Iterator<Item = (Slot, &Derived)> + '_ {
    debug_assert!(!s.flags.derived_dirty, "derived read while dirty");
    s.battlefield
        .iter()
        .zip(s.derived.iter())
        .filter(move |(_, d)| d.controller == player)
        .map(|(&slot, d)| (slot, d))
}

/// What an untapped source `player` controls makes, if the permanent in `slot`, whose derived
/// characteristics are `d`, is one: the kinds of mana, how much, whether it has another
/// ability, and whether making it sacrifices the source.
#[inline]
fn source<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: Slot,
    d: &Derived,
) -> Option<(u8, u8, bool, bool)> {
    let o = s.object(slot);
    // Looked up only past the table's facts, which most permanents stop at.
    let face = || t.def(o.card).face(o.face);
    let mana = t.facts(o.card, o.face).mana;
    // The table's facts stand in for reading the face: they must say what reading it would.
    debug_assert!(
        mana != ManaFace::Not || read_source(s, t, player, face()).is_none(),
        "a face the table calls no source makes mana"
    );
    if mana == ManaFace::Not || d.controller != player || o.flags.has(ObjectFlags::TAPPED) {
        return None;
    }
    // CR 302.6: a creature's {T} abilities need it under its controller's control since their
    // most recent turn began.
    if d.types.intersects(Types::CREATURE) && o.flags.has(ObjectFlags::SUMMONING_SICK) {
        return None;
    }
    if let ManaFace::Plain {
        kinds,
        amount,
        other,
    } = mana
    {
        let plain = Some((kinds, amount, other, false));
        debug_assert_eq!(
            plain,
            read_source(s, t, player, face()),
            "a plain source's mana, as the table has it"
        );
        return plain;
    }
    read_source(s, t, player, face())
}

/// What [`source`] finds, read from the face of an untapped source `player` controls that may
/// tap for mana now.
fn read_source<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
) -> Option<(u8, u8, bool, bool)> {
    let mut kinds = 0;
    let mut amount = 0;
    // A source whose mana depends on other permanents, as Floodfarm Verge's {U}, is a class
    // of its own: tapping it rather than another making the same mana now can leave
    // different mana for later.
    let mut conditional = false;
    for a in face.abilities.iter().filter(|a| is_simple(a)) {
        conditional |= a.only_if_you_control.is_some();
        if !allowed(s, t, a, player) {
            continue;
        }
        kinds |= a.produces.colors;
        debug_assert!(amount == 0 || amount == a.produces.amount);
        amount = a.produces.amount;
    }
    if kinds == 0 {
        // A Treasure: its one mana ability sacrifices it.
        let a = face
            .abilities
            .iter()
            .find(|a| is_sacrificed(a) && allowed(s, t, a, player))?;
        debug_assert!(
            a.produces.amount == 1 || a.produces.colors.is_power_of_two(),
            "a source of several mana makes one kind"
        );
        return Some((a.produces.colors, a.produces.amount, false, true));
    }
    debug_assert!(
        amount == 1 || kinds.is_power_of_two(),
        "a source of several mana makes one kind"
    );
    Some((kinds, amount, conditional || has_other_ability(face), false))
}

impl Sources {
    /// The untapped sources `player` controls, but those in `exclude`.
    pub(crate) fn of<L: Layout>(
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        exclude: Bitboard,
    ) -> Result<Self, Full> {
        let mut classes: Fixed<Class, CLASSES> = Fixed::new();
        for (slot, d) in controlled(s, player) {
            if exclude.contains(slot) {
                continue;
            }
            let Some((kinds, amount, other, consumed)) = source(s, t, player, slot, d) else {
                continue;
            };
            if consumed {
                debug_assert!(!other);
                let class = Class {
                    kinds,
                    amount,
                    rank: CONSUMED_RANK,
                    members: Bitboard::single(slot),
                    consumed: true,
                };
                // Only permanents of one card are interchangeable when spending one: the
                // payment sacrifices whichever comes first.
                let same = |c: &Class| {
                    c.consumed && c.members.iter().next().map(|m| s.card(m)) == Some(s.card(slot))
                };
                if let Some(i) = classes.iter().position(same) {
                    classes.as_mut_slice()[i].members.insert(slot);
                } else {
                    classes.push(class)?;
                }
                continue;
            }
            let rank = (kinds.count_ones() as u8) * 2 + u8::from(other);
            // A source with another ability is a class of its own: tapping it for mana
            // costs that ability, so it is never interchangeable.
            if let Some(i) = classes.iter().position(|c| {
                !other && !c.consumed && c.kinds == kinds && c.amount == amount && c.rank == rank
            }) {
                classes.as_mut_slice()[i].members.insert(slot);
            } else {
                let mut members = Bitboard::EMPTY;
                members.insert(slot);
                classes.push(Class {
                    kinds,
                    amount,
                    rank,
                    members,
                    consumed: false,
                })?;
            }
        }
        classes
            .as_mut_slice()
            .sort_by_key(|c| (c.rank, c.members.iter().next()));
        Ok(Sources { classes })
    }

    fn classes(&self) -> &[Class] {
        self.classes.as_slice()
    }

    /// The units these sources and a pool make together, less the sources a plan taps.
    fn units(&self, pool: [u8; KINDS], tapped: &[u8; CLASSES]) -> Units {
        let mut u = Units::NONE;
        for (k, &n) in pool.iter().enumerate() {
            u.add(1 << k, u16::from(n));
        }
        for (c, &taps) in self.classes().iter().zip(tapped) {
            u.add(c.kinds, u16::from(c.amount) * u16::from(c.count() - taps));
        }
        u
    }

    /// How many sources a plan sacrifices.
    fn sacrificed(&self, plan: &Plan) -> u8 {
        self.classes()
            .iter()
            .zip(&plan.taps)
            .filter(|(c, _)| c.consumed)
            .map(|(_, &n)| n)
            .sum()
    }

    /// Whether the source in `slot` is tapped by a plan.
    fn taps_slot(&self, tapped: &[u8; CLASSES], slot: Slot) -> bool {
        self.classes()
            .iter()
            .zip(tapped)
            .any(|(c, &taps)| c.members.contains(slot) && taps > 0)
    }
}

/// Everything `player` could pay mana with now: their pool and untapped sources, but those
/// in `exclude`.
pub(crate) fn available<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard,
) -> Result<Units, Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    Ok(sources.units(s.player(player).mana.amounts(), &[0; CLASSES]))
}

/// Everything a player could pay mana with now, found once for the many payability checks of
/// one decision: their pool and untapped sources, as units. What a source makes doesn't
/// depend on which others a check excludes, so [`Funds::pays`] answers as [`payable`] does,
/// taking an excluded source's units back out of the whole.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Funds {
    player: Seat,
    /// The sources' slots.
    slots: Bitboard,
    /// What the pool and all the sources make.
    units: Units,
    /// The same, grouped, for the checks that exclude none.
    all: Groups,
}

/// The most sources [`Funds`] holds, as many as a payment's [`Sources`] can.
const MAX_SOURCES: usize = 128;

impl Funds {
    pub(crate) fn of<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Result<Funds, Full> {
        let mut units = Units::NONE;
        for (k, &n) in s.player(player).mana.amounts().iter().enumerate() {
            units.add(1 << k, u16::from(n));
        }
        let mut slots = Bitboard::EMPTY;
        let mut count = 0;
        // The classes [`Sources::of`] would form, so that more than it holds is `Full` here
        // too.
        let mut classes: Fixed<(u8, u8, bool, Option<CardIx>), CLASSES> = Fixed::new();
        for (slot, d) in controlled(s, player) {
            if let Some((kinds, amount, other, consumed)) = source(s, t, player, slot, d) {
                count += 1;
                if count > MAX_SOURCES {
                    return Err(Full);
                }
                slots.insert(slot);
                units.add(kinds, u16::from(amount));
                // Sources a plan consumes are a class per card.
                let class = (kinds, amount, other, consumed.then(|| s.card(slot)));
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

    /// Whether the player can pay `cost` with none of `exclude`, as [`payable`]. `s` is the
    /// state the funds were found in.
    pub(crate) fn pays<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard,
    ) -> bool {
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        let excluded = exclude & self.slots;
        if excluded.is_empty() {
            return self.all.pays(cost);
        }
        let mut u = self.units;
        for slot in excluded.iter() {
            let (kinds, amount, _, _) =
                source(s, t, self.player, slot, s.derived(slot)).expect("a source");
            u.take(kinds, u16::from(amount));
        }
        u.pays(cost)
    }

    /// The untapped sources but those in `exclude`.
    pub(crate) fn sources(&self, exclude: Bitboard) -> Bitboard {
        self.slots & !exclude
    }
}

/// The payability prefilter: whether `player` can pay `cost` at all.
pub(crate) fn payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard,
) -> Result<bool, Full> {
    Ok(available(s, t, player, exclude)?.pays(cost))
}

/// One way to pay a cost: the floating mana spent by kind, how many sources of each class
/// are tapped, and the mana left floating afterwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Plan {
    spend: [u8; KINDS],
    taps: [u8; CLASSES],
    pub(crate) left: [u8; KINDS],
}

/// Calls `f` with every plan that pays `cost` exactly with no source tapped in vain, in
/// greedy order: the most floating mana first, then the least flexible sources. `f`
/// returns false to stop.
fn each_plan(
    cost: ManaCost,
    pool: [u8; KINDS],
    sources: &Sources,
    f: &mut dyn FnMut(&Plan) -> bool,
) {
    let total = cost.total(0);
    let mut plan = Plan::default();
    walk(cost, total, pool, sources, 0, 0, &mut plan, f);
}

/// The plan search: positions `0..KINDS` choose floating mana to spend, the rest how many
/// sources of each class to tap. `must` counts the units that must be spent so far.
#[allow(clippy::too_many_arguments)]
fn walk(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources,
    position: usize,
    must: u32,
    plan: &mut Plan,
    f: &mut dyn FnMut(&Plan) -> bool,
) -> bool {
    let classes = sources.classes();
    if position == KINDS + classes.len() {
        return match complete(cost, total, pool, sources, plan) {
            Some(p) if !wasteful(cost, total, pool, sources, plan) => f(&p),
            _ => true,
        };
    }
    if position < KINDS {
        for n in (0..=pool[position]).rev() {
            if must + u32::from(n) > total {
                continue;
            }
            plan.spend[position] = n;
            if !walk(
                cost,
                total,
                pool,
                sources,
                position + 1,
                must + u32::from(n),
                plan,
                f,
            ) {
                return false;
            }
        }
        plan.spend[position] = 0;
        return true;
    }
    let i = position - KINDS;
    let class = classes[i];
    for n in (0..=class.count()).rev() {
        // Every tapped source spends at least one of its mana.
        if must + u32::from(n) > total {
            continue;
        }
        plan.taps[i] = n;
        let must = must + if class.amount == 1 { u32::from(n) } else { 0 };
        if !walk(cost, total, pool, sources, position + 1, must, plan, f) {
            return false;
        }
    }
    plan.taps[i] = 0;
    true
}

/// How many units each tapped class spends under `plan`: all of a one-mana source's, and of
/// the sources that make several, `need` spread over them, at least one each.
fn spent_by_class(sources: &Sources, plan: &Plan, need: u32) -> [u32; CLASSES] {
    let mut spent = [0; CLASSES];
    let mut rest = need;
    for (i, c) in sources.classes().iter().enumerate() {
        let taps = u32::from(plan.taps[i]);
        if c.amount == 1 {
            spent[i] = taps;
        } else if taps > 0 {
            spent[i] = taps;
            rest -= taps;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        if c.amount > 1 && plan.taps[i] > 0 {
            let room = u32::from(plan.taps[i]) * u32::from(c.amount - 1);
            let more = room.min(rest);
            spent[i] += more;
            rest -= more;
        }
    }
    spent
}

/// Checks a plan's tap counts and floating spend: the result when they pay `cost` exactly.
fn complete(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources,
    plan: &Plan,
) -> Option<Plan> {
    let classes = sources.classes();
    let mut must: u32 = plan.spend.iter().map(|&n| u32::from(n)).sum();
    let mut several_taps = 0;
    let mut several_make = 0;
    for (i, c) in classes.iter().enumerate() {
        let taps = u32::from(plan.taps[i]);
        if c.amount == 1 {
            must += taps;
        } else {
            several_taps += taps;
            several_make += taps * u32::from(c.amount);
        }
    }
    let need = total.checked_sub(must)?;
    if several_taps > need || need > several_make || (several_taps == 0 && need > 0) {
        return None;
    }
    let spent = spent_by_class(sources, plan, need);
    let mut units = Units::NONE;
    for (k, &n) in plan.spend.iter().enumerate() {
        units.add(1 << k, u16::from(n));
    }
    for (c, &n) in classes.iter().zip(&spent) {
        units.add(c.kinds, n as u16);
    }
    if !units.pays(cost) {
        return None;
    }
    let mut left = pool;
    for (l, &spent) in left.iter_mut().zip(&plan.spend) {
        *l -= spent;
    }
    for (i, c) in classes.iter().enumerate() {
        let made = u32::from(plan.taps[i]) * u32::from(c.amount);
        if made > spent[i] {
            left[lowest_kind(c.kinds)] += (made - spent[i]) as u8;
        }
    }
    Some(Plan { left, ..*plan })
}

/// Whether a paying plan taps a source it could leave untapped, spending the same floating
/// mana: three Plains and a Mana Crypt for {3}{W}, when the Crypt and two Plains pay it. The
/// extra source only turns into floating mana, which tapping it later would make as well, so
/// the plan is dominated (§10) and never offered or taken. The cube has no {C} symbol in a
/// cost, so colorless mana left floating is never worth more than the source's own mana.
fn wasteful(cost: ManaCost, total: u32, pool: [u8; KINDS], sources: &Sources, plan: &Plan) -> bool {
    (0..sources.classes().len()).any(|i| {
        plan.taps[i] > 0 && {
            let mut fewer = *plan;
            fewer.taps[i] -= 1;
            complete(cost, total, pool, sources, &fewer).is_some()
        }
    })
}

/// Whether plan `p` taps sources only to leave mana floating that they would make as well
/// left untapped, as plan `q` leaves them: `q` taps some of what `p` taps, sacrifices no more,
/// and leaves less floating, by mana the sources `p` taps beyond it could make. A player who
/// floated {R} from a Mountain pays a Lightning Bolt with it, not with their other Mountain,
/// so `p` is dominated (§10) and never offered or taken: mana floats only until the step
/// ends, and an untapped source makes it whenever its controller next has priority.
///
/// - [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "Each player's mana pool
///   empties at the end of each step and phase".
/// - [CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may activate
///   an activated mana ability whenever they have priority".
fn floats_for_nothing(sources: &Sources, p: &Plan, q: &Plan) -> bool {
    if p == q
        || q.taps.iter().zip(&p.taps).any(|(a, b)| a > b)
        || q.left.iter().zip(&p.left).any(|(a, b)| a > b)
        || sources.sacrificed(q) > sources.sacrificed(p)
    {
        return false;
    }
    let mut extra = Units::NONE;
    for (i, c) in sources.classes().iter().enumerate() {
        extra.add(
            c.kinds,
            u16::from(c.amount) * u16::from(p.taps[i] - q.taps[i]),
        );
    }
    let mut more = [0; KINDS];
    for (m, (a, b)) in more.iter_mut().zip(p.left.iter().zip(&q.left)) {
        *m = a - b;
    }
    extra.pays(ManaCost::new(0, more))
}

fn lowest_kind(kinds: u8) -> usize {
    kinds.trailing_zeros() as usize
}

/// A cost the player might want to pay later: a card's mana cost, or an ability's
/// activation cost and the permanent it taps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Later {
    cost: ManaCost,
    /// The permanent whose {T} the cost includes, or none.
    taps: Option<Slot>,
}

/// The costs a payment should keep payable (§10): the mana costs of the player's cards in
/// hand and the activation costs of their permanents' abilities, each once.
fn later_costs<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Fixed<Later, 32> {
    let mut later: Fixed<Later, 32> = Fixed::new();
    let mut add = |l: Later| {
        if !later.iter().any(|&x| x == l) {
            // Past capacity, the rest are not considered: a heuristic, not a rule.
            let _ = later.push(l);
        }
    };
    for slot in s.zone(player, Zone::Hand).iter() {
        let def = t.def(s.object(slot).card);
        for face in def.faces {
            if !face.types.intersects(Types::LAND) {
                add(Later {
                    cost: face.cost.with_x(0),
                    taps: None,
                });
            }
            for a in face.abilities {
                if a.kind == AbilityKind::Activated && a.cost.discard_self {
                    add(Later {
                        cost: a.cost.mana.with_x(0),
                        taps: None,
                    });
                }
            }
        }
    }
    for &slot in s.battlefield() {
        let o = s.object(slot);
        if s.controller(slot) != player || o.flags.has(ObjectFlags::TAPPED) {
            continue;
        }
        for a in t.def(o.card).face(o.face).abilities {
            if a.kind == AbilityKind::Activated && !a.is_loyalty() {
                add(Later {
                    cost: a.cost.mana.with_x(0),
                    taps: a.cost.tap.then_some(slot),
                });
            }
        }
    }
    later
}

/// What `plan` keeps payable of `later`, best first: the most colored and colorless symbols
/// of any one payable cost, then how many costs stay payable, then their symbols in all. An
/// auto-tapper keeps colors open: {U}{U} for Mana Drain rather than {1}{W} for Get Lost and
/// {W} for Oust, of which the two lands left would cast only one anyway. A cost hungry for
/// colors is the one a careless payment strands; a generic one is paid from whatever is left.
fn still_payable(sources: &Sources, plan: &Plan, later: &[Later]) -> (u32, usize, u32) {
    let units = sources.units(plan.left, &plan.taps);
    let payable = later.iter().filter(|l| match l.taps {
        None => units.pays(l.cost),
        Some(slot) => {
            if sources.taps_slot(&plan.taps, slot) {
                return false;
            }
            // The permanent taps for the ability, so it cannot also make mana for it.
            let mut without = *plan;
            for (i, c) in sources.classes().iter().enumerate() {
                if c.members.contains(slot) {
                    without.taps[i] += 1;
                }
            }
            sources.units(plan.left, &without.taps).pays(l.cost)
        }
    });
    payable.fold((0, 0, 0), |(most, n, all), l| {
        let symbols: u32 = l.cost.symbols.iter().map(|&k| u32::from(k)).sum();
        (most.max(symbols), n + 1, all + symbols)
    })
}

/// The plans a payment of `cost` offers, at most one per distinct floating remainder and
/// number of sources sacrificed, in order of the fewest sacrificed, then the least mana left
/// floating. Sacrificing a source is a plan's own outcome (§10), but almost always a waste:
/// a plan that sacrifices more is offered only when it keeps strictly more later costs payable
/// than every plan sacrificing fewer. Under [`crate::profile::Payment::Greedy`] it is the
/// first plan in greedy order of those sacrificing the fewest, alone.
pub(crate) fn plans<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard,
    greedy: bool,
) -> Result<Fixed<Plan, 64>, Full> {
    sink.work.solver_runs += 1;
    let sources = Sources::of(s, t, player, exclude)?;
    let pool = s.player(player).mana.amounts();
    let mut out: Fixed<Plan, 64> = Fixed::new();
    if greedy {
        let mut best: Option<(u8, Plan)> = None;
        each_plan(cost, pool, &sources, &mut |p| {
            let sacrificed = sources.sacrificed(p);
            if best.is_none_or(|(fewest, _)| sacrificed < fewest) {
                best = Some((sacrificed, *p));
            }
            sacrificed > 0
        });
        if let Some((_, p)) = best {
            out.push(p)?;
        }
        return Ok(out);
    }
    // Scores decide only between plans of one remainder and number sacrificed, and whether a
    // plan sacrificing more is dominated, so each is found when one of those first needs it:
    // a payment with one way to leave each remainder scores none.
    let mut later: Option<Fixed<Later, 32>> = None;
    let mut score = |p: &Plan| {
        let later = later.get_or_insert_with(|| later_costs(s, t, player));
        still_payable(&sources, p, later.as_slice())
    };
    let mut scores: Fixed<Option<(u32, usize, u32)>, 64> = Fixed::new();
    let mut full = false;
    each_plan(cost, pool, &sources, &mut |p| {
        let sacrificed = sources.sacrificed(p);
        match out
            .iter()
            .position(|q| q.left == p.left && sources.sacrificed(q) == sacrificed)
        {
            // The first plan of a remainder taps the least flexible sources; a later one
            // replaces it only by keeping more costs payable.
            Some(i) => {
                let q = out.as_slice()[i];
                let first = *scores.as_mut_slice()[i].get_or_insert_with(|| score(&q));
                let this = score(p);
                if this > first {
                    out.as_mut_slice()[i] = *p;
                    scores.as_mut_slice()[i] = Some(this);
                }
            }
            None => {
                full |= out.push(*p).is_err() || scores.push(None).is_err();
            }
        }
        !full
    });
    if full {
        return Err(Full);
    }
    // A plan is dominated only by one sacrificing fewer, so with one number sacrificed among
    // them all no score is needed.
    let fewest = out.iter().map(|p| sources.sacrificed(p)).min();
    if out.iter().any(|p| Some(sources.sacrificed(p)) != fewest) {
        for (p, sc) in out.iter().zip(scores.as_mut_slice()) {
            sc.get_or_insert_with(|| score(p));
        }
    }
    let mut kept: Fixed<Plan, 64> = Fixed::new();
    for (p, &sc) in out.iter().zip(scores.iter()) {
        let sacrificed = sources.sacrificed(p);
        let dominated = out.iter().zip(scores.iter()).any(|(q, &other)| {
            sources.sacrificed(q) < sacrificed && other.expect("scored") >= sc.expect("scored")
        });
        if !dominated {
            kept.push(*p)?;
        }
    }
    // Only a pool can leave mana floating that a source would make again: a source's own
    // leftover mana is `wasteful`'s.
    if kept.len() > 1 && pool.iter().any(|&n| n > 0) {
        let mut dominated = 0u64;
        for (i, p) in kept.iter().enumerate() {
            if kept.iter().any(|q| floats_for_nothing(&sources, p, q)) {
                dominated |= 1 << i;
            }
        }
        let mut i = 0;
        kept.retain(|_| {
            i += 1;
            dominated & (1 << (i - 1)) == 0
        });
    }
    kept.as_mut_slice().sort_by_key(|p| {
        let floating: u32 = p.left.iter().map(|&n| u32::from(n)).sum();
        (sources.sacrificed(p), floating, p.left)
    });
    Ok(kept)
}

/// Pays `cost` by `plan`: taps its sources for the mana it needs, sacrificing those whose
/// ability says so, and spends it.
///
/// [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "If the total cost
/// includes a mana payment, the player then has a chance to activate mana abilities ... Mana
/// abilities must be activated before costs are paid."
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. ... Partial payments are not allowed."
pub(crate) fn pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard,
    plan: &Plan,
) -> Result<(), Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    let total = cost.total(0);
    let must: u32 = plan.spend.iter().map(|&n| u32::from(n)).sum::<u32>()
        + sources
            .classes()
            .iter()
            .zip(&plan.taps)
            .filter(|(c, _)| c.amount == 1)
            .map(|(_, &n)| u32::from(n))
            .sum::<u32>();
    let spent = spent_by_class(&sources, plan, total - must);
    // The units spent: (kinds, source slot or none for floating mana, how many).
    let mut units: Fixed<(u8, Option<Slot>, u8), 64> = Fixed::new();
    for (k, &n) in plan.spend.iter().enumerate() {
        for _ in 0..n {
            units.push((1 << k, None, 1))?;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        let tapped: Fixed<Slot, 64> = {
            let mut v = Fixed::new();
            for slot in c.members.iter().take(usize::from(plan.taps[i])) {
                v.push(slot)?;
            }
            v
        };
        if c.amount == 1 {
            for &slot in &tapped {
                units.push((c.kinds, Some(slot), 1))?;
            }
        } else {
            // Spread the spent units as `spent_by_class` counted them.
            let mut rest = spent[i];
            for (j, &slot) in tapped.iter().enumerate() {
                let later = (tapped.len() - j - 1) as u32;
                let here = (rest - later).min(u32::from(c.amount));
                units.push((c.kinds, Some(slot), here as u8))?;
                rest -= here;
            }
        }
    }
    let kinds = assign(cost, units.as_slice());
    for (&(unit_kinds, source, n), &kind) in units.iter().zip(kinds.iter()) {
        let kind = kind.unwrap_or_else(|| lowest_kind(unit_kinds));
        if let Some(slot) = source {
            activate(s, t, sink, slot, ManaColor::ALL[kind])?;
        }
        let pool = &mut s.player_mut(player).mana[ManaColor::ALL[kind]];
        debug_assert!(*pool >= n);
        *pool -= n;
    }
    debug_assert_eq!(s.player(player).mana.amounts(), plan.left);
    Ok(())
}

/// One augmenting-path step of [`assign`]: finds a unit slot for symbol `sym`.
fn augment(
    sym: usize,
    symbols: &[usize],
    slots: &[usize],
    units: &[(u8, Option<Slot>, u8)],
    owner: &mut [Option<usize>],
    seen: &mut [bool; 64],
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
/// paying `n` symbols of one kind: the kind each unit is spent as, or `None` where it pays
/// only generic mana. Kuhn's augmenting paths over the few symbols a cost has.
fn assign(cost: ManaCost, units: &[(u8, Option<Slot>, u8)]) -> Fixed<Option<usize>, 64> {
    let mut symbols: Fixed<usize, 64> = Fixed::new();
    for (k, &n) in cost.symbols.iter().enumerate() {
        for _ in 0..n {
            let _ = symbols.push(k);
        }
    }
    // A unit of several mana pays as many symbols, all of one kind: split it into slots.
    let mut slots: Fixed<usize, 64> = Fixed::new();
    for (u, &(_, _, n)) in units.iter().enumerate() {
        for _ in 0..n {
            let _ = slots.push(u);
        }
    }
    let mut owner: Fixed<Option<usize>, 64> = Fixed::new();
    for _ in 0..slots.len() {
        let _ = owner.push(None);
    }
    for sym in 0..symbols.len() {
        let mut seen = [false; 64];
        let matched = augment(
            sym,
            symbols.as_slice(),
            slots.as_slice(),
            units,
            owner.as_mut_slice(),
            &mut seen,
        );
        debug_assert!(matched, "a plan the solver accepted pays every symbol");
    }
    let mut kinds: Fixed<Option<usize>, 64> = Fixed::new();
    for u in 0..units.len() {
        let kind = slots
            .iter()
            .zip(owner.iter())
            .find(|&(&slot_unit, o)| slot_unit == u && o.is_some())
            .map(|(_, o)| symbols.as_slice()[o.unwrap()]);
        let _ = kinds.push(kind);
    }
    kinds
}

/// Activates a source's mana ability for one kind of mana: taps it and adds the mana.
///
/// [CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b): "An activated mana
/// ability doesn't go on the stack, so it can't be targeted, countered, or otherwise
/// responded to. Rather, it resolves immediately after it is activated."
pub(crate) fn activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink,
    slot: Slot,
    color: ManaColor,
) -> Result<(), Full> {
    let o = *s.object(slot);
    let face = t.def(o.card).face(o.face);
    let (index, ability) = face
        .abilities
        .iter()
        .enumerate()
        .find(|(_, a)| {
            (is_simple(a) || is_sacrificed(a))
                && a.produces.includes(color)
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
    if ability.cost.sacrifice_self {
        event::move_one(s, t, sink, slot, Zone::Graveyard, event::Reason::Sacrifice)?;
    }
    let activation = PendingTrigger {
        source: Some(source),
        controller,
        ability: index as u8,
        card: o.card,
        face: o.face,
        ..PendingTrigger::default()
    };
    let mut ctx = EffectCtx::new(s, t, sink, &activation, ability);
    ctx.mana = color;
    crate::coverage::hit(ability);
    // A mana ability never asks a question.
    let _ = L::body(ability)(&mut ctx);
    ctx.finish()
}

/// Pays one unit toward `remaining`: a colored or colorless symbol of its kind if one is
/// left, generic mana otherwise. Paying a symbol of the unit's own kind first is never worse.
fn pay_unit(remaining: &mut ManaCost, kind: usize) {
    if remaining.symbols[kind] > 0 {
        remaining.symbols[kind] -= 1;
    } else {
        debug_assert!(remaining.generic > 0);
        remaining.generic -= 1;
    }
}

/// Whether a unit of `kind` can pay part of `remaining`.
fn helps(remaining: ManaCost, kind: usize) -> bool {
    remaining.symbols[kind] > 0 || remaining.generic > 0
}

/// Manual payment (the `manual_mana` flag): the floating mana and the sources, one per
/// class, whose next unit pays toward `remaining` and leaves the rest payable.
pub(crate) fn manual_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    remaining: ManaCost,
    exclude: Bitboard,
) -> Result<Fixed<Opt, 64>, Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    let pool = s.player(player).mana.amounts();
    let mut out = Fixed::new();
    for kind in 0..KINDS {
        if pool[kind] == 0 || !helps(remaining, kind) {
            continue;
        }
        let mut rest = remaining;
        pay_unit(&mut rest, kind);
        let mut after = pool;
        after[kind] -= 1;
        if sources.units(after, &[0; CLASSES]).pays(rest) {
            out.push(Opt::Pool(ManaColor::ALL[kind]))?;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        let slot = c.members.iter().next().expect("a class has a member");
        let mut taps = [0; CLASSES];
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

/// Performs a manual payment option, returning what remains to pay.
pub(crate) fn manual_pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    mut remaining: ManaCost,
    option: Opt,
) -> Result<ManaCost, Full> {
    let color = match option {
        Opt::Pool(color) => color,
        Opt::Mana { slot, color } => {
            activate(s, t, sink, slot, color)?;
            color
        }
        _ => unreachable!("a manual payment option"),
    };
    s.player_mut(player).mana[color] -= 1;
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
    out: &mut Fixed<Opt, 64>,
) {
    let sources: Bitboard = controlled(s, player)
        .filter(|&(slot, d)| source(s, t, player, slot, d).is_some())
        .map(|(slot, _)| slot)
        .collect();
    for slot in crate::engine::collapse(s, sources).iter() {
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
