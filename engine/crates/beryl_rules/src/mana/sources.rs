//! What can make mana: the sources a player's mana abilities form, in classes of
//! interchangeable permanents.
//!
//! Implements: none. Groups usable mana sources and checks their activation restrictions.

use crate::bitboard::Bitboard;
#[cfg(doc)]
use crate::card::ManaColor;
use crate::card::{Ability, AbilityIx, Face, Keywords, ObjectAbility, Types, Words};
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::{Derived, State};
use crate::table::{Capability, GameTable, ManaFace, has_other_ability, is_sacrificed, is_simple};
use crate::{filter::Evaluate, fixed::Full};

use super::remembered::{self, Paying};
use super::solver::Plan;
use super::{CLASSES, KINDS, Taps, units::Units};

/// Interchangeable sources: the same mana, and nothing else to do with them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Class<L: Layout = Small> {
    /// The kinds of mana each makes, a bit per [`ManaColor`].
    pub(super) kinds: u8,
    pub(super) amount: u8,
    /// The flexibility rank (§10): fewer kinds first, then sources with no other ability,
    /// and sources a plan consumes last. Lower ranks are tapped first.
    rank: u8,
    pub(super) members: Bitboard<L::Slot>,
    /// Whether tapping one for mana also sacrifices it: a plan's taps of this class are the
    /// permanents it consumes (§10).
    consumed: bool,
    /// The least card among the members, which orders classes of one rank.
    least: crate::query::DefinitionKey<L::Card>,
    /// Whether its members are tapped for their restricted mana ability, which the payment's
    /// restriction accepts ([`super::remembered`]): one member, a class of its own.
    pub(super) restricted: bool,
    /// Whether that mana's restriction has a rider.
    pub(super) rider: bool,
    /// Of a restricted class, the class of the same member's unrestricted mana, if it has
    /// one, and of that class, the restricted one: tapping the permanent for one leaves it
    /// tapped for the other. [`NO_TWIN`] for none: a byte, so that the class stays 40 bytes,
    /// which a plan's search copies.
    twin: u8,
}

/// [`Class::twin`] of a class with none.
const NO_TWIN: u8 = u8::MAX;

impl<L: Layout> Class<L> {
    pub(super) fn count(&self) -> u8 {
        u8::try_from(self.members.count()).expect("a class holds at most 255 sources")
    }

    /// Whether another source fits: a plan counts a class's taps in a `u8`, so in a layout of
    /// more slots than that, sources past 255 interchangeable ones form another class.
    fn has_room(&self) -> bool {
        u8::try_from(L::Slot::COUNT).is_ok() || self.members.count() < u32::from(u8::MAX)
    }

    /// Whether it is a restricted class whose member has another class: its taps are counted
    /// with that class's.
    pub(super) fn is_twin(&self) -> bool {
        self.restricted && self.twin != NO_TWIN
    }

    /// The class of the same member's other mana, if it has one ([`Class::twin`]).
    pub(super) fn twin(&self) -> Option<u8> {
        (self.twin != NO_TWIN).then_some(self.twin)
    }
}

/// A player's untapped sources, by class in rank order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sources<L: Layout = Small> {
    classes: L::Queue<Class<L>, CLASSES>,
    /// The mana that remembers in the pool that the payment's restriction accepts: each
    /// entry's place in the pool, its kind as a [`ManaColor`] index and whether its
    /// restriction has a rider. A plan
    /// spends some of them ([`Plan::remembered`]).
    remembered: L::Queue<(u8, u8, bool), 4>,
    /// Whether a class is restricted.
    restricted: bool,
    /// Whether a class is restricted or the pool holds mana that remembers the payment
    /// accepts ([`Sources::has_memory`]), found once as the sources are.
    memory: bool,
}

/// Whether `player` may activate a mana ability now: one restricted to "Activate only if you
/// control" a kind of permanent needs one on the battlefield
/// (see [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)): "A player can't begin to
/// activate an ability that's prohibited from being activated." The kind of permanent is
/// read through `words`, those of the permanent with the ability: the card table refuses a
/// granted mana ability, so it is always the permanent's own.
pub(super) fn allowed<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    a: &Ability,
    player: Seat,
    words: &Words,
) -> bool {
    a.only_if_you_control().is_none_or(|f| {
        s.battlefield()
            .iter()
            .any(|&slot| s.controller(slot) == player && f.matches(s, t, slot, player, words))
    })
}

/// The rank of every class a plan consumes, above any other source's: kinds count for at
/// most five colors and {C}, two ranks each.
const CONSUMED_RANK: u8 = 14;
// Two ranks per kind, followed by the rank reserved for consumed sources.
const _: () = assert!(2 * KINDS + 2 == 14);

