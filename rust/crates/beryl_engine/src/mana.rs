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
use crate::card::{
    Ability, AbilityKind, Face, Keywords, ManaColor, ManaCost, Replacement, Timing, Types,
};
use crate::decision::Opt;
use crate::event::{self, Sink};
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::{Derived, ObjectFlags, PendingTrigger, State, TurnStep, Zone, ZoneKey};
use crate::table::{GameTable, ManaFace, has_other_ability, is_sacrificed, is_simple};
use crate::{filter::Evaluate, fixed::Full};

mod units;

use units::Groups;
pub(crate) use units::Units;

/// The kinds of mana, W U B R G and C, as [`ManaColor`] indices.
const KINDS: usize = 6;
/// Classes of interchangeable sources one solve considers in [`Small`]; [`crate::Large`] holds
/// any number.
const CLASSES: usize = 16;

/// How many sources of each class a plan taps, an entry per class: in a layout that holds more
/// classes than [`Small`], an entry per slot, as every class has a member.
type Taps<L> = <L as Layout>::Table<u8, CLASSES>;

/// Interchangeable sources: the same mana, and nothing else to do with them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Class<L: Layout = Small> {
    /// The kinds of mana each makes, a bit per [`ManaColor`].
    kinds: u8,
    amount: u8,
    /// The flexibility rank (§10): fewer kinds first, then sources with no other ability,
    /// and sources a plan consumes last. Lower ranks are tapped first.
    rank: u8,
    members: Bitboard<L::Slot>,
    /// Whether tapping one for mana also sacrifices it: a plan's taps of this class are the
    /// permanents it consumes (§10).
    consumed: bool,
    /// The least card among the members, which orders classes of one rank.
    least: crate::query::DefinitionKey,
}

impl<L: Layout> Class<L> {
    fn count(&self) -> u8 {
        self.members.count() as u8
    }

    /// Whether another source fits: a plan counts a class's taps in a `u8`, so in a layout of
    /// more slots than that, sources past 255 interchangeable ones form another class.
    fn has_room(&self) -> bool {
        u8::try_from(L::Slot::COUNT).is_ok() || self.members.count() < u32::from(u8::MAX)
    }
}

/// A player's untapped sources, by class in rank order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sources<L: Layout = Small> {
    classes: L::Queue<Class<L>, CLASSES>,
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

/// The rank of every class a plan consumes, above any other source's: kinds count for at
/// most five colors and {C}, two ranks each.
const CONSUMED_RANK: u8 = 2 * KINDS as u8 + 2;

/// The permanents `player` controls, in battlefield order: read from their derived
/// characteristics, which are at their places in the battlefield list.
fn controlled<L: Layout>(
    s: &State<L>,
    player: Seat,
) -> impl Iterator<Item = (L::Slot, &Derived)> + '_ {
    s.permanents().filter(move |(_, d)| d.controller == player)
}

