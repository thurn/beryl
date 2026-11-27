//! Whether a player may begin to cast a spell (CR 601.3), and so which casts a priority
//! decision offers. `activation` does the same for activated abilities (CR 602.5), and
//! `refusal` says why a card the player clicked may not be cast.
//!
//! Implements: CR 305.9, CR 702.37a

use std::cell::{Cell, OnceCell};

use crate::bitboard::Bitboard;
use crate::card::{CastingOption, Castings, Face, FaceIx, ManaCost, Types};
use crate::continuous::rule_change;
use crate::decision::{Candidates, OPTIONS, Opt, Refusal};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::mana::{self, Funds};
use crate::state::{RuleKind, State};
use crate::table::{Capability, GameTable};

use super::convoke;
use super::costs::{From, base_spell_cost, option_sets, x_range_start};
use super::delve;
use super::mana_payment;
use super::parts;
use super::targets::{Itself, targetable};
use super::timing::{instant_speed, sorcery_timing};

mod activation;
mod copies;
mod others;
mod preparation;
mod refusal;
pub(super) mod special;
mod ways;

use activation::activation_options;
pub use activation::pointless;
pub(super) use activation::spent_by_cost;
use refusal::keep_furthest;
pub(crate) use refusal::refusal;
pub use ways::castable_from_graveyard;
use ways::{Ways, cast_sources};

/// What every check of one player's options reads, found once for all of them: the mana they
/// could pay with and whether an opponent limits them to sorcery speed, each found at the first
/// check that needs it.
///
/// `TARGETS` is whether a spell or ability needs a legal target to be offered. It is false only
/// for [`payable_responses`]. It is a const parameter rather than a field so that the checks
/// behind every priority decision are compiled separately from the search's copy: with a
/// second caller, the compiler stopped inlining [`activation_options`] into the decision
/// builder that rollouts run, which cost the rollout benchmark about 0.3%.
pub(super) struct Situation<L: Layout = Small, const TARGETS: bool = true> {
    player: Seat,
    /// None once found if more classes of sources than [`Funds`] holds: a stall for whatever
    /// needed them.
    funds: OnceCell<Option<Funds<L>>>,
    restricted: OnceCell<Option<L::Queue<mana::Restricted<L>, { mana::RESTRICTED }>>>,
    spell_costs: OnceCell<rule_change::SpellCosts>,
    full: Cell<bool>,
}

impl<L: Layout, const TARGETS: bool> Situation<L, TARGETS> {
    pub(super) fn of(player: Seat) -> Self {
        Situation {
            player,
            funds: OnceCell::new(),
            restricted: OnceCell::new(),
            spell_costs: OnceCell::new(),
            full: Cell::new(false),
        }
    }

    fn funds(&self, s: &State<L>, t: &GameTable) -> Option<&Funds<L>> {
        let funds = self
            .funds
            .get_or_init(|| Funds::of(s, t, self.player).ok())
            .as_ref();
        if funds.is_none() {
            self.full.set(true);
        }
        funds
    }

    fn restricted(&self, s: &State<L>, t: &GameTable) -> Option<&[mana::Restricted<L>]> {
        self.restricted
            .get_or_init(|| mana::restricted_sources(s, t, self.player).ok())
            .as_ref()
            .map(Queue::as_slice)
    }

    #[inline]
    fn apply_spell_cost(
        &self,
        s: &State<L>,
        t: &GameTable,
        face: &Face,
        cost: ManaCost,
    ) -> ManaCost {
        if t.rules_of(RuleKind::SpellCost).is_empty() {
            return cost;
        }
        self.spell_costs
            .get_or_init(|| rule_change::SpellCosts::of(s, t, self.player))
            .apply(s, t, self.player, face, cost)
    }

    /// `Full` if a check needed more classes of sources than [`Funds`] holds, so that its answer was
    /// not found: the options listed stand only if this is `Ok`.
    pub(super) fn complete(&self) -> Result<(), Full> {
        if self.full.get() { Err(Full) } else { Ok(()) }
    }
}

/// Whether a face may be cast now, as far as its card types and the timing rules say: what
/// [`refused`] checks first, which depends only on the face, so on the card.
#[inline]
fn in_time<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    face: &Face,
    from: From,
) -> bool {
    // CR 305.9: "If an object is both a land and another card type, it can be played only as
    // a land. It can't be cast as a spell." see CR 712.8c: "Only the face that will be face up
    // on the stack is evaluated to determine if it can be cast."
    if face.types.intersects(Types::LAND) {
        return false;
    }
    match from {
        // Sorcery timing first: it is three compares, lets any spell be cast, and is how
        // most spells are, so the rule changes are read only outside it.
        From::Priority => {
            sorcery_timing(s, now.player)
                || (instant_speed(s, t, face, now.player)
                    && !rule_change::cast_only_as_sorcery(s, t, now.player, face))
        }
        // During a resolution the stack is never empty, so a sorcery-speed limit forbids it.
        From::Resolution { .. } => !rule_change::cast_only_as_sorcery(s, t, now.player, face),
    }
}