/// The rank of every restricted class, the last: after the class of the same permanent's
/// unrestricted mana, which a plan's search counts first.
const RESTRICTED_RANK: u8 = CONSUMED_RANK + 1;

/// Whether `player` may tap the permanent in `slot`, whose derived characteristics are `d`,
/// for mana now: they control it, it is untapped, and a creature has been theirs since their
/// most recent turn began (see [CR 302.6]): "A creature's activated ability with the tap symbol
/// ... in its activation cost can't be activated unless the creature has been under its
/// controller's control continuously since their most recent turn began."
///
/// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
#[inline]
pub(super) fn can_tap<L: Layout>(
    s: &State<L>,
    player: Seat,
    slot: L::Slot,
    d: &Derived<L>,
) -> bool {
    let o = s.object(slot);
    d.controller == player
        && !o.is_tapped()
        && !(d.types.intersects(Types::CREATURE) && o.is_summoning_sick())
}

/// The permanents `player` controls, in battlefield order: read from their derived
/// characteristics, which are at their places in the battlefield list.
pub(super) fn controlled<L: Layout>(
    s: &State<L>,
    player: Seat,
) -> impl Iterator<Item = (L::Slot, &Derived<L>)> + '_ {
    s.permanents_with_derived()
        .filter(move |(_, d)| d.controller == player)
}

/// What an untapped source `player` controls makes, if the permanent in `slot`, whose derived
/// characteristics are `d`, is one: the kinds of mana, how much, whether it has another
/// ability, and whether making it sacrifices the source.
#[inline]
pub(super) fn source<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    d: &Derived<L>,
) -> Option<(u8, u8, bool, bool)> {
    // Looked up only past the table's facts, which most permanents stop at.
    let face = || s.definition(slot).face(t);
    let mana = s.definition(slot).facts(t).mana;
    // The table's facts stand in for reading the face, for a permanent that makes its card's
    // mana: they must say what reading it would. One that lost an ability, or whose land
    // types an effect changed, is read ability by ability (see CR 613.1f, see CR 305.7), as is a
    // merged permanent, which has the abilities of the cards under it too (see CR 702.140e).
    let card_mana = || s.makes_card_mana(slot);
    debug_assert!(
        mana != ManaFace::Not || !card_mana() || read_source(s, t, player, slot, face()).is_none(),
        "a face the table calls no source makes mana"
    );
    let o = s.object(slot);
    if (mana == ManaFace::Not && card_mana()) || d.controller != player || o.is_tapped() {
        return None;
    }
    // See CR 302.6: a creature's {T} abilities need it under its controller's control since their
    // most recent turn began.
    // see CR 702.10c: haste lifts that.
    if d.types.intersects(Types::CREATURE)
        && o.is_summoning_sick()
        && !d.keywords.contains(Keywords::HASTE)
    {
        return None;
    }
    if let ManaFace::Plain {
        kinds,
        amount,
        other,
    } = mana
        && card_mana()
    {
        let plain = Some((kinds, amount, other, false));
        debug_assert_eq!(
            plain,
            read_source(s, t, player, slot, face()),
            "a plain source's mana, as the table has it"
        );
        return plain;
    }
    read_source(s, t, player, slot, face())
}

/// Whether the permanent in `slot` is a merged permanent with the abilities of the cards
/// under it ([`ObjectAbility::Merged`]): never, after one test, in a game with no card that
/// merges.
#[inline]
fn merged<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    t.can(Capability::Merges) && s.has(slot, ObjectAbility::Merged(0, AbilityIx::new(0)))
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
/// tap for mana now, the permanent in `slot`: of the abilities it has
/// ([`State::abilities`]), with the intrinsic mana abilities of its basic land types
/// ([`State::intrinsic_mana`]), each of which makes one mana of its kind.
fn read_source<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    face: &Face,
) -> Option<(u8, u8, bool, bool)> {
    let mut kinds = s.intrinsic_mana(t, slot);
    let mut amount = u8::from(kinds != 0);
    // A source whose mana depends on other permanents, as Floodfarm Verge's {U}, is a class
    // of its own: tapping it rather than another making the same mana now can leave
    // different mana for later.
    let mut conditional = false;
    let words = s.words_of(slot);
    let mut abilities = s.abilities(t, slot).map(|(_, a)| a);
    for a in abilities.clone().filter(|a| is_simple(a)) {
        conditional |= a.only_if_you_control().is_some();
        if !allowed(s, t, a, player, &words) {
            continue;
        }
        kinds |= a.produces().colors;
        debug_assert!(amount == 0 || amount == a.produces().amount);
        amount = a.produces().amount;
    }
    if kinds == 0 {
        // A Treasure: its one mana ability sacrifices it.
        let a = abilities.find(|a| is_sacrificed(a) && allowed(s, t, a, player, &words))?;
        debug_assert!(
            a.produces().amount == 1 || a.produces().colors.is_power_of_two(),
            "a source of several mana makes one kind"
        );
        return Some((a.produces().colors, a.produces().amount, false, true));
    }
    debug_assert!(
        amount == 1 || kinds.is_power_of_two(),
        "a source of several mana makes one kind"
    );
    // A merged permanent's other cards may have other abilities: it is a class of its own.
    // It makes no card's mana (`State::makes_card_mana`), which most sources do, so that is
    // asked first.
    let other = has_other_ability(face) || (!s.makes_card_mana(slot) && merged(s, t, slot));
    Some((kinds, amount, conditional || other, false))
}

