//! Rule changes: what the rules ask of the rule changes in force, whether a static ability
//! or a resolved effect makes them
//! (see [CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some continuous
//! effects affect game rules rather than objects."
//!
//! A rule change is data (`beryl_card`'s `RuleChange`), which a game numbers
//! (`beryl_kernel`'s `rule_changes`). The set in force for a player is kept current with
//! the layers ([`crate::continuous::layers`]) and by the Effects store, and the card table has the set
//! of each kind a check here asks about, so asking while none of that kind is in force is
//! one AND and walks nothing. Only when one is does a check read what it says: the spells
//! its filter accepts, or its condition. Which permanent makes a change is found only to
//! tell a player why.
//!
//! Implements: none. Reads rule changes after continuous effects have applied.

use beryl_card::{Governed, ManaCost, PlayerRules, RuleChange, SpellCost};

use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityKind, Face, ObjectAbility, Words};
use crate::filter::{self, Evaluate, Filter};
use crate::ids::Seat;
use crate::interpret;
use crate::layout::Layout;
use crate::state::{RuleId, RuleKind, Rules, State};
use crate::table::{Capability, GameTable};

/// Whether a rule change in force has `player` cast the spell `face` would be only when
/// they could cast a sorcery, as a static ability "Each opponent can cast spells only any
/// time they could cast a sorcery." does each opponent. A prohibition beats a permission
/// (see [CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)): "When a rule or effect
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
/// does (see [CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)).
#[inline]
pub(crate) fn cast_as_instant<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
) -> bool {
    governs_cast(s, t, RuleKind::CastAsInstant, player, face)
}

/// Whether `player`'s life total can change: no rule change in force says it can't, as "Until
/// your next turn, your life total can't change" does (see [CR 119.7], see [CR 119.8]). One AND while
/// no card in the game makes a change to players. Every gain and loss of life and every
/// payment of life asks here.
///
/// [CR 119.7]: https://yawgatog.com/resources/magic-rules/#R1197
/// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
#[inline]
pub(crate) fn life_can_change<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> bool {
    !player_rule(s, t, player, PlayerRules::LIFE_CANT_CHANGE)
}

/// Whether `player` has protection from everything (see [CR 702.16j]): "Such a permanent or player
/// can't be targeted by spells or abilities and can't be enchanted by Auras. ... All damage
/// that would be dealt to such a permanent or player is prevented." One AND while no card in
/// the game makes a change to players.
///
/// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
#[inline]
pub(crate) fn protected_from_everything<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> bool {
    player_rule(s, t, player, PlayerRules::PROTECTION_FROM_EVERYTHING)
}

/// The player's hand limit for cleanup (see [CR 514.1]): "if the active player's hand
/// contains more cards than their maximum hand size (normally seven), they discard
/// enough cards to reduce their hand size to that number."
///
/// [CR 514.1]: https://yawgatog.com/resources/magic-rules/#R5141
#[inline]
pub(crate) fn maximum_hand<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> u32 {
    if player_rule(s, t, player, PlayerRules::NO_MAXIMUM_HAND_SIZE) {
        u32::MAX
    } else {
        t.format().maximum_hand()
    }
}

/// Whether a rule change in force for `player` makes the change `which` to them. The card
/// table's set of changes to players is read first, so a game whose cards make none reads
/// nothing of the state.
#[inline]
fn player_rule<L: Layout>(s: &State<L>, t: &GameTable, player: Seat, which: PlayerRules) -> bool {
    let of_kind = t.rules_of(RuleKind::Player);
    !of_kind.is_empty() && makes(t, s.rules_for(player).common(of_kind), which)
}

