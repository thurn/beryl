//! What a plan leaves payable later: the costs a player may still face this turn.

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, CostAction, Face, Keywords, ManaCost, Replacement, Rule, Timing, Types,
};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::{ObjectFlags, State, Subject, TurnStep, Zone, ZoneKey};
use crate::table::{GameTable, ManaFace};

use super::solver::{Plan, each_plan};
use super::sources::Sources;
use super::{Taps, units::Units};

/// A cost the player might want to pay later: a card's mana cost, or an ability's
/// activation cost and the permanent it taps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Later<L: Layout = Small> {
    cost: ManaCost,
    /// The permanent whose {T} the cost includes, or none.
    taps: Option<L::Slot>,
}

/// The costs a payment should keep payable (§10): the mana costs of the player's cards in
/// hand and the activation costs of their permanents' abilities, each once.
pub(super) fn later_costs<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> L::Queue<Later<L>, 32> {
    let mut later: L::Queue<Later<L>, 32> = Default::default();
    let mut add = |l: Later<L>| {
        if !later.iter().any(|&x| x == l) {
            later.push_if_room(l); // A heuristic: what a fixed layout has no room for is left.
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
                if a.kind == AbilityKind::Activated && a.cost.does(CostAction::Discard) {
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
        if s.controller(slot) != player || o.flags().has(ObjectFlags::TAPPED) {
            continue;
        }
        for a in s.copiable(slot).face(t).abilities {
            if a.kind == AbilityKind::Activated && !a.is_loyalty() {
                add(Later {
                    cost: a.cost.mana.with_x(0),
                    taps: a.cost.does(CostAction::Tap).then_some(slot),
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
pub(super) fn still_payable<L: Layout>(
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
        } else if self.later.push_if_room(l) {
            // Past a fixed layout's capacity, the rest are not considered: a heuristic.
            self.copies.push_bounded(1); // One for each of `later`, in a list as long.
        }
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
/// ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)), or a sorcery given flash
/// by a rule change. Casting's own check is kept apart: the hot priority path inlines it.
fn instant_speed<L: Layout>(s: &State<L>, t: &GameTable, face: &Face, player: Seat) -> bool {
    face.types.intersects(Types::INSTANT)
        || face.keywords.contains(Keywords::FLASH)
        || (face.types.intersects(Types::SORCERY)
            && s.rule_applies(t, Rule::SorceriesAsFlash, Subject::Player(player)))
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
    let land_drop = sorcery && s.player(player).lands_played() < 1;
    // A transforming card's back face is never cast: "A double-faced spell is cast with its
    // front face up by default" ([CR 712.8](https://yawgatog.com/resources/magic-rules/#R7128)).
    // A land is played, not cast ([CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051)).
    let castable = |face: &Face| {
        !face.transformed
            && !face.types.intersects(Types::LAND)
            && (sorcery || instant_speed(s, t, face, player))
    };
    let activatable = |a: &Ability| sorcery || a.timing == Timing::Instant;
    // The hand by card, not by slot: [`Lookahead::together`]'s search is bounded, so the order
    // it meets the costs in can decide, and the slots are where the game's randomness dealt
    // the cards.
    let mut hand: L::Queue<L::Slot, 64> = Default::default();
    for slot in s.in_candidate_order(s.zone(ZoneKey::Of(player, Zone::Hand))) {
        hand.push_bounded(slot); // Their own cards: no more than a fixed layout's deck.
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
                ahead.lands.push_if_room((kinds, amount)); // A heuristic's sample.
            }
            for a in face.abilities {
                if a.kind == AbilityKind::Activated
                    && a.cost.does(CostAction::Discard)
                    && activatable(a)
                {
                    ahead.add(a.cost.mana, None);
                }
            }
        }
    }
    for p in s.cast_permissions().filter(|p| p.terms.player == player) {
        // Only a card is cast (CR 601.2a).
        let faces = s.card_def(t, p.card.slot).map_or_default(|def| def.faces);
        for face in faces {
            if castable(face) {
                let cost = face.cost;
                ahead.add(
                    ManaCost {
                        generic: cost.generic.saturating_add(p.terms.increase),
                        ..cost
                    },
                    None,
                );
            }
        }
    }
    for &slot in s.battlefield() {
        let o = s.object(slot);
        if s.controller(slot) != player || o.flags().has(ObjectFlags::TAPPED) {
            continue;
        }
        for a in s.copiable(slot).face(t).abilities {
            if a.kind == AbilityKind::Activated && !a.is_loyalty() && activatable(a) {
                ahead.add(a.cost.mana, a.cost.does(CostAction::Tap).then_some(slot));
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
            used.push_bounded(0); // One for each of `later`, in a list as long.
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

/// The human's payment ([`crate::Profile::taps_mana`]): each of the plans `kept` offers, as
/// another player's, replaced by the way to pay `cost` leaving the same remainder and
/// sacrificing as many sources whose [`Outlook`] over the rest of the turn is best. Which
/// plans a payment offers stays the same under either profile, so a decision offered to the
/// human reads the same to a search that clears it; only which sources each taps changes.
#[cold]
#[inline(never)]
pub(super) fn looking_ahead<L: Layout>(
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
        let creatures_of = c
            .members
            .filter(|m| s.derived(m).types.intersects(Types::CREATURE));
        creatures[i] = u8::try_from(creatures_of.count()).unwrap_or(u8::MAX);
    }
    let mut best: L::Queue<Outlook, 64> = Default::default();
    for k in kept.iter() {
        best.push(outlook(sources, k, &ahead, &creatures))?;
    }
    each_plan(cost, s.player(player).mana().amounts(), sources, &mut |p| {
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
