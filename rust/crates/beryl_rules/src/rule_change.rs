//! Rule changes: what the rules ask of the rule changes in force, whether a static ability
//! or a resolved effect makes them
//! ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some continuous
//! effects affect game rules rather than objects."
//!
//! A rule change is data (`beryl_card`'s `RuleChange`), which a game numbers
//! (`beryl_kernel`'s `rule_changes`). The set in force for a player is kept current with
//! the layers ([`crate::layers`]) and by the Effects store, and the card table has the set
//! of each kind a check here asks about, so asking while none of that kind is in force is
//! one AND and walks nothing. Only when one is does a check read what it says: the spells
//! its filter accepts, or its condition. Which permanent makes a change is found only to
//! tell a player why.

use beryl_card::{Governed, RuleChange};

use crate::card::{Ability, AbilityIx, AbilityKind, Face};
use crate::filter::{self, Filter};
use crate::ids::Seat;
use crate::interpret;
use crate::layout::Layout;
use crate::state::{RuleId, RuleKind, Rules, State};
use crate::table::GameTable;

/// Whether a rule change in force has `player` cast the spell `face` would be only when
/// they could cast a sorcery, as a static ability "Each opponent can cast spells only any
/// time they could cast a sorcery." does each opponent. A prohibition beats a permission
/// ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)): "When a rule or effect
/// allows or directs something to happen, and another effect states that it can't happen,
/// the "can't" effect takes precedence."
#[inline]
pub(crate) fn cast_only_as_sorcery<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
) -> bool {
    governs_cast(s, t, RuleKind::CastOnlyAsSorcery, player, face)
}

/// Whether a rule change in force lets `player` cast the spell `face` would be any time
/// they could cast an instant, as "You may cast sorcery spells as though they had flash."
/// does ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)).
#[inline]
pub(crate) fn cast_as_instant<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
) -> bool {
    governs_cast(s, t, RuleKind::CastAsInstant, player, face)
}

/// Whether a rule change of `kind` in force for `player` governs their casting the spell
/// `face` would be: one AND while none of that kind is in force for them.
#[inline]
fn governs_cast<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    kind: RuleKind,
    player: Seat,
    face: &Face,
) -> bool {
    let in_force = s.rules_for(player).common(t.rules_of(kind));
    !in_force.is_empty()
        && (!in_force.common(t.rules_of_every_spell()).is_empty()
            || governs_a_cast_of(t, in_force, face))
}

/// Whether one of `rules`, which govern casting, governs casting the spell `face` would be.
/// Out of line: a check comes here only while a rule change of its kind that reads a filter
/// is in force, and inlined it keeps the timing of every spell from being inlined into the
/// casting options (the rollout benchmark counts 0.75% more instructions with it inline).
#[inline(never)]
fn governs_a_cast_of(t: &GameTable, rules: Rules, face: &Face) -> bool {
    rules.iter().any(|id| accepts(spells_of(t.rule(id)), face))
}

/// The spells a rule change that governs casting is about.
///
/// # Panics
///
/// For one that governs something else, which no caller gives.
#[inline]
fn spells_of(rule: &RuleChange) -> Filter {
    match rule.what {
        Governed::CastSpells(filter) => filter,
        Governed::ActivateLoyaltyAbilities(_) => unreachable!("a rule change about casting"),
    }
}

/// Whether the spell `face` would be passes `filter` by its own characteristics: all that
/// is read of a spell a player has yet to cast. The card table has only filters read this
/// way (`beryl_kernel`'s `rule_changes::RuleKind::of`).
#[inline]
fn accepts(filter: Filter, face: &Face) -> bool {
    match filter {
        Filter::Any => true,
        Filter::Types(types) => face.types.intersects(types),
        filter => accepts_composed(filter, face),
    }
}

