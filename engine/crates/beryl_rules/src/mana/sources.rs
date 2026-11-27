//! What can make mana: the sources a player's mana abilities form, in classes of
//! interchangeable permanents.
//!
//! Implements: CR 302.6, CR 702.10c

use crate::bitboard::Bitboard;
use crate::card::AbilityKind;
#[cfg(doc)]
use crate::card::ManaColor;
use crate::card::{Ability, AbilityIx, Face, Keywords, ObjectAbility, Types, Words};
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::{Derived, State};
use crate::table::{Capability, GameTable, ManaFace, has_other_ability, is_sacrificed, is_simple};
use crate::{filter::Evaluate, fixed::Full};

use super::remembered::{self, Paying};
use super::rich::Extra;
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
    /// How many members are in `members`, kept beside it so `count()` is a byte load.
    count: u8,
    pub(super) members: Bitboard<L::Slot>,
    /// The least card among the members, which orders classes of one rank.
    least: crate::query::DefinitionKey<L::Card>,
    /// Bit flags: [`CLASS_CONSUMED`], [`CLASS_RESTRICTED`], [`CLASS_RIDER`].
    flags: u8,
    /// Of a restricted class, the class of the same member's unrestricted mana, if it has
    /// one, and of that class, the restricted one: tapping the permanent for one leaves it
    /// tapped for the other. Likewise of a consumed class whose member also taps for mana
    /// without being sacrificed ([`sacrifice_alternative`]), and of that other class.
    /// [`NO_TWIN`] for none: a byte, so that the class stays 40 bytes, which a plan's search
    /// copies.
    twin: u8,
}

// A plan's search copies classes: they stay 40 bytes, and what the rich path needs of a
// source goes in [`Sources::extras`].
const _: () = assert!(size_of::<Class<Small>>() == 40);

const CLASS_CONSUMED: u8 = 1;
const CLASS_RESTRICTED: u8 = 2;
const CLASS_RIDER: u8 = 4;

/// [`Class::twin`] of a class with none.
const NO_TWIN: u8 = u8::MAX;

impl<L: Layout> Class<L> {
    #[inline]
    pub(super) fn count(&self) -> u8 {
        self.count
    }

    #[inline]
    pub(super) fn is_consumed(&self) -> bool {
        self.flags & CLASS_CONSUMED != 0
    }

    #[inline]
    pub(super) fn is_restricted(&self) -> bool {
        self.flags & CLASS_RESTRICTED != 0
    }

    #[inline]
    pub(super) fn has_rider(&self) -> bool {
        self.flags & CLASS_RIDER != 0
    }

    /// Whether another source fits: a plan counts a class's taps in a `u8`, so in a layout of
    /// more slots than that, sources past 255 interchangeable ones form another class.
    fn has_room(&self) -> bool {
        self.count < u8::MAX
    }

    /// Whether it is a restricted or consumed class whose member has another class: its taps
    /// are counted with that class's.
    pub(super) fn is_twin(&self) -> bool {
        (self.flags & (CLASS_RESTRICTED | CLASS_CONSUMED) != 0) && self.twin != NO_TWIN
    }

    /// The class of the same member's other mana, if it has one ([`Class::twin`]).
    pub(super) fn twin(&self) -> Option<u8> {
        (self.twin != NO_TWIN).then_some(self.twin)
    }
}

/// A player's untapped sources, by class in rank order.
#[derive(Clone, Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Sources<L: Layout = Small> {
    classes: L::Queue<Class<L>, CLASSES>,
    /// The mana that remembers in the pool that the payment's restriction accepts: each
    /// entry's place in the pool, its kind as a [`ManaColor`] index and whether its
    /// restriction has a rider. A plan
    /// spends some of them ([`Plan::remembered`]).
    remembered: L::Queue<(u8, u8, bool), 4>,
    /// Whether a class is restricted.
    restricted: bool,
    /// Whether any class is consumed when tapped.
    has_consumed: bool,
    /// Whether any class makes more than one mana per tap.
    has_several: bool,
    /// Whether a class is restricted, a source has twin classes ([`Class::twin`]) or the pool
    /// holds mana that remembers the payment accepts ([`Sources::has_memory`]), found once as
    /// the sources are.
    memory: bool,
    /// Bit `i` is set if `remembered[i]` has a rider.
    rider_remembered: u8,
    /// Bit `i` is set if `classes[i]` has a rider (for `i < 32`).
    rider_classes: u32,
    /// What the rich path plans with beside the classes (`mana::rich`): empty otherwise.
    pub(super) extras: Vec<Extra<L>>,
}