/// What an untapped source `player` controls makes, if the permanent in `slot`, whose derived
/// characteristics are `d`, is one: the kinds of mana, how much, whether it has another
/// ability, and whether making it sacrifices the source.
#[inline]
fn source<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    d: &Derived,
) -> Option<(u8, u8, bool, bool)> {
    let o = s.object(slot);
    // Looked up only past the table's facts, which most permanents stop at.
    let face = || s.copiable(slot).face(t);
    let mana = s.copiable(slot).facts(t).mana;
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

/// Amount and whether the source is consumed by its mana activation, for payment previews.
#[must_use]
pub fn mana_contribution<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
) -> Option<(u8, bool)> {
    source(s, t, player, slot, s.derived(slot)).map(|(_, amount, _, consumed)| (amount, consumed))
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

impl<L: Layout> Sources<L> {
    /// The untapped sources `player` controls, but those in `exclude`.
    pub(crate) fn of(
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        exclude: Bitboard<L::Slot>,
    ) -> Result<Self, Full> {
        let mut classes: L::Queue<Class<L>, CLASSES> = Default::default();
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
                    least: s.copiable(slot).key(),
                };
                // Only permanents of one card are interchangeable when spending one: the
                // payment sacrifices whichever comes first.
                let same =
                    |c: &Class<L>| c.consumed && c.has_room() && c.least == s.copiable(slot).key();
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
                !other
                    && !c.consumed
                    && c.kinds == kinds
                    && c.amount == amount
                    && c.rank == rank
                    && c.has_room()
            }) {
                let c = &mut classes.as_mut_slice()[i];
                c.members.insert(slot);
                c.least = c.least.min(s.copiable(slot).key());
            } else {
                let mut members = Bitboard::EMPTY;
                members.insert(slot);
                classes.push(Class {
                    kinds,
                    amount,
                    rank,
                    members,
                    consumed: false,
                    least: s.copiable(slot).key(),
                })?;
            }
        }
        // Classes of one rank by card, never by slot, which the game's randomness chose as it
        // dealt the cards to their slots: by the least card of each, as a class may hold
        // several, such as Islands and a modal double-faced card's land face.
        classes.as_mut_slice().sort_by_key(|c| (c.rank, c.least));
        Ok(Sources { classes })
    }

    fn classes(&self) -> &[Class<L>] {
        self.classes.as_slice()
    }

    /// The units these sources and a pool make together, less the sources a plan taps.
    fn units(&self, pool: [u8; KINDS], tapped: &Taps<L>) -> Units {
        let mut u = Units::NONE;
        for (k, &n) in pool.iter().enumerate() {
            u.add(1 << k, u16::from(n));
        }
        for (c, &taps) in self.classes().iter().zip(tapped.iter()) {
            u.add(c.kinds, u16::from(c.amount) * u16::from(c.count() - taps));
        }
        u
    }

    /// How many sources a plan sacrifices.
    fn sacrificed(&self, plan: &Plan<L>) -> u8 {
        self.classes()
            .iter()
            .zip(plan.taps.iter())
            .filter(|(c, _)| c.consumed)
            .map(|(_, &n)| n)
            .sum()
    }

    /// Whether the source in `slot` is tapped by a plan.
    fn taps_slot(&self, tapped: &Taps<L>, slot: L::Slot) -> bool {
        self.classes()
            .iter()
            .zip(tapped.iter())
            .any(|(c, &taps)| c.members.contains(slot) && taps > 0)
    }
}

/// Everything `player` could pay mana with now: their pool and untapped sources, but those
/// in `exclude`.
pub(crate) fn available<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard<L::Slot>,
) -> Result<Units, Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    Ok(sources.units(s.player(player).mana.amounts(), &Table::filled(0)))
}

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
        for (k, &n) in s.player(player).mana.amounts().iter().enumerate() {
            units.add(1 << k, u16::from(n));
        }
        let mut slots = Bitboard::EMPTY;
        // The classes [`Sources::of`] would form, so that more than it holds is `Full` here
        // too.
        let mut classes: L::Queue<_, CLASSES> = Default::default();
        for (slot, d) in controlled(s, player) {
            if let Some((kinds, amount, other, consumed)) = source(s, t, player, slot, d) {
                slots.insert(slot);
                units.add(kinds, u16::from(amount));
                // Sources a plan consumes are a class per card.
                let card = consumed.then(|| s.copiable(slot).key());
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

    /// Whether the player can pay `cost` with none of `exclude`, as [`payable`]. `s` is the
    /// state the funds were found in.
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
        let mut u = self.units;
        // Each source's units come off its own count, so the order is nothing.
        for slot in s.in_candidate_order(excluded) {
            let (kinds, amount, _, _) =
                source(s, t, self.player, slot, s.derived(slot)).expect("a source");
            u.take(kinds, u16::from(amount));
        }
        u.pays(cost)
    }

    /// The untapped sources but those in `exclude`.
    pub(crate) fn sources(&self, exclude: Bitboard<L::Slot>) -> Bitboard<L::Slot> {
        self.slots & !exclude
    }
}

/// The payability prefilter: whether `player` can pay `cost` at all.
pub(crate) fn payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
) -> Result<bool, Full> {
    Ok(available(s, t, player, exclude)?.pays(cost))
}

