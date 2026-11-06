//! Mana: mana abilities, the payability prefilter and the payment solver (architecture §10,
//! decision D7).
//!
//! [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "When an effect instructs a
//! player to add mana, that mana goes into a player's mana pool. From there, it can be used to
//! pay costs immediately, or it can stay in the player's mana pool as unspent mana."
//!
//! The solver works over **sources**: permanents whose mana abilities cost only {T} and do
//! nothing but add mana, the lands and rocks of the cube. Sources that make the same mana and
//! have no other ability are interchangeable, so they form a **class**, and a plan says how
//! many of each class it taps and how much floating mana of each kind it spends. Plans that
//! leave the same mana floating differ only in which sources they tap: the solver picks one.
//! Plans that leave different mana floating are a decision.

use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityKind, Cost, ManaColor, ManaCost, ManaSet, Types};
use crate::decision::Opt;
use crate::effect::EffectCtx;
use crate::event::{self, Sink};
use crate::fixed::{Fixed, Full};
use crate::ids::{Seat, Slot};
use crate::state::{ObjectFlags, PendingTrigger, State, Zone};
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
        self.groups().pays(cost)
    }

    /// The units grouped by their set of kinds, the form [`Units::pays`] checks.
    fn groups(&self) -> Groups {
        let mut g = Groups {
            total: self.total(),
            by_kinds: [(0, 0); 1 << KINDS],
            len: 0,
        };
        for (kinds, n) in self.nonempty() {
            g.by_kinds[g.len] = (kinds, u32::from(n));
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
    by_kinds: [(u8, u32); 1 << KINDS],
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
        let groups = &self.by_kinds[..self.len];
        let mut mask = present;
        while mask != 0 {
            let need: u32 = (0..KINDS)
                .filter(|&k| mask & (1 << k) != 0)
                .map(|k| u32::from(cost.symbols[k]))
                .sum();
            let cover: u32 = groups
                .iter()
                .filter(|&&(kinds, _)| kinds & mask != 0)
                .map(|&(_, n)| n)
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
    /// The flexibility rank (§10): fewer kinds first, then sources with no other ability.
    /// Lower ranks are tapped first.
    rank: u8,
    members: Bitboard,
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
fn allowed(s: &State, t: &GameTable, a: &Ability, player: Seat) -> bool {
    a.only_if_you_control.is_none_or(|f| {
        s.battlefield()
            .iter()
            .any(|&slot| s.controller(slot) == player && f.matches(s, t, slot, player))
    })
}

/// Whether an ability is a mana ability whose whole cost is {T} and sacrificing its source,
/// as a Treasure's ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)):
/// "{T}, Sacrifice this token: Add one mana of any color." Spending the permanent is a
/// resource a plan consumes (§10), so the solver never does it on its own: the player
/// activates it, at priority or while paying.
fn is_sacrificed(a: &Ability) -> bool {
    a.kind == AbilityKind::Mana
        && a.cost
            == Cost {
                sacrifice_self: true,
                ..Cost::TAP
            }
}

/// The untapped permanents `player` controls, but those in `exclude`, that a mana ability
/// sacrifices, one of each interchangeable group (§7.2), with the mana that ability makes.
fn sacrificed_sources(
    s: &State,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard,
) -> Result<Fixed<(Slot, ManaSet), 64>, Full> {
    let candidates: Bitboard = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&slot| {
            let o = s.object(slot);
            // CR 302.6, as for the solver's sources.
            let sick = s.derived(slot).types.intersects(Types::CREATURE)
                && o.flags.has(ObjectFlags::SUMMONING_SICK);
            // Checked before collapsing, which compares the card and face this reads.
            let sacrifices = || {
                t.def(o.card)
                    .face(o.face)
                    .abilities
                    .iter()
                    .any(is_sacrificed)
            };
            s.controller(slot) == player
                && !o.flags.has(ObjectFlags::TAPPED)
                && !exclude.contains(slot)
                && !sick
                && sacrifices()
        })
        .collect();
    let mut out = Fixed::new();
    for slot in crate::engine::collapse(s, candidates).iter() {
        let o = s.object(slot);
        let face = t.def(o.card).face(o.face);
        if let Some(a) = face.abilities.iter().find(|a| is_sacrificed(a)) {
            out.push((slot, a.produces))?;
        }
    }
    Ok(out)
}

/// What an untapped source `player` controls makes, if the permanent in `slot` is one: the
/// kinds of mana, how much, and whether it has another ability.
fn source(s: &State, t: &GameTable, player: Seat, slot: Slot) -> Option<(u8, u8, bool)> {
    let o = s.object(slot);
    if s.controller(slot) != player || o.flags.has(ObjectFlags::TAPPED) {
        return None;
    }
    let face = t.def(o.card).face(o.face);
    // CR 302.6: a creature's {T} abilities need it under its controller's control since their
    // most recent turn began.
    if s.derived(slot).types.intersects(Types::CREATURE) && o.flags.has(ObjectFlags::SUMMONING_SICK)
    {
        return None;
    }
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
        return None;
    }
    debug_assert!(
        amount == 1 || kinds.is_power_of_two(),
        "a source of several mana makes one kind"
    );
    // Abilities that work only from a hand, such as cycling, don't count.
    let other = conditional
        || face.abilities.iter().any(|a| {
            matches!(a.kind, AbilityKind::Activated | AbilityKind::Mana)
                && !is_simple(a)
                && !a.cost.discard_self
        });
    Some((kinds, amount, other))
}

impl Sources {
    /// The untapped sources `player` controls, but those in `exclude`.
    pub(crate) fn of(
        s: &State,
        t: &GameTable,
        player: Seat,
        exclude: Bitboard,
    ) -> Result<Self, Full> {
        let mut classes: Fixed<Class, CLASSES> = Fixed::new();
        for &slot in s.battlefield() {
            if exclude.contains(slot) {
                continue;
            }
            let Some((kinds, amount, other)) = source(s, t, player, slot) else {
                continue;
            };
            let rank = (kinds.count_ones() as u8) * 2 + u8::from(other);
            // A source with another ability is a class of its own: tapping it for mana
            // costs that ability, so it is never interchangeable.
            if let Some(i) = classes
                .iter()
                .position(|c| !other && c.kinds == kinds && c.amount == amount && c.rank == rank)
            {
                classes.as_mut_slice()[i].members.insert(slot);
            } else {
                let mut members = Bitboard::EMPTY;
                members.insert(slot);
                classes.push(Class {
                    kinds,
                    amount,
                    rank,
                    members,
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
pub(crate) fn available(
    s: &State,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard,
) -> Result<Units, Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    Ok(sources.units(s.player(player).mana.amounts(), &[0; CLASSES]))
}

/// Everything a player could pay mana with now, found once for the many payability checks of
/// one decision: their pool, and each untapped source with the mana it makes. What a source
/// makes doesn't depend on which others a check excludes, so [`Funds::pays`] answers as
/// [`payable`] does.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Funds {
    pool: [u8; KINDS],
    /// Each source's slot, kinds of mana and amount.
    sources: Fixed<(Slot, u8, u8), 128>,
    /// The sources' slots.
    slots: Bitboard,
    /// What all of them and the pool can pay, for the checks that exclude none.
    all: Groups,
}

impl Funds {
    pub(crate) fn of(s: &State, t: &GameTable, player: Seat) -> Result<Funds, Full> {
        let mut sources = Fixed::new();
        // The classes [`Sources::of`] would form, so that more than it holds is `Full` here
        // too.
        let mut classes: Fixed<(u8, u8, bool), CLASSES> = Fixed::new();
        for &slot in s.battlefield() {
            if let Some((kinds, amount, other)) = source(s, t, player, slot) {
                sources.push((slot, kinds, amount))?;
                if other || !classes.iter().any(|&c| c == (kinds, amount, false)) {
                    classes.push((kinds, amount, other))?;
                }
            }
        }
        let mut funds = Funds {
            pool: s.player(player).mana.amounts(),
            sources,
            slots: sources.iter().map(|&(slot, _, _)| slot).collect(),
            all: Units::NONE.groups(),
        };
        funds.all = funds.units(Bitboard::EMPTY).groups();
        Ok(funds)
    }

    /// The units the pool and the sources but those in `exclude` make, as [`available`].
    fn units(&self, exclude: Bitboard) -> Units {
        let mut u = Units::NONE;
        for (k, &n) in self.pool.iter().enumerate() {
            u.add(1 << k, u16::from(n));
        }
        for &(slot, kinds, amount) in &self.sources {
            if !exclude.contains(slot) {
                u.add(kinds, u16::from(amount));
            }
        }
        u
    }

    /// Whether the player can pay `cost` with none of `exclude`, as [`payable`].
    pub(crate) fn pays(&self, cost: ManaCost, exclude: Bitboard) -> bool {
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        if (exclude & self.slots).is_empty() {
            self.all.pays(cost)
        } else {
            self.units(exclude).pays(cost)
        }
    }

    /// The untapped sources but those in `exclude`.
    pub(crate) fn sources(&self, exclude: Bitboard) -> Bitboard {
        self.sources
            .iter()
            .map(|&(slot, _, _)| slot)
            .filter(|&slot| !exclude.contains(slot))
            .collect()
    }
}

/// The payability prefilter: whether `player` can pay `cost` at all.
pub(crate) fn payable(
    s: &State,
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
fn later_costs(s: &State, t: &GameTable, player: Seat) -> Fixed<Later, 32> {
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

/// The plans a payment of `cost` offers, at most one per distinct floating remainder, in
/// order of the least mana left floating. Under [`crate::profile::Payment::Greedy`] it is
/// the first plan in greedy order alone.
pub(crate) fn plans(
    s: &State,
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
        each_plan(cost, pool, &sources, &mut |p| {
            let _ = out.push(*p);
            false
        });
        return Ok(out);
    }
    let later = later_costs(s, t, player);
    let mut scores: Fixed<(u32, usize, u32), 64> = Fixed::new();
    let mut full = false;
    each_plan(cost, pool, &sources, &mut |p| {
        let score = still_payable(&sources, p, later.as_slice());
        match out.iter().position(|q| q.left == p.left) {
            // The first plan of a remainder taps the least flexible sources; a later one
            // replaces it only by keeping more costs payable.
            Some(i) => {
                if score > scores.as_slice()[i] {
                    out.as_mut_slice()[i] = *p;
                    scores.as_mut_slice()[i] = score;
                }
            }
            None => {
                full |= out.push(*p).is_err() || scores.push(score).is_err();
            }
        }
        !full
    });
    if full {
        return Err(Full);
    }
    out.as_mut_slice().sort_by_key(|p| {
        let floating: u32 = p.left.iter().map(|&n| u32::from(n)).sum();
        (floating, p.left)
    });
    Ok(out)
}

/// Pays `cost` by `plan`: taps its sources for the mana it needs and spends it.
///
/// [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "If the total cost
/// includes a mana payment, the player then has a chance to activate mana abilities ... Mana
/// abilities must be activated before costs are paid."
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost. ... Partial payments are not allowed."
pub(crate) fn pay(
    s: &mut State,
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
pub(crate) fn activate(
    s: &mut State,
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
    // A mana ability never asks a question.
    let _ = (ability.body)(&mut ctx);
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
pub(crate) fn manual_options(
    s: &State,
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
    for &(slot, produces) in &sacrificed_sources(s, t, player, exclude)? {
        for kind in (0..KINDS).filter(|&k| produces.colors & (1 << k) != 0) {
            if !helps(remaining, kind) {
                continue;
            }
            let mut rest = remaining;
            pay_unit(&mut rest, kind);
            let mut after = pool;
            after[kind] += produces.amount - 1;
            if sources.units(after, &[0; CLASSES]).pays(rest) {
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
pub(crate) fn manual_pay(
    s: &mut State,
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

/// The activations of mana abilities that sacrifice their source, offered at priority under
/// any payment profile: one for each kind of mana each can make
/// ([CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a)): "A player may activate
/// an activated mana ability whenever they have priority". The solver never spends them
/// (§10), so mana a payment needs from one floats first.
pub(crate) fn sacrifice_options(
    s: &State,
    t: &GameTable,
    player: Seat,
    out: &mut Fixed<Opt, 64>,
) -> Result<(), Full> {
    for &(slot, produces) in &sacrificed_sources(s, t, player, Bitboard::EMPTY)? {
        for kind in (0..KINDS).filter(|&k| produces.colors & (1 << k) != 0) {
            out.push(Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            })?;
        }
    }
    Ok(())
}

/// The standalone mana activations offered at priority under manual payment
/// ([CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a)): "A player may activate
/// an activated mana ability whenever they have priority". Otherwise tapping ahead of a
/// payment is dominated by tapping during it (§10), so nothing is offered.
pub(crate) fn standalone_options(
    s: &State,
    t: &GameTable,
    player: Seat,
    out: &mut Fixed<Opt, 64>,
) -> Result<(), Full> {
    let sources = Sources::of(s, t, player, Bitboard::EMPTY)?;
    for c in sources.classes() {
        let slot = c.members.iter().next().expect("a class has a member");
        for kind in (0..KINDS).filter(|&k| c.kinds & (1 << k) != 0) {
            out.push(Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            })?;
        }
    }
    Ok(())
}