/// Whether `player` may activate a mana ability now (see
/// [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)): "A player can't begin to
/// activate an ability that's prohibited from being activated." Read through `words`.
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
/// for an ability's {T} cost now: they control it, it is untapped, and a creature has been
/// theirs since their most recent turn began ([CR 302.6]): "A creature's activated ability
/// with the tap symbol ... in its activation cost can't be activated unless the creature has
/// been under its controller's control continuously since their most recent turn began."
/// Haste lifts that ([CR 702.10c]): "If a creature has haste, its controller can activate its
/// activated abilities whose cost includes the tap symbol or the untap symbol even if that
/// creature hasn't been controlled by that player continuously since their most recent turn
/// began."
///
/// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
/// [CR 702.10c]: https://yawgatog.com/resources/magic-rules/#R70210c
// Inline everywhere: every payability check asks it of each permanent its player controls.
#[allow(clippy::inline_always)]
#[inline(always)]
pub(super) fn can_tap<L: Layout>(
    s: &State<L>,
    player: Seat,
    slot: L::Slot,
    d: &Derived<L>,
) -> bool {
    let o = s.object(slot);
    d.controller == player
        && !o.is_tapped()
        && !(d.types.intersects(Types::CREATURE)
            && o.is_summoning_sick()
            && !d.keywords.contains(Keywords::HASTE))
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
#[allow(
    clippy::disallowed_methods,
    reason = "reads a permanent's face up, which the table's facts describe"
)]
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
        mana != ManaFace::Not
            || !card_mana()
            || read_source(s, t, player, slot, face(), mana).is_none(),
        "a face the table calls no source makes mana"
    );
    if (mana == ManaFace::Not && card_mana()) || !can_tap(s, player, slot, d) {
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
            read_source(s, t, player, slot, face(), mana),
            "a plain source's mana, as the table has it"
        );
        return plain;
    }
    // Past the plain faces, which most sources are: the rich path plans a priced face.
    if mana == ManaFace::Priced && card_mana() {
        return None;
    }
    read_source(s, t, player, slot, face(), mana)
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
    facts: ManaFace,
) -> Option<(u8, u8, bool, bool)> {
    let mut kinds = s.intrinsic_mana(t, slot);
    let mut amount = u8::from(kinds != 0);
    // A source whose mana depends on other permanents, as Floodfarm Verge's {U}, is a class
    // of its own: tapping it rather than another making the same mana now can leave
    // different mana for later.
    let mut conditional = false;
    let words = s.words_of(slot);
    if t.can_either(Capability::PricedMana, Capability::ManaChanges)
        && super::rich::has_extra_mana(s, t, slot)
    {
        return None;
    }
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
    let face_other = facts.other().unwrap_or_else(|| has_other_ability(face));
    let other = face_other || (!s.makes_card_mana(slot) && merged(s, t, slot));
    Some((kinds, amount, conditional || other, false))
}

/// The second way an untapped source `player` controls makes mana, the permanent in `slot`,
/// which [`source`] found makes mana for {T} alone: its mana ability that costs {T} and
/// sacrificing it ([`is_sacrificed`]), as "{T}, Sacrifice this land: Add {U}{U}." beside
/// "{T}: Add {U}.". The kinds of mana it makes and how much. Each is a mana ability of its own,
/// and its controller chooses which to activate (see
/// [CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a)): "A player may activate an
/// activated mana ability whenever they have priority". `alternatives` is whether the game's cards have such a face
/// ([`Capability::SacrificeAlternative`]): in a game with none, nothing more is read.
#[inline]
pub(super) fn sacrifice_alternative<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    alternatives: bool,
) -> Option<(u8, u8)> {
    if !alternatives {
        return None;
    }
    read_alternative(s, t, player, slot)
}

