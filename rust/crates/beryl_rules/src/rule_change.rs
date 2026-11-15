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
use beryl_kernel::chosen::ChosenValue;

use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityIx, AbilityKind, Face, ObjectAbility, Words};
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
        Governed::ActivateLoyaltyAbilities(_)
        | Governed::Block(_)
        | Governed::ActivateAbilities { .. } => unreachable!("a rule change about casting"),
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
        // A rule in force is known by its card, not by the permanent whose text says it, and
        // the spell is not yet an object, so both read as printed.
        Filter::SubtypeWord(word) => face
            .subtypes
            .read(&Words::NONE)
            .contains(word.read(&Words::NONE)),
        Filter::ColorWords(colors) => face.colors.intersects(colors.read(&Words::NONE)),
        Filter::Not(filter) => !accepts_composed(*filter, face),
        Filter::All(filters) => filters.iter().all(|&filter| accepts_composed(filter, face)),
        Filter::AnyOf(filters) => filters.iter().any(|&filter| accepts_composed(filter, face)),
        Filter::Whose(_)
        | Filter::Tapped
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX => {
            unreachable!("the card table refuses a filter that is not of a spell's own")
        }
    }
}

/// The rule changes in force that keep `player` from activating the activated abilities,
/// other than mana abilities, of some sources, as "Activated abilities of sources with the
/// chosen name can't be activated unless they're mana abilities." does ([CR 602.5]): "A
/// player can't begin to activate an ability that's prohibited from being activated." A
/// prohibition beats a permission ([CR 101.2]): "When a rule or effect allows or directs
/// something to happen, and another effect states that it can't happen, the "can't" effect
/// takes precedence." While none is in force, one AND and nothing else to ask; while one is,
/// [`named_by_a_maker`] says which sources it stops.
///
/// [CR 602.5]: https://yawgatog.com/resources/magic-rules/#R6025
/// [CR 101.2]: https://yawgatog.com/resources/magic-rules/#R1012
#[inline]
pub(crate) fn prohibitions<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Rules {
    // The table's set first: in a game whose cards make no such change, the state is not read.
    let of_kind = t.rules_of(RuleKind::CantActivateNamed);
    if of_kind.is_empty() {
        return Rules::NONE;
    }
    s.rules_for(player).common(of_kind)
}

/// Of `objects`, those whose activated abilities `player` may activate as far as `rules`
/// ([`prohibitions`]) say: those no maker of one of them named ([`named_by_a_maker`]). Out of
/// line, so that the list of activations is the same code while no such rule is in force.
#[cold]
#[inline(never)]
pub(crate) fn unprohibited<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rules: Rules,
    player: Seat,
    objects: Bitboard<L::Slot>,
) -> Bitboard<L::Slot> {
    objects.filter(|slot| !named_by_a_maker(s, t, rules, player, slot))
}

/// Whether a permanent whose static ability makes one of `rules` ([`prohibitions`]) for
/// `player` chose the name of the object in `slot`, so that `player` can't activate its
/// activated abilities other than mana abilities. A rule change is numbered once for the
/// game, and what it stops depends on the name each permanent that makes it chose, so the
/// makers are found and their names read. The source's name is read from its copiable
/// values ([CR 707.2]): a token copy of a card has the card's name, and a face-down permanent
/// has none ([CR 708.2a]): it "becomes a 2/2 face-down creature with no text, no name, no
/// subtypes, and no mana cost." A permanent that chose "another card" names no card of the
/// game. "Each opponent" and "you" are read for the
/// maker's controller ([CR 109.5]): "For a static ability, this is the current controller of
/// the object it's on."
///
/// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
/// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
fn named_by_a_maker<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rules: Rules,
    player: Seat,
    slot: L::Slot,
) -> bool {
    let name = s.definition(slot).name(t);
    if name.is_empty() {
        return false;
    }
    s.permanents().any(|(maker, d)| {
        let Some(ChosenValue::Name(Some(card))) = s.chosen(maker) else {
            return false;
        };
        let made = s.definition(maker).player_rules(t).common(rules);
        t.def(card).name == name
            && made.iter().any(|id| {
                filter::is_for(&t.rule(id).players, player, d.controller)
                    && maker_of(s, t, maker, id).is_some()
            })
    })
}

/// Whether the loyalty abilities of the permanent in `slot` may be activated any time its
/// controller could cast an instant, where [CR 606.3] has "during a main phase of their
/// turn": by a static ability of its own whose condition holds ([`own_rule_holds`]).
///
/// [CR 606.3]: https://yawgatog.com/resources/magic-rules/#R6063
#[inline]
pub(crate) fn loyalty_as_instant<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    own_rule_holds(s, t, slot, RuleKind::LoyaltyAsInstant)
}

/// Whether the creature in `slot` can't block, as "This creature can't block." says
/// ([CR 509.1a]): "The defending player chooses which creatures they control, if any, will
/// block." A prohibition beats a permission ([CR 101.2]): "When a rule or effect allows or
/// directs something to happen, and another effect states that it can't happen, the
/// "can't" effect takes precedence." Made by a static ability of its own whose condition
/// holds ([`own_rule_holds`]).
///
/// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
/// [CR 101.2]: https://yawgatog.com/resources/magic-rules/#R1012
#[inline]
pub(crate) fn cant_block<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    own_rule_holds(s, t, slot, RuleKind::CantBlock)
}

/// Whether a static ability of the permanent in `slot` whose condition holds makes a rule
/// change of `kind`, one asked of the permanent with the ability ([CR 604.1]): "Static
/// abilities do something all the time rather than being activated or triggered." With no
/// such rule change among the game's cards one test, and with none on this permanent two.
///
/// No effect makes one: the card table refuses an ability whose effect would start a rule
/// change that is asked of a permanent.
///
/// [CR 604.1]: https://yawgatog.com/resources/magic-rules/#R6041
#[inline]
fn own_rule_holds<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot, kind: RuleKind) -> bool {
    let of_kind = t.rules_of(kind);
    if of_kind.is_empty() {
        return false;
    }
    debug_assert!(
        (s.effect_rules_on(s.object_ref(slot)).common(of_kind)).is_empty(),
        "an effect that changes a rule asked of a permanent"
    );
    let own = s.definition(slot).rules(t).common(of_kind);
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
    // The card table refuses a card that grants a rule change (`TableError::Granted`), so
    // the maker is one of the permanent's own abilities.
    s.abilities(t, slot).find_map(|(index, ability)| {
        let ObjectAbility::Own(index) = index else {
            return None;
        };
        let makes = ability.kind() == AbilityKind::Static
            && ability
                .rule()
                .is_some_and(|named| std::ptr::eq(named, rule));
        makes.then_some((index, ability))
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
            let made = s.definition(slot).player_rules(t).common(of_kind);
            made.iter().any(|id| {
                let rule = t.rule(id);
                filter::is_for(&rule.players, player, d.controller)
                    && accepts(spells_of(rule), face)
            })
        })
        .map(|(slot, _)| slot)
}