/// Whether one of `rules`, which change players, makes the change `which`. Out of line: only a
/// game with such a change in force comes here.
#[inline(never)]
fn makes(t: &GameTable, rules: Rules, which: PlayerRules) -> bool {
    rules
        .iter()
        .any(|id| matches!(t.rule(id).what, Governed::Player(changes) if changes.contains(which)))
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

/// Whether the active player skips the given step under a rule in force
/// (see [CR 500.11]): "To skip a step, phase, or turn is to proceed past it as though
/// it didn’t exist."
///
/// [CR 500.11]: https://yawgatog.com/resources/magic-rules/#R50011
pub(crate) fn skips_step<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    step: crate::state::TurnStep,
) -> bool {
    let active = s.turn().active();
    s.rules_for(active)
        .common(t.rules_of(RuleKind::SkipStep))
        .iter()
        .any(|id| matches!(t.rule(id).what, Governed::TakeStep(at) if at == step))
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
        | Governed::UntapInUntapStep(_)
        | Governed::BlockAnyNumber(_)
        | Governed::Attack { .. }
        | Governed::ActivateAbilities { .. }
        | Governed::Counter(_)
        | Governed::SpellCosts { .. }
        | Governed::PreventDamage
        | Governed::TakeStep(_)
        | Governed::Player(_) => unreachable!("a rule change about casting"),
    }
}

/// Whether the spell `face` would be passes `filter` by its own characteristics: all that
/// is read of a spell a player has yet to cast. The card table has only filters read this
/// way (`beryl_kernel`'s `rule_changes::RuleKind::of`).
#[inline]
pub(crate) fn accepts(filter: Filter, face: &Face) -> bool {
    match filter {
        Filter::Any => true,
        Filter::Types(types) => face.types.intersects(types),
        filter @ (Filter::Supertypes(_)
        | Filter::SubtypeWord(_)
        | Filter::ColorWords(_)
        | Filter::Whose(_)
        | Filter::Owned(_)
        | Filter::AttackedThisTurn
        | Filter::Tapped
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::Not(_)
        | Filter::All(_)
        | Filter::AnyOf(_)
        | Filter::InZone(_)) => accepts_composed(filter, face),
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
        | Filter::Owned(_)
        | Filter::AttackedThisTurn
        | Filter::Tapped
        | Filter::Token
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::InZone(_) => {
            unreachable!("the card table refuses a filter that is not of a spell's own")
        }
    }
}

/// The rule changes in force that keep `player` from activating the activated abilities,
/// other than mana abilities, of some sources, as "Activated abilities of sources with the
/// chosen name can't be activated unless they're mana abilities." does (see [CR 602.5]): "A
/// player can't begin to activate an ability that's prohibited from being activated." A
/// prohibition beats a permission (see [CR 101.2]): "When a rule or effect allows or directs
/// something to happen, and another effect states that it can't happen, the "can't" effect
/// takes precedence." While none is in force, one AND and nothing else to ask; while one is,
/// [`unprohibited`] says which sources it stops.
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
/// ([`prohibitions`]) say. Find the permanents that named a card once for all candidate
/// sources. A rule change is numbered once for the game, so each permanent that makes it
/// can choose a different name.
///
/// A source's names come from its copiable values (see [CR 707.2]). For a split card
/// (see [CR 709.4a]), the object "has the chosen name if one of its names is the chosen name."
/// A face-down permanent
/// "becomes a 2/2 face-down creature with no text, no name, no subtypes, and no mana cost"
/// (see [CR 708.2a]). The choice of a card outside this game has no name to match here.
/// "Each opponent" and "you" are read for the maker's controller (see [CR 109.5]): "For a
/// static ability, this is the current controller of the object it's on."
///
/// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
/// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
/// [CR 709.4a]: https://yawgatog.com/resources/magic-rules/#R7094a
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
#[cold]
#[inline(never)]
pub(crate) fn unprohibited<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rules: Rules,
    player: Seat,
    objects: Bitboard<L::Slot>,
) -> Bitboard<L::Slot> {
    if objects.is_empty() {
        return objects;
    }
    let mut allowed = objects;
    for (maker, name) in s.chosen_names(t) {
        let made = player_rules_of(s, t, maker).common(rules);
        if made.iter().any(|id| {
            filter::is_for(&t.rule(id).players, player, s.derived(maker).controller)
                && maker_of(s, t, maker, id).is_some()
        }) {
            allowed = allowed.filter(|slot| !s.copiable(slot).has_name(t, name));
        }
    }
    allowed
}

/// Whether the loyalty abilities of the permanent in `slot` may be activated any time its
/// controller could cast an instant, through a static ability of its own whose condition
/// holds ([`own_rule_holds`]). The usual timing is "during a main phase of their turn"
/// (see [CR 606.3]).
///
/// [CR 606.3]: https://yawgatog.com/resources/magic-rules/#R6063
#[inline]
pub(crate) fn loyalty_as_instant<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    own_rule_holds(s, t, slot, RuleKind::LoyaltyAsInstant)
}

