//! Reflexive triggered abilities ([`Effect::Reflexive`]; CR 603.12): a test-only artifact with
//! "{T}: Exile target card from a graveyard. When a creature card is exiled this way, put a
//! +1/+1 counter on target creature you control." The second sentence triggers as the first
//! resolves, goes on the stack the next time a player would receive priority, and chooses
//! its target only then.

use beryl_card::effect::build::{add_counters, exile, reflexive, sequence, target, those};
use beryl_card::filter::Filter;
use beryl_card::phrases::creature_you_control;
use beryl_card::target::{TargetKind, TargetSpec};
use beryl_card::{
    Ability, CardDef, Condition, Cost, CounterKind, Effect, Face, ObjectAbility, Trigger, Types,
};

use super::LAND;
use super::cast::{BEAR, Game};
use crate::decision::DecisionKind;
use crate::ids::Slot;
use crate::state::{StackItem, Zone};
use crate::{CardTable, Decklist, Opt, Profile};

/// "{T}: Exile target card. When a creature card is exiled this way, ..."
static EXILE_AND_REFLECT: Effect = sequence(&[
    exile(target()),
    reflexive(Condition::Is(those(), Filter::Types(Types::CREATURE)), 1),
]);

/// "{T}: Exile target card from a graveyard. When a creature card is exiled this way, put a
/// +1/+1 counter on target creature you control."
static URN: CardDef = CardDef {
    name: "Test Urn",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::activated(Cost::TAP)
                .target(&TargetSpec::one(TargetKind::GraveyardCard, Filter::Any))
                .effect(&EXILE_AND_REFLECT),
            Ability::triggered(&Trigger::REFLEXIVE)
                .target(&TargetSpec::permanent(creature_you_control()))
                .effect(&add_counters(target(), CounterKind::PlusOne, 1)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &URN]);
const DECK: &Decklist = &[(20, "Test Land"), (10, "Test Bear"), (10, "Test Urn")];

/// A game in the starting player's precombat main phase with a Test Urn of theirs on the
/// battlefield. Each player has a land in hand, so every main phase of theirs is a decision
/// and the game stops there.
fn game() -> (Game, Slot) {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    for player in [g.me, g.them] {
        g.give(player, "Test Land", Zone::Hand);
    }
    let urn = g.give(g.me, "Test Urn", Zone::Battlefield);
    (g, urn)
}

/// Activates `urn`'s ability targeting `card` and has both players pass, so it resolves.
fn exile_with(g: &mut Game, urn: Slot, card: Slot) {
    g.pick(Opt::Activate {
        slot: urn,
        ability: ObjectAbility::own(0),
    });
    if g.current().kind == DecisionKind::Target {
        g.pick_object(card);
    }
    g.pass_both();
}

/// Whether the top of the stack is the urn's reflexive ability.
fn reflexive_on_top(g: &Game) -> bool {
    matches!(
        g.s.stack().last().map(|entry| entry.item),
        Some(StackItem::Triggered { of, .. }) if of.ability == crate::state::TriggeredAbility::own(beryl_card::AbilityIx::new(1))
    )
}

#[test]
fn a_creature_card_exiled_triggers_and_its_target_is_chosen_then() {
    let (mut g, urn) = game();
    let first = g.give(g.me, "Test Bear", Zone::Battlefield);
    let second = g.give(g.me, "Test Bear", Zone::Battlefield);
    // Two Bears alike would be one choice (`engine::collapse`).
    g.s.board().set_tapped(first, true);
    let dead = g.give(g.them, "Test Bear", Zone::Graveyard);
    exile_with(&mut g, urn, dead);
    assert_eq!(g.s.object(dead).zone(), Zone::Exile);
    // CR 603.12: it triggered during the resolution and chooses its target as it goes on the
    // stack, after the ability that made it has left.
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::Target, "{d:?}");
    assert_eq!(d.player, g.me);
    g.pick_object(second);
    assert!(reflexive_on_top(&g));
    assert_eq!(g.s.stack().len(), 1);
    g.pass_both();
    assert!(g.s.stack().is_empty());
    assert_eq!(g.s.object(second).counters().plus_one, 1);
    assert_eq!(g.s.object(first).counters().plus_one, 0);
}

#[test]
fn a_noncreature_card_exiled_triggers_nothing() {
    let (mut g, urn) = game();
    let bear = g.give(g.me, "Test Bear", Zone::Battlefield);
    let land = g.give(g.them, "Test Land", Zone::Graveyard);
    exile_with(&mut g, urn, land);
    assert_eq!(g.s.object(land).zone(), Zone::Exile);
    assert!(g.s.stack().is_empty());
    assert!(g.s.triggers().is_empty());
    assert_eq!(g.decide().kind, DecisionKind::Priority);
    assert_eq!(g.s.object(bear).counters().plus_one, 0);
}

#[test]
fn the_opponent_may_respond_between_the_exile_and_the_counter() {
    let (mut g, urn) = game();
    let bear = g.give(g.me, "Test Bear", Zone::Battlefield);
    let dead = g.give(g.them, "Test Bear", Zone::Graveyard);
    // The opponent has something to respond with: an Urn of their own and a card to exile.
    let their_urn = g.give(g.them, "Test Urn", Zone::Battlefield);
    let land = g.give(g.me, "Test Land", Zone::Graveyard);
    exile_with(&mut g, urn, dead);
    // The only legal target is chosen without asking.
    assert!(reflexive_on_top(&g));
    assert_eq!(g.s.object(bear).counters().plus_one, 0);
    // A player who could only pass is not given priority: the controller's Urn is tapped.
    let d = g.decide();
    assert_eq!((d.kind, d.player), (DecisionKind::Priority, g.them));
    assert!(reflexive_on_top(&g));
    // The opponent exiles the land in response, above the trigger.
    g.pick(Opt::Activate {
        slot: their_urn,
        ability: ObjectAbility::own(0),
    });
    if g.current().kind == DecisionKind::Target {
        g.pick_object(land);
    }
    assert_eq!(g.s.stack().len(), 2);
    g.pass_both();
    assert_eq!(g.s.object(land).zone(), Zone::Exile);
    assert!(reflexive_on_top(&g));
    assert_eq!(g.s.object(bear).counters().plus_one, 0);
    g.pass_both();
    assert!(g.s.stack().is_empty());
    assert_eq!(g.s.object(bear).counters().plus_one, 1);
}

#[test]
fn with_no_creature_to_target_the_trigger_leaves_the_stack() {
    let (mut g, urn) = game();
    let theirs = g.give(g.them, "Test Bear", Zone::Battlefield);
    let dead = g.give(g.them, "Test Bear", Zone::Graveyard);
    exile_with(&mut g, urn, dead);
    assert_eq!(g.s.object(dead).zone(), Zone::Exile);
    // CR 603.3d: no legal target, so it is removed from the stack.
    assert!(g.s.stack().is_empty());
    assert!(g.s.triggers().is_empty());
    assert_eq!(g.decide().kind, DecisionKind::Priority);
    assert_eq!(g.s.object(theirs).counters().plus_one, 0);
}

/// It renders as one paragraph, the reflexive ability inside the activated one's.
#[test]
fn it_renders_as_a_sentence_of_the_ability_that_makes_it() {
    let text = beryl_card::render::render(&URN);
    assert!(
        text.contains(
            "{T}: Exile target card. When a creature card is exiled this way, put a +1/+1 counter on target creature you control."
        ),
        "{text}"
    );
}