/// Whether `cost` is paid with restricted mana or with objects (see CR 702.66a, see CR 702.51a).
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn pays_otherwise<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    funds: &Funds<L>,
    cost: ManaCost,
    player: Seat,
    slot: L::Slot,
    face: FaceIx,
    face_ref: &Face,
    costs: Castings,
) -> bool {
    if t.can(Capability::RemembersMana)
        && t.restrictions()
            .iter()
            .any(|r| rule_change::accepts(r.cast, face_ref))
    {
        let Some(restricted) = now.restricted(s, t) else {
            return true;
        };
        let has_pool = !s.player(now.player).remembered_mana().is_empty();
        if has_pool
            || restricted
                .iter()
                .any(|r| rule_change::accepts(t.restriction(r.restriction).cast, face_ref))
        {
            let paying = mana::Paying::casting(s, t, slot, face);
            if paying.reads_restrictions(t)
                && funds.pays_restricted_with(s, t, cost, Bitboard::EMPTY, paying, restricted)
            {
                return true;
            }
        }
    }
    t.can(Capability::PaysWithObjects)
        && ((delve::delves(face_ref)
            && delve::delve_pays(s, t, funds, player, slot, face, costs, cost))
            || convoke::part_of(face_ref).is_some_and(|index| {
                convoke::convoke_pays(s, t, funds, player, slot, face, index, cost)
            }))
}

/// Why `player` may not cast the card in `slot` with this face and these casting options:
/// the first check it fails, or none if they may. A timing refusal names no permanent here
/// ([`refusal`] does).
///
/// [CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to cast
/// a spell only if a rule or effect allows that player to cast it and no rule or effect
/// prohibits that player from casting it."
/// See [CR 716.3a]: "When casting an adventurer card as an Adventure, only the alternative
/// characteristics are evaluated to see if it can be cast."
///
/// [CR 716.3a]: https://yawgatog.com/resources/magic-rules/#R7163a
#[expect(
    clippy::inline_always,
    reason = "with the escape casts a second caller, the compiler called it out of line from \
              the priority decision's casts, which cost the rollout benchmark 0.25%"
)]
#[inline(always)]
#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face being cast (see CR 709.3b)"
)]
fn refused<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    face_index: FaceIx,
    costs: Castings,
    from: From,
    increase: u8,
) -> Option<Refusal<L>> {
    let player = now.player;
    let face = s.copiable(slot).turned(t, face_index).face(t);
    // A rule change that prohibits the cast (CR 601.3 above) is told as a timing refusal.
    if !in_time(s, t, now, face, from) || rule_change::cant_cast(s, t, player, face) {
        return Some(Refusal::Timing { by: None });
    }
    let free = matches!(from, From::Resolution { free: true });
    // The checks are pure, so their order changes nothing but their cost: the mana, found
    // once for the decision, first, and the targets, which read the battlefield, last.
    // [CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183): "A player can't pay a
    // cost without having the necessary resources to pay it fully."
    let x = x_range_start(face, costs, free).unwrap_or(0);
    let cost = now.apply_spell_cost(s, t, face, base_spell_cost(face, costs, x, increase, free));
    if !now.funds(s, t).is_some_and(|f| {
        f.pays(s, t, cost, Bitboard::EMPTY)
            || (t.can_either(Capability::RemembersMana, Capability::PaysWithObjects)
                && pays_otherwise(s, t, now, f, cost, player, slot, face_index, face, costs))
            || (t.can_either(Capability::PricedMana, Capability::ManaChanges)
                && f.pays_for(s, t, cost, Bitboard::EMPTY, mana::Paying::NOTHING))
    }) {
        return Some(Refusal::Mana);
    }
    // CR 601.2h: the player pays the total cost, including any parts funded by earlier parts.
    let payable = if t.can(Capability::SacrificeToCast) {
        mana_payment::whole_cost_payable(s, t, player, slot, face_index, face, costs, cost)
    } else {
        parts::spell_payable(s, t, player, slot, face, costs)
    };
    match payable {
        Ok(true) => {}
        Ok(false) => return Some(Refusal::Cost),
        Err(Full) => {
            now.full.set(true);
            return Some(Refusal::Cost);
        }
    }
    (TARGETS
        && !targetable(
            s,
            t,
            face.spell_ability_with(costs),
            player,
            Itself {
                spell: Some(slot),
                on_stack: false,
            },
            0,
            None,
        ))
    .then_some(Refusal::Target)
}