impl<L: Layout> Sources<L> {
    /// The untapped sources `player` controls, but those in `exclude`, for a payment of what
    /// `paying` is for.
    pub(crate) fn of(
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
    ) -> Result<Self, Full> {
        // Built in place: a copy of its classes costs every payment.
        let mut out = Sources::<L>::default();
        let classes = &mut out.classes;
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
                    least: s.definition(slot).key(),
                    restricted: false,
                    rider: false,
                    twin: NO_TWIN,
                };
                // Only permanents of one card are interchangeable when spending one: the
                // payment sacrifices whichever comes first.
                let same = |c: &Class<L>| {
                    c.consumed && c.has_room() && c.least == s.definition(slot).key()
                };
                if let Some(i) = classes.iter().position(same) {
                    classes.as_mut_slice()[i].members.insert(slot);
                } else {
                    classes.push(class)?;
                }
                continue;
            }
            let rank = u8::try_from(kinds.count_ones()).expect("number of bits in a byte") * 2
                + u8::from(other);
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
                c.least = c.least.min(s.definition(slot).key());
            } else {
                let mut members = Bitboard::EMPTY;
                members.insert(slot);
                classes.push(Class {
                    kinds,
                    amount,
                    rank,
                    members,
                    consumed: false,
                    least: s.definition(slot).key(),
                    restricted: false,
                    rider: false,
                    twin: NO_TWIN,
                })?;
            }
        }
        if paying.reads_restrictions(t) {
            out.restricted = restricted_classes(s, t, player, exclude, paying, &mut out.classes)?;
            out.remembered = accepted(s, t, player, paying);
            out.memory = out.restricted || !out.remembered.is_empty();
        }
        // Classes of one rank by card, never by slot, which the game's randomness chose as it
        // dealt the cards to their slots: by the least card of each, as a class may hold
        // several, such as Islands and a modal double-faced card's land face.
        out.classes
            .as_mut_slice()
            .sort_by_key(|c| (c.rank, c.least));
        if out.restricted {
            link_twins(out.classes.as_mut_slice());
        }
        Ok(out)
    }

    /// Whether a class is restricted or the pool holds mana that remembers the payment
    /// accepts: never in a game whose cards make no restricted mana.
    #[inline]
    pub(super) fn has_memory(&self) -> bool {
        self.memory
    }

    /// The mana that remembers in the pool that the payment accepts
    /// ([`Sources::remembered`]).
    pub(super) fn remembered(&self) -> &[(u8, u8, bool)] {
        self.remembered.as_slice()
    }

    /// Whether a plan spends restricted mana whose restriction has a rider: a plan that does
    /// has a different outcome from one that doesn't (see CR 106.6).
    #[inline]
    pub(super) fn rides(&self, plan: &Plan<L>) -> bool {
        if !self.memory {
            return false;
        }
        let tapped = (self.classes().iter().zip(plan.taps.iter())).any(|(c, &n)| c.rider && n > 0);
        let spent = (self.remembered().iter().enumerate())
            .any(|(i, &(_, _, rider))| rider && plan.remembered & (1 << i) != 0);
        tapped || spent
    }

    /// What a plan does but which sources it taps: the mana it leaves floating, the sources
    /// it sacrifices, the mana that remembers it spends and whether it gives the spell a
    /// rider. Plans of one outcome are one choice (§10).
    #[inline]
    pub(super) fn outcome(&self, p: &Plan<L>) -> ([u8; KINDS], u8, u8, bool) {
        (p.left, self.sacrificed(p), p.remembered, self.rides(p))
    }

    pub(super) fn classes(&self) -> &[Class<L>] {
        self.classes.as_slice()
    }

    /// The units these sources and a pool make together, less the sources a plan taps, with
    /// the mana that remembers the payment accepts.
    #[inline]
    pub(super) fn units(&self, pool: [u8; KINDS], tapped: &Taps<L>) -> Units {
        if self.has_memory() {
            return self.units_left(pool, tapped, 0);
        }
        let mut u = Units::NONE;
        for (k, &n) in pool.iter().enumerate() {
            u.add(1 << k, u16::from(n));
        }
        for (c, &taps) in self.classes().iter().zip(tapped.iter()) {
            u.add(c.kinds, u16::from(c.amount) * u16::from(c.count() - taps));
        }
        u
    }

    /// [`Sources::units`], less the mana that remembers in `spent`, a bit per entry of
    /// [`Sources::remembered`]. An untapped permanent with a restricted class and another is
    /// one unit of either's kinds.
    #[inline]
    pub(super) fn units_left(&self, pool: [u8; KINDS], tapped: &Taps<L>, spent: u8) -> Units {
        let mut u = Units::NONE;
        for (k, &n) in pool.iter().enumerate() {
            u.add(1 << k, u16::from(n));
        }
        self.restricted_units(&mut u, tapped, spent);
        u
    }

    /// [`Sources::units_left`]'s units of the classes and of the mana that remembers, where a
    /// class is restricted or the pool holds such mana.
    #[cold]
    #[inline(never)]
    fn restricted_units(&self, u: &mut Units, tapped: &Taps<L>, spent: u8) {
        for (i, &(_, kind, _)) in self.remembered().iter().enumerate() {
            if spent & (1 << i) == 0 {
                u.add(1 << kind, 1);
            }
        }
        let classes = self.classes();
        for (c, &taps) in classes.iter().zip(tapped.iter()) {
            if c.is_twin() {
                continue;
            }
            let (mut kinds, mut left) = (c.kinds, c.count().saturating_sub(taps));
            if let Some(twin) = c.twin() {
                kinds |= classes[usize::from(twin)].kinds;
                left = left.saturating_sub(tapped[usize::from(twin)]);
            }
            u.add(kinds, u16::from(c.amount) * u16::from(left));
        }
    }

    /// How many sources a plan sacrifices.
    pub(super) fn sacrificed(&self, plan: &Plan<L>) -> u8 {
        self.classes()
            .iter()
            .zip(plan.taps.iter())
            .filter(|(c, _)| c.consumed)
            .map(|(_, &n)| n)
            .sum()
    }

    /// Whether the source in `slot` is tapped by a plan.
    pub(super) fn taps_slot(&self, tapped: &Taps<L>, slot: L::Slot) -> bool {
        self.classes()
            .iter()
            .zip(tapped.iter())
            .any(|(c, &taps)| c.members.contains(slot) && taps > 0)
    }
}

