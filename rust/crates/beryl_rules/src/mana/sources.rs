//! What can make mana: the sources a player's mana abilities form, in classes of
//! interchangeable permanents.

use crate::bitboard::Bitboard;
use crate::card::{Ability, Face, Types};
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::{Derived, ObjectFlags, State};
use crate::table::{GameTable, ManaFace, has_other_ability, is_sacrificed, is_simple};
use crate::{filter::Evaluate, fixed::Full};

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
}

impl<L: Layout> Class<L> {
    pub(super) fn count(&self) -> u8 {
        debug_assert!(self.members.count() <= 255, "a class past its room");
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
pub(super) fn allowed<L: Layout>(s: &State<L>, t: &GameTable, a: &Ability, player: Seat) -> bool {
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
pub(super) fn controlled<L: Layout>(
    s: &State<L>,
    player: Seat,
) -> impl Iterator<Item = (L::Slot, &Derived<L>)> + '_ {
    s.permanents().filter(move |(_, d)| d.controller == player)
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
    let o = s.object(slot);
    // Looked up only past the table's facts, which most permanents stop at.
    let face = || s.copiable(slot).face(t);
    let mana = s.copiable(slot).facts(t).mana;
    // The table's facts stand in for reading the face: they must say what reading it would.
    debug_assert!(
        mana != ManaFace::Not || read_source(s, t, player, face()).is_none(),
        "a face the table calls no source makes mana"
    );
    if mana == ManaFace::Not || d.controller != player || o.flags().has(ObjectFlags::TAPPED) {
        return None;
    }
    // CR 302.6: a creature's {T} abilities need it under its controller's control since their
    // most recent turn began.
    if d.types.intersects(Types::CREATURE) && o.flags().has(ObjectFlags::SUMMONING_SICK) {
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

    pub(super) fn classes(&self) -> &[Class<L>] {
        self.classes.as_slice()
    }

    /// The units these sources and a pool make together, less the sources a plan taps.
    pub(super) fn units(&self, pool: [u8; KINDS], tapped: &Taps<L>) -> Units {
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

/// Everything `player` could pay mana with now: their pool and untapped sources, but those
/// in `exclude`.
pub(crate) fn available<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard<L::Slot>,
) -> Result<Units, Full> {
    let sources = Sources::of(s, t, player, exclude)?;
    Ok(sources.units(s.player(player).mana().amounts(), &Table::filled(0)))
}