/// Whether the creature in `slot` can't block, as "This creature can't block." says
/// (see [CR 509.1a]): "The defending player chooses which creatures they control, if any, will
/// block." A prohibition beats a permission (see [CR 101.2]): "When a rule or effect allows or
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

/// Whether a card of this game allows a blocker to choose more than one attacker (see CR 509.1a).
#[inline]
pub(crate) fn multiple_blocks_possible(t: &GameTable) -> bool {
    !t.rules_of(RuleKind::BlockAnyNumber).is_empty()
}

/// Whether the creature's static ability lets it block more than one attacker (see CR 509.1a).
#[cold]
pub(crate) fn can_block_any_number<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let of_kind = t.rules_of(RuleKind::BlockAnyNumber);
    if of_kind.is_empty() {
        return false;
    }
    object_rules(s, t, slot).common(of_kind).iter().any(|id| {
        (!s.abilities_may_be_lost() || maker_of(s, t, slot, id).is_some())
            && own_condition_holds(s, t, slot, id)
    })
}

/// Whether the creature in `slot` can't attack, because it can attack no defending player and
/// no planeswalker one controls, as "This creature can't attack unless defending player
/// controls an Island." says of a defending player with no Island (see [CR 508.1c]): "The
/// active player checks each creature they control to see whether it's affected by any
/// restrictions (effects that say a creature can't attack, or that it can't attack unless
/// some condition is met)." Made by a static ability of its own whose condition holds. Asked
/// only in a game whose cards can say it ([`attacks_restricted`]).
///
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
#[cold]
#[inline(never)]
pub(crate) fn cant_attack<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let of_kind = t.rules_of(RuleKind::CantAttack);
    !(s.defending_players()).any(|defending| !attack_forbidden(s, t, slot, defending, of_kind))
}

/// Whether some card of the game restricts attacking ([`cant_attack`]): one test, so that a
/// declaration of attackers in a game with none asks nothing of each creature.
#[inline]
pub(crate) fn attacks_restricted(t: &GameTable) -> bool {
    !t.rules_of(RuleKind::CantAttack).is_empty()
}

/// Whether the creature in `slot` can't attack `defending` ([`cant_attack`]). The permanents
/// the defending player must control are read for the creature's controller, through the
/// words of the creature's text (see [CR 612.1]): "This can apply to any words or symbols printed
/// on that object, but generally affects only that object’s rules text". So a creature whose
/// "Island" reads as "Swamp" can attack only a player who controls a Swamp.
///
/// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
fn attack_forbidden<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    defending: Seat,
    of_kind: Rules,
) -> bool {
    let own = object_rules(s, t, slot).common(of_kind);
    own.iter().any(|id| {
        let Governed::Attack {
            unless_defender_controls,
            ..
        } = t.rule(id).what
        else {
            unreachable!("a rule change of the kind that forbids attacking");
        };
        let allowed = unless_defender_controls.is_some_and(|filter| {
            let you = s.controller(slot);
            let words = s.words_of(slot);
            s.permanents_with_derived().any(|(permanent, d)| {
                d.controller == defending && filter.matches(s, t, permanent, you, &words)
            })
        });
        !allowed && own_condition_holds(s, t, slot, id)
    })
}

/// Whether the spell `spell` can't be countered (see [CR 701.6a]): "To counter a spell or ability
/// means to cancel it, removing it from the stack." A prohibition beats what would counter
/// it (see [CR 101.2]): "When a rule or effect allows or directs something to happen, and another
/// effect states that it can't happen, the "can't" effect takes precedence." Its own static
/// ability may say so, or the mana spent on it (see [CR 106.6]): "Some spells or abilities that
/// produce mana ... have an additional effect that affects the spell or ability that mana is
/// spent on". One test while no card of the game says it.
///
/// [CR 701.6a]: https://yawgatog.com/resources/magic-rules/#R7016a
/// [CR 101.2]: https://yawgatog.com/resources/magic-rules/#R1012
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
#[inline]
pub(crate) fn cant_be_countered<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spell: crate::state::ObjectRef<L>,
) -> bool {
    let of_kind = t.rules_of(RuleKind::CantBeCountered);
    if of_kind.is_empty() {
        return false;
    }
    countered_never(s, t, spell, of_kind)
}