/// [`accepts`], for a filter made of others: out of line, as it calls itself.
fn accepts_composed(filter: Filter, face: &Face) -> bool {
    match filter {
        Filter::Any => true,
        Filter::Types(types) => face.types.intersects(types),
        Filter::Supertypes(supertypes) => face.supertypes.intersects(supertypes),
        Filter::Subtype(subtype) => face.subtypes.contains(subtype),
        Filter::Colors(colors) => face.colors.intersects(colors),
        Filter::Not(filter) => !accepts_composed(*filter, face),
        Filter::All(filters) => filters.iter().all(|&filter| accepts_composed(filter, face)),
        Filter::AnyOf(filters) => filters.iter().any(|&filter| accepts_composed(filter, face)),
        Filter::Whose(_)
        | Filter::Tapped
        | Filter::ManaValueAtMost(_)
        | Filter::ManaValueAtMostX => {
            unreachable!("the card table refuses a filter that is not of a spell's own")
        }
    }
}

/// Whether the loyalty abilities of the permanent in `slot` may be activated any time its
/// controller could cast an instant, where
/// [CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063) has "during a main phase
/// of their turn": by a static ability of its own whose condition holds
/// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041): "Static abilities do
/// something all the time rather than being activated or triggered."). With no such rule
/// change among the game's cards one test, and with none on this permanent two.
///
/// No effect makes one: the card table refuses an ability whose effect would start a rule
/// change that is asked of a permanent.
#[inline]
pub(crate) fn loyalty_as_instant<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let of_kind = t.rules_of(RuleKind::LoyaltyAsInstant);
    if of_kind.is_empty() {
        return false;
    }
    debug_assert!(
        (s.effect_rules_on(s.object_ref(slot)).common(of_kind)).is_empty(),
        "an effect that changes a rule asked of a permanent"
    );
    let own = s.copiable(slot).rules(t).common(of_kind);
    own.iter().any(|id| {
        // Most conditions are read of the permanent, by the rule change's number; any
        // other is read in a scene, for the ability that has it.
        let holds = match t.rule_condition(id) {
            None => true,
            Some(condition) => interpret::without_a_scene(s, slot, condition)
                .unwrap_or_else(|| in_a_scene(s, t, slot, id)),
        };
        #[cfg(feature = "coverage")]
        if holds && let Some((_, ability)) = maker_of(s, t, slot, id) {
            crate::coverage::hit(ability);
        }
        holds
    })
}

/// The static ability of the permanent in `slot` that makes the rule change `id`, and its
/// place on the face that is up.
fn maker_of<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    id: RuleId,
) -> Option<(AbilityIx, &'static Ability)> {
    let rule = t.rule(id);
    let abilities = s.copiable(slot).face(t).abilities;
    (0u8..).zip(abilities).find_map(|(index, ability)| {
        let makes = ability.kind == AbilityKind::Static
            && ability.rule.is_some_and(|named| std::ptr::eq(named, rule));
        makes.then_some((AbilityIx::new(index), ability))
    })
}

/// Whether the condition of the static ability of the permanent in `slot` that makes the
/// rule change `id` holds, read in a scene.
#[cold]
fn in_a_scene<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot, id: RuleId) -> bool {
    maker_of(s, t, slot, id).is_some_and(|(index, ability)| {
        interpret::static_condition_holds(s, t, slot, index, ability)
    })
}

/// The first permanent with a static ability that has a rule change of `kind` govern
/// `player`'s casting the spell `face` would be, if one does and not a resolved effect
/// alone. "Each opponent" and "you" in the ability are read for the permanent's controller
/// after continuous effects
/// ([CR 109.5](https://yawgatog.com/resources/magic-rules/#R1095)): "For a static ability,
/// this is the current controller of the object it's on."
pub(crate) fn maker<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    kind: RuleKind,
    player: Seat,
    face: &Face,
) -> Option<L::Slot> {
    let of_kind = t.rules_of(kind);
    // A permanent's derived characteristics, its controller among them, are at its place
    // in the battlefield list.
    s.permanents()
        .find(|&(slot, d)| {
            let made = s.copiable(slot).player_rules(t).common(of_kind);
            made.iter().any(|id| {
                let rule = t.rule(id);
                filter::is_for(&rule.players, player, d.controller)
                    && accepts(spells_of(rule), face)
            })
        })
        .map(|(slot, _)| slot)
}