/// [`sacrifice_alternative`] in a game whose cards have such a face. Out of line.
#[cold]
#[inline(never)]
fn read_alternative<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
) -> Option<(u8, u8)> {
    find_alternative(s, t, player, slot).map(|(_, made)| (made.colors, made.amount))
}

/// The ability of the permanent in `slot` that [`sacrifice_alternative`] activates.
pub(super) fn alternative_ability<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<ObjectAbility> {
    find_alternative(s, t, s.controller(slot), slot).map(|(ability, _)| ability)
}

/// The first mana ability of the permanent in `slot` that costs {T} and sacrificing it and
/// that `player` may activate now, and the mana it makes.
fn find_alternative<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
) -> Option<(ObjectAbility, crate::card::ManaSet)> {
    let words = s.words_of(slot);
    s.abilities(t, slot)
        .find(|&(_, a)| is_sacrificed(a) && allowed(s, t, a, player, &words))
        .map(|(ability, a)| (ability, a.produces()))
}

/// Whether `ability` of the permanent in `slot` is the mana ability [`sacrifice_alternative`]
/// finds: one that costs {T} and sacrificing it, of a source that also makes mana for {T}
/// alone, which priority offers as an option of its own.
pub(crate) fn is_alternative<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
) -> bool {
    t.can(Capability::SacrificeAlternative)
        && s.abilities(t, slot).any(|(a, printed)| {
            a == ability && printed.kind() == AbilityKind::Mana && is_sacrificed(printed)
        })
        && source(s, t, s.controller(slot), slot, s.derived(slot))
            .is_some_and(|(.., consumed)| !consumed)
}

/// The most a source makes that can tap for `kinds` and `amount` or, as its twin, for
/// `twin`'s kinds and amount, as one count of units of some kinds: the larger amount of one
/// kind where both make the same kinds, and one amount of either's kinds where they differ.
/// Where the two differ in both kinds and amount, this is the smaller amount, which never
/// says that a source pays what it cannot.
pub(super) fn capacity(kinds: u8, amount: u8, twin: Option<(u8, u8)>) -> (u8, u8) {
    match twin {
        None => (kinds, amount),
        Some((k, n)) if k == kinds => (kinds, amount.max(n)),
        Some((k, n)) => (kinds | k, amount.min(n)),
    }
}