/// Everything `player` could pay mana with now for what `paying` is for: their pool and
/// untapped sources, but those in `exclude`.
pub(crate) fn available<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> Result<Units, Full> {
    let sources = Sources::of(s, t, player, exclude, paying)?;
    Ok(sources.units(s.player(player).mana().amounts(), &Table::filled(0)))
}

/// Adds to `classes` a class of its own for each source `player` controls, but those in
/// `exclude`, whose restricted mana `paying` accepts ([`super::remembered`]), and says
/// whether it added one.
#[cold]
#[inline(never)]
fn restricted_classes<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
    classes: &mut L::Queue<Class<L>, CLASSES>,
) -> Result<bool, Full> {
    let mut added = false;
    for r in remembered::restricted_sources(s, t, player)?.iter() {
        if exclude.contains(r.slot) || !paying.accepts(t, r.restriction, r.chosen_type) {
            continue;
        }
        classes.push(Class {
            kinds: r.kinds,
            amount: 1,
            rank: RESTRICTED_RANK,
            members: Bitboard::single(r.slot),
            consumed: false,
            least: s.definition(r.slot).key(),
            restricted: true,
            rider: t.restriction(r.restriction).rider.is_some(),
            twin: NO_TWIN,
        })?;
        added = true;
    }
    Ok(added)
}

/// The mana that remembers in `player`'s pool that `paying` accepts, with whether each has a
/// rider.
#[cold]
#[inline(never)]
fn accepted<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    paying: Paying<L>,
) -> L::Queue<(u8, u8, bool), 4> {
    let mut out: L::Queue<(u8, u8, bool), 4> = Default::default();
    let pool = s.player(player).remembered_mana();
    for &(i, kind) in remembered::accepted_entries(s, t, player, paying).iter() {
        let restriction = t.restriction(pool[usize::from(i)].restriction);
        out.push_bounded((i, kind, restriction.rider.is_some()));
    }
    out
}

/// Links each restricted class with the class of its member's unrestricted mana, if it has one
/// ([`Class::twin`]), once the classes are in their order.
fn link_twins<L: Layout>(classes: &mut [Class<L>]) {
    for i in 0..classes.len() {
        if !classes[i].restricted {
            continue;
        }
        let member = classes[i].members;
        if let Some(j) = (0..classes.len()).find(|&j| j != i && classes[j].members == member) {
            classes[i].twin = u8::try_from(j).expect("a class index");
            classes[j].twin = u8::try_from(i).expect("a class index");
        }
    }
}
const _: () = assert!(size_of::<Class<Small>>() == 40);