/// Appends the spells `player` can cast now, by card, face and casting options.
#[allow(
    clippy::disallowed_methods,
    reason = "reads each face the card may be cast as"
)]
pub(super) fn cast_options<L: Layout, const TARGETS: bool, const SEARCH: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    cards: Bitboard<L::Slot>,
    from: From,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let player = now.player;
    // Cards no face of which may be cast now are left out before collapsing, which saves
    // comparing them: whether one may depends only on its card, which interchangeable cards
    // share, so the cards kept are the same.
    let special = !SEARCH && t.can_either(Capability::Morph, Capability::Split);
    let cards = cards.filter(|slot| {
        copies::castable_def(s, t, slot, from).is_some_and(|def| {
            def.face_ixs().any(|f| {
                let face = def.face(f);
                face.playable()
                    && (!SEARCH || face.permits_cast_while_searching())
                    && in_time(s, t, now, face, from)
            }) || (special && special::in_time(s, t, now, slot, def, from))
        })
    });
    // Each card cast during a search leaves its own place in the library.
    let cards = if SEARCH { cards } else { collapse(s, cards) };
    Candidates::<L>::extend_options(out, cards, |out, slot| {
        let Some(def) = copies::castable_def(s, t, slot, from) else {
            return Ok(());
        };
        let ways = Ways::of(s, t, player, slot, from);
        for f in def.face_ixs() {
            let face = def.face(f);
            if !face.playable() || SEARCH && !face.permits_cast_while_searching() {
                continue;
            }
            let ways = if ways.on_adventure && f == FaceIx::BACK {
                ways.back(s, player, slot)
            } else {
                ways
            };
            if ways.plain {
                for costs in option_sets(face, ways.alternative) {
                    if ways.permitted(s, t, player, face, costs)
                        && refused(s, t, now, slot, f, costs, from, ways.increase).is_none()
                    {
                        out.push(Opt::Cast {
                            slot,
                            face: f,
                            costs,
                        })?;
                    }
                }
            }
            if ways.escape {
                escape_options(s, t, now, slot, f, from, out)?;
            }
        }
        if special {
            special::options(s, t, now, slot, def, ways, from, out)?;
        }
        Ok(())
    })
}

/// A card with morph may be cast face down. [CR 702.37a] says "Morph is a static ability
/// that functions in any zone from which you could play the card it’s on".
///
/// [CR 702.37a]: https://yawgatog.com/resources/magic-rules/#R70237a
fn has_morph(face: &Face) -> bool {
    face.casting
        .iter()
        .any(|o| matches!(o, CastingOption::Morph(_)))
}

/// Appends the casts of face `face` of the card in `slot` with the escape an effect grants it
/// ([`Castings::ESCAPE`]), with each set of the face's own options that pays none of its
/// alternative costs. Out of line: only a card that has escape comes here.
#[cold]
#[inline(never)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face being cast (see CR 709.3b)"
)]
fn escape_options<L: Layout, const TARGETS: bool>(
    s: &State<L>,
    t: &GameTable,
    now: &Situation<L, TARGETS>,
    slot: L::Slot,
    face: FaceIx,
    from: From,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let def_face = s.copiable(slot).turned(t, face).face(t);
    for costs in option_sets(def_face, true).map(Castings::with_escape) {
        if refused(s, t, now, slot, face, costs, from, 0).is_none() {
            out.push(Opt::Cast { slot, face, costs })?;
        }
    }
    Ok(())
}

/// Appends the priority actions casting and activation add: spells and abilities.
pub(crate) fn priority_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let now = Situation::of(player);
    cast_options::<L, true, false>(s, t, &now, cast_sources(s, t, player), From::Priority, out)?;
    activation_options(s, t, &now, out)?;
    if t.can(Capability::Preparation) {
        preparation::casts(s, t, &now, out)?;
    }
    now.complete()?;
    Ok(())
}

/// Spells and abilities whose costs `player` could pay at instant speed on another
/// player's turn. A response's target may only appear after the other player acts.
///
/// Under see [CR 117.1a], "A player may cast an instant spell any time they have priority."
/// Under [CR 118.3], "A player can't pay a cost without having the necessary resources
/// to pay it fully."
///
/// [CR 117.1a]: https://yawgatog.com/resources/magic-rules/#R1171a
/// [CR 118.3]: https://yawgatog.com/resources/magic-rules/#R1183
pub fn payable_responses<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    debug_assert_ne!(s.turn().active(), player);
    // A spell or ability with no legal target yet still counts.
    let now = Situation::<L, false>::of(player);
    let mut out = Default::default();
    cast_options::<L, false, false>(
        s,
        t,
        &now,
        cast_sources(s, t, player),
        From::Priority,
        &mut out,
    )?;
    activation_options(s, t, &now, &mut out)?;
    if t.can(Capability::Preparation) {
        preparation::casts(s, t, &now, &mut out)?;
    }
    now.complete()?;
    Ok(out)
}