/// [`cant_be_countered`] in a game whose cards can say it: whether the spell is on the stack
/// and an effect on it, or a static ability of its own whose condition holds, says it.
#[cold]
#[inline(never)]
fn countered_never<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spell: crate::state::ObjectRef<L>,
    of_kind: Rules,
) -> bool {
    let on_stack = s
        .spell(spell)
        .and_then(|spell| s.spell_entry(spell))
        .is_some();
    if !on_stack {
        return false;
    }
    if !s.effect_rules_on(spell).common(of_kind).is_empty() {
        return true;
    }
    let own = s.definition(spell.slot).rules(t).common(of_kind);
    own.iter().any(|id| match t.rule_condition(id) {
        None => true,
        Some(condition) => interpret::without_a_scene(s, spell.slot, condition)
            .unwrap_or_else(|| in_a_scene(s, t, spell.slot, id)),
    })
}

/// Whether a static ability of the permanent in `slot` whose condition holds makes a rule
/// change of `kind`, one asked of the permanent with the ability (see [CR 604.1]): "Static
/// abilities do something all the time rather than being activated or triggered." With no
/// such rule change among the game's cards one test, and with none on this permanent two.
///
/// No effect makes one: the card table refuses an ability whose effect would start a rule
/// change that is asked of a permanent.
///
/// [CR 604.1]: https://yawgatog.com/resources/magic-rules/#R6041
// Out of line, as the compiler had it before a second rule kind was asked of an object: inlined
// into the decision builder and the blocker check it costs the rollout benchmark more.
#[inline(never)]
pub(crate) fn own_rule_holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    kind: RuleKind,
) -> bool {
    let of_kind = t.rules_of(kind);
    if of_kind.is_empty() {
        return false;
    }
    debug_assert!(
        (s.effect_rules_on(s.object_ref(slot)).common(of_kind)).is_empty(),
        "an effect that changes a rule asked of a permanent"
    );
    let own = object_rules(s, t, slot).common(of_kind);
    own.iter().any(|id| own_condition_holds(s, t, slot, id))
}

/// The rule changes the static abilities of the permanent in `slot` make: those of the face
/// that is up and, of a merged permanent, those of the cards under it (see [CR 702.140e]): "A
/// mutated permanent has all abilities of each card and token that represents it." The faces
/// under it are read only in a game with a card that merges.
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
#[inline]
fn object_rules<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Rules {
    let own = s.definition(slot).rules(t);
    if t.can(Capability::Merges) {
        own.union(merged_rules(s, t, slot))
    } else {
        own
    }
}

/// The rule changes the static abilities of the cards under the merged permanent in `slot`
/// make, if it is one and has their abilities ([`object_rules`]). Out of line: only a game with
/// a card that merges comes here.
#[cold]
#[inline(never)]
fn merged_rules<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Rules {
    s.merged_faces_now(t, slot)
        .map(|(card, face)| crate::definition::Definition::<L>::of_face(card, face).rules(t))
        .fold(Rules::NONE, Rules::union)
}

/// The rule changes for players among those the static abilities of the permanent in `slot`
/// make ([`object_rules`]).
#[inline]
fn player_rules_of<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Rules {
    let own = s.definition(slot).player_rules(t);
    if t.can(Capability::Merges) {
        own.union(merged_rules(s, t, slot).common(t.player_rules()))
    } else {
        own
    }
}

/// Whether the condition of the static ability of the permanent in `slot` that makes the
/// rule change `id` for its own permanent holds, or it has none.
fn own_condition_holds<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot, id: RuleId) -> bool {
    // Most conditions are read of the permanent, by the rule change's number; any other is
    // read in a scene, for the ability that has it.
    let holds = match t.rule_condition(id) {
        None => true,
        Some(condition) => interpret::without_a_scene(s, slot, condition)
            .unwrap_or_else(|| in_a_scene(s, t, slot, id)),
    };
    #[cfg(debug_assertions)]
    if holds && let Some((_, ability)) = maker_of(s, t, slot, id) {
        crate::coverage::hit(ability);
    }
    holds
}