impl<L: Layout> Sources<L> {
    /// The untapped sources `player` controls, but those in `exclude`, for a payment of what
    /// `paying` is for.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn of(
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
    ) -> Result<Self, Full> {
        // Built in place: a copy of its classes costs every payment.
        let mut out = Sources::<L>::default();
        let alternatives = t.can(Capability::SacrificeAlternative);
        let reads_restrictions = paying.reads_restrictions(t);
        let mut restricted_slots: L::Queue<L::Slot, 16> = Default::default();
        let mut twins = false;
        let classes = &mut out.classes;
        for (slot, d) in controlled(s, player) {
            if exclude.contains(slot) {
                continue;
            }
            if reads_restrictions
                && s.definition(slot).facts(t).has_restricted_mana
                && can_tap(s, player, slot, d)
            {
                restricted_slots.push(slot)?;
            }
            let Some((kinds, amount, other, consumed)) = source(s, t, player, slot, d) else {
                continue;
            };
            if amount > 1 {
                out.has_several = true;
            }
            if consumed {
                debug_assert!(!other);
                out.has_consumed = true;
                let class = Class {
                    kinds,
                    amount,
                    rank: CONSUMED_RANK,
                    count: 1,
                    members: Bitboard::single(slot),
                    least: s.definition(slot).key(),
                    flags: CLASS_CONSUMED,
                    twin: NO_TWIN,
                };
                // Only permanents of one card are interchangeable when spending one: the
                // payment sacrifices whichever comes first.
                let same = |c: &Class<L>| {
                    c.is_consumed() && c.has_room() && c.least == s.definition(slot).key() && {
                        // A twin's class has its one member.
                        let member = c.members.min(|m| m).expect("a class has a member");
                        sacrifice_alternative(s, t, player, member, alternatives).is_none()
                    }
                };
                if let Some(i) = classes.iter().position(same) {
                    let c = &mut classes.as_mut_slice()[i];
                    c.members.insert(slot);
                    c.count += 1;
                } else {
                    classes.push(class)?;
                }
                continue;
            }
            // A source that may also sacrifice itself for mana has a consumed class of its
            // own beside its other class, its twin: a plan taps it for one or the other.
            if let Some((kinds, amount)) = sacrifice_alternative(s, t, player, slot, alternatives) {
                debug_assert!(
                    other,
                    "a source with a second mana ability is a class of its own"
                );
                out.has_consumed = true;
                if amount > 1 {
                    out.has_several = true;
                }
                classes.push(Class {
                    kinds,
                    amount,
                    rank: CONSUMED_RANK,
                    count: 1,
                    members: Bitboard::single(slot),
                    least: s.definition(slot).key(),
                    flags: CLASS_CONSUMED,
                    twin: NO_TWIN,
                })?;
                twins = true;
            }
            let rank = u8::try_from(kinds.count_ones()).expect("number of bits in a byte") * 2
                + u8::from(other);
            // A source with another ability is a class of its own: tapping it for mana
            // costs that ability, so it is never interchangeable.
            if let Some(i) = classes.iter().position(|c| {
                !other
                    && !c.is_consumed()
                    && c.kinds == kinds
                    && c.amount == amount
                    && c.rank == rank
                    && c.has_room()
            }) {
                let c = &mut classes.as_mut_slice()[i];
                c.members.insert(slot);
                c.count += 1;
                c.least = c.least.min(s.definition(slot).key());
            } else {
                let mut members = Bitboard::EMPTY;
                members.insert(slot);
                classes.push(Class {
                    kinds,
                    amount,
                    rank,
                    count: 1,
                    members,
                    least: s.definition(slot).key(),
                    flags: 0,
                    twin: NO_TWIN,
                })?;
            }
        }
        if reads_restrictions {
            if !restricted_slots.is_empty() {
                out.restricted = restricted_classes(
                    s,
                    t,
                    player,
                    restricted_slots.as_slice(),
                    paying,
                    &mut out.classes,
                )?;
            }
            if !s.player(player).remembered_mana().is_empty() {
                out.remembered = remembered::accepted_entries(s, t, player, paying);
            }
            out.memory = out.restricted || !out.remembered.is_empty();
        }
        out.memory |= twins;
        // Classes of one rank by card, never by slot, which the game's randomness chose as it
        // dealt the cards to their slots: by the least card of each, as a class may hold
        // several, such as Islands and a modal double-faced card's land face.
        out.classes
            .as_mut_slice()
            .sort_by_key(|c| (c.rank, c.least));
        if out.restricted || twins {
            link_twins(out.classes.as_mut_slice());
        }
        if out.restricted {
            for (i, c) in out.classes.iter().enumerate() {
                if c.has_rider() && i < 32 {
                    out.rider_classes |= 1 << i;
                }
            }
        }
        for (i, &(_, _, rider)) in out.remembered.iter().enumerate() {
            if rider {
                out.rider_remembered |= 1 << i;
            }
        }
        if t.can_either(Capability::PricedMana, Capability::ManaChanges) {
            super::rich::collect_extras(s, t, player, exclude, &mut out.extras)?;
        }
        Ok(out)
    }

    /// Whether a class is restricted, a source has twin classes or the pool holds mana that
    /// remembers the payment accepts: never in a game whose cards make no restricted mana and
    /// have no [`sacrifice_alternative`].
    #[inline]
    pub(super) fn has_memory(&self) -> bool {
        self.memory
    }

    #[inline]
    pub(super) fn has_consumed(&self) -> bool {
        self.has_consumed
    }

    #[inline]
    pub(super) fn has_several(&self) -> bool {
        self.has_several
    }

    #[inline]
    pub(super) fn has_rider(&self) -> bool {
        self.rider_remembered != 0 || self.rider_classes != 0 || self.classes().len() > 32
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
        if self.rider_remembered & plan.remembered != 0 {
            return true;
        }
        let mut bits = self.rider_classes;
        while bits != 0 {
            let i = bits.trailing_zeros() as usize;
            if plan.taps[i] > 0 {
                return true;
            }
            bits &= bits - 1;
        }
        if self.classes().len() > 32 {
            return (self.classes()[32..]
                .iter()
                .zip(plan.taps.as_slice()[32..].iter()))
            .any(|(c, &n)| c.has_rider() && n > 0);
        }
        false
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
    /// [`Sources::remembered`]. An untapped permanent with twin classes makes what
    /// [`capacity`] says.
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
        if !self.remembered.is_empty() {
            for (i, &(_, kind, _)) in self.remembered().iter().enumerate() {
                if spent & (1 << i) == 0 {
                    u.add(1 << kind, 1);
                }
            }
        }
        let classes = self.classes();
        for (c, &taps) in classes.iter().zip(tapped.iter()) {
            if c.is_twin() {
                continue;
            }
            let (kinds, amount, left) = if let Some(twin) = c.twin() {
                let other = &classes[usize::from(twin)];
                let left = c
                    .count()
                    .saturating_sub(taps)
                    .saturating_sub(tapped[usize::from(twin)]);
                let (kinds, amount) =
                    capacity(c.kinds, c.amount, Some((other.kinds, other.amount)));
                (kinds, amount, left)
            } else {
                (c.kinds, c.amount, c.count() - taps)
            };
            if left > 0 {
                u.add(kinds, u16::from(amount) * u16::from(left));
            }
        }
    }

    /// How many sources a plan sacrifices.
    #[inline]
    pub(super) fn sacrificed(&self, plan: &Plan<L>) -> u8 {
        if !self.has_consumed {
            return 0;
        }
        self.classes()
            .iter()
            .zip(plan.taps.iter())
            .filter(|(c, _)| c.is_consumed())
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
    Ok(sources.units(s.player(player).mana().amounts_u8(), &Table::filled(0)))
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
    restricted_slots: &[L::Slot],
    paying: Paying<L>,
    classes: &mut L::Queue<Class<L>, CLASSES>,
) -> Result<bool, Full> {
    let mut added = false;
    let mut found = 0usize;
    for &slot in restricted_slots {
        let Some(r) = remembered::restricted_of_slot(s, t, player, slot, s.derived(slot)) else {
            continue;
        };
        found += 1;
        if found > remembered::RESTRICTED {
            return Err(Full);
        }
        if !paying.accepts(t, r.restriction, r.chosen_type) {
            continue;
        }
        let rider = if t.restriction(r.restriction).rider.is_some() {
            CLASS_RIDER
        } else {
            0
        };
        classes.push(Class {
            kinds: r.kinds,
            amount: 1,
            rank: RESTRICTED_RANK,
            count: 1,
            members: Bitboard::single(r.slot),
            least: s.definition(r.slot).key(),
            flags: CLASS_RESTRICTED | rider,
            twin: NO_TWIN,
        })?;
        added = true;
    }
    Ok(added)
}

/// Links each restricted class with the class of its member's unrestricted mana, if it has one,
/// and each consumed class with the class of its member's mana for {T} alone
/// ([`Class::twin`]), once the classes are in their order.
fn link_twins<L: Layout>(classes: &mut [Class<L>]) {
    for i in 0..classes.len() {
        if !classes[i].is_restricted() && !classes[i].is_consumed() {
            continue;
        }
        let member = classes[i].members;
        if let Some(j) = (0..classes.len()).find(|&j| j != i && classes[j].members == member) {
            classes[i].twin = u8::try_from(j).expect("a class index");
            classes[j].twin = u8::try_from(i).expect("a class index");
        }
    }
}