/// One way to pay a cost: the floating mana spent by kind, how many sources of each class
/// are tapped, and the mana left floating afterwards.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Plan<L: Layout = Small> {
    spend: [u8; KINDS],
    taps: Taps<L>,
    pub(crate) left: [u8; KINDS],
}

impl<L: Layout> Default for Plan<L> {
    fn default() -> Self {
        Plan {
            spend: [0; KINDS],
            taps: Table::filled(0),
            left: [0; KINDS],
        }
    }
}

/// Calls `f` with every plan that pays `cost` exactly with no source tapped in vain, in
/// greedy order: the most floating mana first, then the least flexible sources. `f`
/// returns false to stop.
fn each_plan<L: Layout>(
    cost: ManaCost,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    f: &mut dyn FnMut(&Plan<L>) -> bool,
) {
    let total = cost.total(0);
    let mut plan = Plan::default();
    walk(cost, total, pool, sources, 0, 0, &mut plan, f);
}

/// The plan search: positions `0..KINDS` choose floating mana to spend, the rest how many
/// sources of each class to tap. `must` counts the units that must be spent so far.
#[allow(clippy::too_many_arguments)]
fn walk<L: Layout>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    position: usize,
    must: u32,
    plan: &mut Plan<L>,
    f: &mut dyn FnMut(&Plan<L>) -> bool,
) -> bool {
    let classes = sources.classes();
    if position == KINDS + classes.len() {
        return match complete(cost, total, pool, sources, plan) {
            Some(left) if !wasteful(cost, total, pool, sources, plan) => {
                plan.left = left;
                f(plan)
            }
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
fn spent_by_class<L: Layout>(
    sources: &Sources<L>,
    plan: &Plan<L>,
    need: u32,
) -> L::Table<u32, CLASSES> {
    let mut spent: L::Table<u32, CLASSES> = Table::filled(0);
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

/// Checks a plan's tap counts and floating spend: the mana it leaves floating when they pay
/// `cost` exactly.
fn complete<L: Layout>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    plan: &Plan<L>,
) -> Option<[u8; KINDS]> {
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
    for (c, &n) in classes.iter().zip(spent.iter()) {
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
    Some(left)
}

/// Whether a paying plan taps a source it could leave untapped, spending the same floating
/// mana: three Plains and a Mana Crypt for {3}{W}, when the Crypt and two Plains pay it. The
/// extra source only turns into floating mana, which tapping it later would make as well, so
/// the plan is dominated (§10) and never offered or taken. The cube has no {C} symbol in a
/// cost, so colorless mana left floating is never worth more than the source's own mana.
fn wasteful<L: Layout>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    plan: &mut Plan<L>,
) -> bool {
    (0..sources.classes().len()).any(|i| {
        plan.taps[i] > 0 && {
            plan.taps[i] -= 1;
            let fewer = complete(cost, total, pool, sources, plan).is_some();
            plan.taps[i] += 1;
            fewer
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
fn floats_for_nothing<L: Layout>(sources: &Sources<L>, p: &Plan<L>, q: &Plan<L>) -> bool {
    if p == q
        || q.taps.iter().zip(p.taps.iter()).any(|(a, b)| a > b)
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Later<L: Layout = Small> {
    cost: ManaCost,
    /// The permanent whose {T} the cost includes, or none.
    taps: Option<L::Slot>,
}

/// The costs a payment should keep payable (§10): the mana costs of the player's cards in
/// hand and the activation costs of their permanents' abilities, each once.
fn later_costs<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> L::Queue<Later<L>, 32> {
    let mut later: L::Queue<Later<L>, 32> = Default::default();
    let mut add = |l: Later<L>| {
        if !later.iter().any(|&x| x == l) {
            // Past a fixed layout's capacity, the rest are not considered: a heuristic, not a
            // rule.
            let _ = later.push(l);
        }
    };
    // Each cost is kept once, and `still_payable` reads the list as a set: no order shows.
    let hand = s.in_candidate_order(s.zone(ZoneKey::Of(player, Zone::Hand)));
    for def in hand.filter_map(|slot| s.card_def(t, slot)) {
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
        for a in s.copiable(slot).face(t).abilities {
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
fn still_payable<L: Layout>(
    sources: &Sources<L>,
    plan: &Plan<L>,
    later: &[Later<L>],
) -> (u32, usize, u32) {
    let units = sources.units(plan.left, &plan.taps);
    let payable = later.iter().filter(|l| match l.taps {
        None => units.pays(l.cost),
        Some(slot) => {
            if sources.taps_slot(&plan.taps, slot) {
                return false;
            }
            // The permanent taps for the ability, so it cannot also make mana for it.
            let mut without = plan.taps.clone();
            for (i, c) in sources.classes().iter().enumerate() {
                if c.members.contains(slot) {
                    without[i] += 1;
                }
            }
            sources.units(plan.left, &without).pays(l.cost)
        }
    });
    payable.fold((0, 0, 0), |(most, n, all), l| {
        let symbols: u32 = l.cost.symbols.iter().map(|&k| u32::from(k)).sum();
        (most.max(symbols), n + 1, all + symbols)
    })
}

/// What the rest of the turn may ask of the mana a payment leaves (§10): the costs the player
/// might still pay before their sources untap, each once with how many of the player's cards
/// or abilities have it (two Lightning Bolts can both be cast), and the mana of each land they
/// might still play untapped, each distinct one once.
#[derive(Clone, Debug, Default)]
struct Ahead<L: Layout = Small> {
    later: L::Queue<Later<L>, 32>,
    copies: L::Queue<u8, 32>,
    /// A land's kinds of mana and how much it makes.
    lands: L::Queue<(u8, u8), 8>,
}

impl<L: Layout> Ahead<L> {
    fn add(&mut self, cost: ManaCost, taps: Option<L::Slot>) {
        let l = Later {
            cost: cost.with_x(0),
            taps,
        };
        if let Some(i) = self.later.iter().position(|&x| x == l) {
            let n = &mut self.copies.as_mut_slice()[i];
            *n = n.saturating_add(1);
        } else if self.later.push(l).is_ok() {
            let _ = self.copies.push(1);
        }
        // Past a fixed layout's capacity, the rest are not considered: a heuristic, not a rule.
    }
}

/// Whether `player` may still act at sorcery speed before their sources next untap: it is
/// their turn, short of its end step, so a main phase may still come with the stack empty.
///
/// - [CR 307.1](https://yawgatog.com/resources/magic-rules/#R3071): "A player who has priority
///   may cast a sorcery card from their hand during a main phase of their turn when the stack
///   is empty."
/// - [CR 502.3](https://yawgatog.com/resources/magic-rules/#R5023): "the active player
///   determines which permanents they control will untap. Then they untap them all
///   simultaneously."
fn sorcery_window<L: Layout>(s: &State<L>, player: Seat) -> bool {
    s.turn().active() == player && s.turn().step() <= TurnStep::PostcombatMain
}

/// Whether `player` may cast `face` at instant speed, as casting decides: an instant
/// ([CR 117.1a](https://yawgatog.com/resources/magic-rules/#R1171a)), a face with flash
/// ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)), or a sorcery while an
/// effect lets them cast sorceries as though they had flash. Kept apart from casting's own
/// check, which the hot priority path inlines.
fn instant_speed<L: Layout>(s: &State<L>, face: &Face, player: Seat) -> bool {
    face.types.intersects(Types::INSTANT)
        || face.keywords.contains(Keywords::FLASH)
        || (face.types.intersects(Types::SORCERY) && s.player(player).sorceries_as_flash)
}

/// Whether a land `player` plays now enters untapped, by its face's replacement abilities, as
/// [`event`] decides as it enters
/// ([CR 614.1c](https://yawgatog.com/resources/magic-rules/#R6141c)): "Effects that read
/// '[This permanent] enters with . . . ,' 'As [this permanent] enters . . . ,' or '[This
/// permanent] enters as . . . ' are replacement effects." One that asks for life enters
/// untapped if the player pays, which is theirs to choose.
fn enters_untapped<L: Layout>(s: &State<L>, player: Seat, face: &Face) -> bool {
    face.abilities
        .iter()
        .filter(|a| a.kind == AbilityKind::Replacement)
        .all(|a| match a.replacement {
            Replacement::EntersTapped => false,
            Replacement::EntersTappedUnlessFewLands(n) => {
                // As [`event`] counts them: every land is another land.
                s.battlefield()
                    .iter()
                    .filter(|&&p| {
                        let d = s.derived(p);
                        d.controller == player && d.types.contains(Types::LAND)
                    })
                    .count()
                    <= usize::from(n)
            }
            _ => true,
        })
}

/// What the rest of the turn may ask of `player`'s mana (§10): the mana costs of the cards in
/// their hand and those they may cast under a permission, and the activation costs of their
/// permanents' abilities, those that need sorcery speed only while [`sorcery_window`] allows;
/// and the lands in their hand, while they may still play one this turn.
///
/// [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can normally play
/// one land during their turn".
fn ahead<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Ahead<L> {
    let mut ahead: Ahead<L> = Ahead::default();
    let sorcery = sorcery_window(s, player);
    let land_drop = sorcery && s.player(player).lands_played < 1;
    // A transforming card's back face is never cast: "A double-faced spell is cast with its
    // front face up by default" ([CR 712.11](https://yawgatog.com/resources/magic-rules/#R71211)).
    // A land is played, not cast ([CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051)).
    let castable = |face: &Face| {
        !face.transformed
            && !face.types.intersects(Types::LAND)
            && (sorcery || instant_speed(s, face, player))
    };
    let activatable = |a: &Ability| sorcery || a.timing == Timing::Instant;
    // The hand by card, not by slot: [`Lookahead::together`]'s search is bounded, so the order
    // it meets the costs in can decide, and the slots are where the game's randomness dealt
    // the cards.
    let mut hand: L::Queue<L::Slot, 64> = Default::default();
    for slot in s.in_candidate_order(s.zone(ZoneKey::Of(player, Zone::Hand))) {
        let _ = hand.push(slot);
    }
    hand.as_mut_slice()
        .sort_by_key(|&slot| s.copiable(slot).key());
    for card in hand.iter().filter_map(|&slot| s.card_behind(slot)) {
        let def = t.def(card);
        for f in def.face_ixs() {
            let face = def.face(f);
            if castable(face) {
                ahead.add(face.cost, None);
            }
            if land_drop
                && !face.transformed
                && face.types.intersects(Types::LAND)
                && enters_untapped(s, player, face)
                && let ManaFace::Plain { kinds, amount, .. } = t.facts(card, f).mana
                && !ahead.lands.iter().any(|&l| l == (kinds, amount))
            {
                let _ = ahead.lands.push((kinds, amount));
            }
            for a in face.abilities {
                if a.kind == AbilityKind::Activated && a.cost.discard_self && activatable(a) {
                    ahead.add(a.cost.mana, None);
                }
            }
        }
    }
    let mine = |p: &&crate::state::CastPermission<L>| p.player == player && s.is_current(p.object);
    for p in s.permissions().iter().filter(mine) {
        // Only a card is cast (CR 601.2a).
        let faces = s.card_def(t, p.object.slot).map_or_default(|def| def.faces);
        for face in faces {
            if castable(face) {
                let cost = face.cost;
                ahead.add(
                    ManaCost {
                        generic: cost.generic.saturating_add(p.extra),
                        ..cost
                    },
                    None,
                );
            }
        }
    }
    for &slot in s.battlefield() {
        let o = s.object(slot);
        if s.controller(slot) != player || o.flags.has(ObjectFlags::TAPPED) {
            continue;
        }
        for a in s.copiable(slot).face(t).abilities {
            if a.kind == AbilityKind::Activated && !a.is_loyalty() && activatable(a) {
                ahead.add(a.cost.mana, a.cost.tap.then_some(slot));
            }
        }
    }
    ahead
}

/// What a plan leaves the rest of the turn, best first (§10). First, the most mana that later
/// costs payable together spend, then their colored and colorless symbols: an auto-tapper
/// keeps a Counterspell and a Brainstorm castable together rather than either alone, and
/// {U}{U} for Mana Drain rather than {1}{W} for Get Lost and {W} for Oust, of which the two
/// lands left would cast only one anyway. Then, of the costs payable each alone, the most
/// symbols of any one, how many, and their symbols in all: a cost hungry for colors is the
/// one a careless payment strands; a generic one is paid from whatever is left. Last, how
/// many creatures stay untapped, free to attack or block
/// ([CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a): "The chosen creatures
/// must be untapped"), so a dual taps before a mana creature when nothing else tells them
/// apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Outlook {
    together: u32,
    together_symbols: u32,
    most: u32,
    payable: u32,
    all: u32,
    creatures: u32,
}

/// How many payability checks [`Lookahead::together`] makes at most for one plan: a heuristic
/// bound on a search that grows with the subsets of the hand.
const TOGETHER_CHECKS: u32 = 128;

/// The search for what a plan leaves, over its leftover mana and the costs ahead.
struct Lookahead<'a, L: Layout> {
    sources: &'a Sources<L>,
    plan: &'a Plan<L>,
    later: &'a [Later<L>],
    copies: &'a [u8],
}

impl<L: Layout> Lookahead<'_, L> {
    /// `units` less the mana of the permanent `l` taps for its cost, or none if the plan taps
    /// it: a permanent tapping for an ability cannot also make mana for it.
    fn without(&self, units: Units, l: &Later<L>) -> Option<Units> {
        let Some(slot) = l.taps else {
            return Some(units);
        };
        if self.sources.taps_slot(&self.plan.taps, slot) {
            return None;
        }
        let mut u = units;
        if let Some(c) = self
            .sources
            .classes()
            .iter()
            .find(|c| c.members.contains(slot))
        {
            u.take(c.kinds, u16::from(c.amount));
        }
        Some(u)
    }

    /// The outlook with `units` to pay with.
    fn outlook(&self, units: Units) -> Outlook {
        let mut out = Outlook::default();
        for l in self.later {
            if self.without(units, l).is_some_and(|u| u.pays(l.cost)) {
                let symbols = symbols(l.cost);
                out.most = out.most.max(symbols);
                out.payable += 1;
                out.all += symbols;
            }
        }
        let mut used: L::Queue<u8, 32> = Default::default();
        for _ in self.later {
            let _ = used.push(0);
        }
        let mut best = (0, 0);
        let mut checks = TOGETHER_CHECKS;
        self.together(
            0,
            ManaCost::NONE,
            units,
            Bitboard::EMPTY,
            used.as_mut_slice(),
            &mut best,
            &mut checks,
        );
        (out.together, out.together_symbols) = best;
        out
    }

    /// Extends `sum`, costs payable together from `units`, by those from `start` on, keeping
    /// in `best` the most mana and then symbols of any such set. `tapping` holds the
    /// permanents the set's abilities tap, `used` how many of each cost's copies it pays.
    #[allow(clippy::too_many_arguments)]
    fn together(
        &self,
        start: usize,
        sum: ManaCost,
        units: Units,
        tapping: Bitboard<L::Slot>,
        used: &mut [u8],
        best: &mut (u32, u32),
        checks: &mut u32,
    ) {
        *best = (*best).max((sum.total(0), symbols(sum)));
        for (i, l) in self.later.iter().enumerate().skip(start) {
            // A cost of no mana spends none, and a permanent taps once.
            if l.cost.total(0) == 0
                || used[i] >= self.copies[i]
                || l.taps.is_some_and(|slot| tapping.contains(slot))
            {
                continue;
            }
            let Some(u) = self.without(units, l) else {
                continue;
            };
            let mut next = sum;
            next.generic = next.generic.saturating_add(l.cost.generic);
            for (n, &k) in next.symbols.iter_mut().zip(&l.cost.symbols) {
                *n = n.saturating_add(k);
            }
            if next.total(0) > u.total() {
                continue;
            }
            if *checks == 0 {
                return;
            }
            *checks -= 1;
            if !u.pays(next) {
                continue;
            }
            let mut tapping = tapping;
            if let Some(slot) = l.taps {
                tapping.insert(slot);
            }
            used[i] += 1;
            self.together(i, next, u, tapping, used, best, checks);
            used[i] -= 1;
        }
    }
}

/// A cost's colored and colorless symbols.
fn symbols(cost: ManaCost) -> u32 {
    cost.symbols.iter().map(|&k| u32::from(k)).sum()
}

/// What `plan` leaves the rest of the turn ([`Outlook`]): with its leftover mana, and with
/// that of the best land the player may still play.
fn outlook<L: Layout>(
    sources: &Sources<L>,
    plan: &Plan<L>,
    ahead: &Ahead<L>,
    creatures: &Taps<L>,
) -> Outlook {
    let look = Lookahead {
        sources,
        plan,
        later: ahead.later.as_slice(),
        copies: ahead.copies.as_slice(),
    };
    let units = sources.units(plan.left, &plan.taps);
    let mut best = look.outlook(units);
    for &(kinds, amount) in ahead.lands.iter() {
        let mut u = units;
        u.add(kinds, u16::from(amount));
        best = best.max(look.outlook(u));
    }
    // [`pay`] taps a class's other members before its creatures.
    best.creatures = sources
        .classes()
        .iter()
        .zip(plan.taps.iter().zip(creatures.iter()))
        .map(|(c, (&taps, &n))| u32::from(n.min(c.count() - taps)))
        .sum();
    best
}

/// The plans a payment of `cost` offers, at most one per distinct floating remainder and
/// number of sources sacrificed, in order of the fewest sacrificed, then the least mana left
/// floating. Sacrificing a source is a plan's own outcome (§10), but almost always a waste:
/// a plan that sacrifices more is offered only when it keeps strictly more later costs payable
/// than every plan sacrificing fewer. Under [`crate::profile::Payment::Greedy`] it is the
/// first plan in greedy order of those sacrificing the fewest, alone.
///
/// The human's payment ([`crate::Profile::taps_mana`]) offers the same plans, each tapping
/// the sources [`looking_ahead`] picks.
pub(crate) fn plans<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    greedy: bool,
) -> Result<L::Queue<Plan<L>, 64>, Full> {
    sink.work.solver_runs += 1;
    let sources = Sources::of(s, t, player, exclude)?;
    let pool = s.player(player).mana.amounts();
    let mut out: L::Queue<Plan<L>, 64> = Default::default();
    if greedy {
        let mut best: Option<(u8, Plan<L>)> = None;
        each_plan(cost, pool, &sources, &mut |p| {
            let sacrificed = sources.sacrificed(p);
            if best.as_ref().is_none_or(|&(fewest, _)| sacrificed < fewest) {
                best = Some((sacrificed, p.clone()));
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
    let mut later: Option<L::Queue<Later<L>, 32>> = None;
    let mut score = |p: &Plan<L>| {
        let later = later.get_or_insert_with(|| later_costs(s, t, player));
        still_payable(&sources, p, later.as_slice())
    };
    let mut scores: L::Queue<Option<(u32, usize, u32)>, 64> = Default::default();
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
                let q = &out.as_slice()[i];
                let first = *scores.as_mut_slice()[i].get_or_insert_with(|| score(q));
                let this = score(p);
                if this > first {
                    out.as_mut_slice()[i] = p.clone();
                    scores.as_mut_slice()[i] = Some(this);
                }
            }
            None => {
                full |= out.push(p.clone()).is_err() || scores.push(None).is_err();
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
    let mut kept: L::Queue<Plan<L>, 64> = Default::default();
    for (p, &sc) in out.iter().zip(scores.iter()) {
        let sacrificed = sources.sacrificed(p);
        let dominated = out.iter().zip(scores.iter()).any(|(q, &other)| {
            sources.sacrificed(q) < sacrificed && other.expect("scored") >= sc.expect("scored")
        });
        if !dominated {
            kept.push(p.clone())?;
        }
    }
    // Only a pool can leave mana floating that a source would make again: a source's own
    // leftover mana is `wasteful`'s.
    if kept.len() > 1 && pool.iter().any(|&n| n > 0) {
        let all = kept.clone();
        kept.retain(|p| !all.iter().any(|q| floats_for_nothing(&sources, p, q)));
    }
    kept.as_mut_slice().sort_by_key(|p| {
        let floating: u32 = p.left.iter().map(|&n| u32::from(n)).sum();
        (sources.sacrificed(p), floating, p.left)
    });
    if s.profile().taps_mana == Some(player) {
        looking_ahead(s, t, player, cost, &sources, kept.as_mut_slice())?;
    }
    Ok(kept)
}

/// The human's payment ([`crate::Profile::taps_mana`]): each of the plans `kept` offers, as
/// another player's, replaced by the way to pay `cost` leaving the same remainder and
/// sacrificing as many sources whose [`Outlook`] over the rest of the turn is best. Which
/// plans a payment offers stays the same under either profile, so a decision offered to the
/// human reads the same to a search that clears it; only which sources each taps changes.
#[cold]
#[inline(never)]
fn looking_ahead<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    sources: &Sources<L>,
    kept: &mut [Plan<L>],
) -> Result<(), Full> {
    let ahead = ahead(s, t, player);
    let mut creatures: Taps<L> = Table::filled(0);
    for (i, c) in sources.classes().iter().enumerate() {
        creatures[i] = c
            .members
            .filter(|m| s.derived(m).types.intersects(Types::CREATURE))
            .count() as u8;
    }
    let mut best: L::Queue<Outlook, 64> = Default::default();
    for k in kept.iter() {
        best.push(outlook(sources, k, &ahead, &creatures))?;
    }
    each_plan(cost, s.player(player).mana.amounts(), sources, &mut |p| {
        let sacrificed = sources.sacrificed(p);
        if let Some(i) = kept
            .iter()
            .position(|k| k.left == p.left && sources.sacrificed(k) == sacrificed)
        {
            let o = outlook(sources, p, &ahead, &creatures);
            if o > best.as_slice()[i] {
                best.as_mut_slice()[i] = o;
                kept[i] = p.clone();
            }
        }
        true
    });
    Ok(())
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
                units.push((c.kinds, Some(slot), here as u8))?;
                rest -= here;
            }
        }
    }
    let kinds = assign::<L>(cost, units.as_slice());
    for (&(unit_kinds, source, n), &kind) in units.iter().zip(kinds.iter()) {
        let kind = kind.unwrap_or_else(|| lowest_kind(unit_kinds));
        if let Some(slot) = source {
            activate(s, t, sink, slot, ManaColor::ALL[kind])?;
        }
        s.spend_mana(player, ManaColor::ALL[kind], n);
    }
    debug_assert_eq!(s.player(player).mana.amounts(), plan.left);
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
/// paying `n` symbols of one kind: the kind each unit is spent as, or `None` where it pays
/// only generic mana. Kuhn's augmenting paths over the few symbols a cost has.
fn assign<L: Layout>(
    cost: ManaCost,
    units: &[(u8, Option<L::Slot>, u8)],
) -> L::Queue<Option<usize>, 64> {
    let mut symbols: L::Queue<usize, 64> = Default::default();
    for (k, &n) in cost.symbols.iter().enumerate() {
        for _ in 0..n {
            let _ = symbols.push(k);
        }
    }
    // A unit of several mana pays as many symbols, all of one kind: split it into slots.
    let mut slots: L::Queue<usize, 64> = Default::default();
    for (u, &(_, _, n)) in units.iter().enumerate() {
        for _ in 0..n {
            let _ = slots.push(u);
        }
    }
    let mut owner: L::Queue<Option<usize>, 64> = Default::default();
    let mut seen: L::Queue<bool, 64> = Default::default();
    for _ in 0..slots.len() {
        let _ = owner.push(None);
        let _ = seen.push(false);
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
    let mut kinds: L::Queue<Option<usize>, 64> = Default::default();
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
    exclude: Bitboard<L::Slot>,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    let pool = s.player(player).mana.amounts();
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

/// Performs a manual payment option, returning what remains to pay.
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
            activate(s, t, sink, slot, color)?;
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
/// "they then have a chance to activate mana abilities".
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