/// `cost`, the total cost of the spell `face` would be before the rule changes in force
/// ([`crate::cast`]'s `spell_cost`), as those that change what casting it costs `player`
/// leave it, as "Noncreature spells cost {1} more to cast." does. While the game's cards
/// make no such change, one test and `cost` as it was.
#[inline]
pub(crate) fn spell_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
    cost: ManaCost,
) -> ManaCost {
    let of_kind = t.rules_of(RuleKind::SpellCost);
    if of_kind.is_empty() {
        return cost;
    }
    changed_cost(s, t, of_kind, player, face, cost)
}

/// [`spell_cost`] while the game's cards make a change of what a spell costs: each
/// permanent whose static ability makes one of `rules` for `player`, whose condition holds
/// and whose filter accepts the spell changes it. "Each opponent" and "you" are read for
/// the permanent's controller (see [CR 109.5]): "For a static ability, this is the current
/// controller of the object it's on." Casting applies these changes in order (see [CR 601.2f]): "The
/// total cost is the mana cost or alternative cost (as determined in rule 601.2b), plus all
/// additional costs and cost increases, and minus all cost reductions. If multiple cost
/// reductions apply, the player may apply them in any order. If the mana component of the
/// total cost is reduced to nothing by cost reduction effects, it is considered to be {0}.
/// It can't be reduced to less than {0}." Then the greatest floor, an effect that directly
/// affects the total cost: "Once the total cost is determined, any effects that directly
/// affect the total cost are applied." A reduction and a floor's difference are generic
/// mana, and X counts at its value, as `cost` already has it. Out of line, so that the
/// casting code is the same while no such change is among the game's cards.
///
/// Each permanent's own abilities are searched for the maker ([`maker_of`]), rather than
/// its face's numbered rules ([`own_rule_holds`] reads those): a read of them here changed
/// how the checks every game makes are compiled, and the rollout benchmark counted 0.17%
/// more instructions with it.
///
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
#[cold]
#[inline(never)]
fn changed_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rules: Rules,
    player: Seat,
    face: &Face,
    mut cost: ManaCost,
) -> ManaCost {
    let (mut more, mut less, mut floor) = (0u8, 0u8, 0u8);
    for (slot, d) in s.permanents_with_derived() {
        for id in rules.iter() {
            let rule = t.rule(id);
            let Governed::SpellCosts { spells, change } = rule.what else {
                unreachable!("a rule change about what a spell costs");
            };
            if !(filter::is_for(&rule.players, player, d.controller)
                && accepts_composed(spells, face))
            {
                continue;
            }
            let Some((index, ability)) = maker_of(s, t, slot, id) else {
                continue;
            };
            if ability.condition().is_none()
                || interpret::static_condition_holds(s, t, slot, index, ability)
            {
                #[cfg(debug_assertions)]
                crate::coverage::hit(ability);
                match change {
                    SpellCost::More(n) => more = more.saturating_add(n),
                    SpellCost::Less(n) => less = less.saturating_add(n),
                    SpellCost::AtLeast(n) => floor = floor.max(n),
                }
            }
        }
    }
    cost.generic = cost.generic.saturating_add(more).saturating_sub(less);
    let short = u32::from(floor).saturating_sub(cost.total(0));
    cost.generic = cost
        .generic
        .saturating_add(u8::try_from(short).unwrap_or(u8::MAX));
    cost
}

/// The static ability of the permanent in `slot` that makes the rule change `id`, and its
/// place among the permanent's abilities: one of the face that is up, or of a card under a
/// merged permanent ([`State::abilities`]).
fn maker_of<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    id: RuleId,
) -> Option<(ObjectAbility, &'static Ability)> {
    let rule = t.rule(id);
    // The card table refuses a card that grants a rule change (`TableError::Granted`), so
    // the maker is not a granted ability.
    s.abilities(t, slot).find_map(|(index, ability)| {
        let makes = !matches!(index, ObjectAbility::Granted(_))
            && ability.kind() == AbilityKind::Static
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
/// (see [CR 109.5](https://yawgatog.com/resources/magic-rules/#R1095)): "For a static ability,
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
    s.permanents_with_derived()
        .find(|&(slot, d)| {
            let made = player_rules_of(s, t, slot).common(of_kind);
            made.iter().any(|id| {
                let rule = t.rule(id);
                filter::is_for(&rule.players, player, d.controller)
                    && accepts(spells_of(rule), face)
            })
        })
        .map(|(slot, _)| slot)
}
