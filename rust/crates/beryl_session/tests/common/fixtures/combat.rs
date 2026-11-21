//! Combat positions used to exercise the session’s attack and block scripts.

use beryl_cards::props::Prop;
use beryl_cards::{names, tokens};
use beryl_scenario::prelude::{
    DecisionKind, ME, P0, P1, Pick, Resolve, Scenario, THEM, TurnStep, Zone, advance_to, stack,
};

pub const ADELINE_ATTACKS_PLANESWALKER: &Scenario =
    &Scenario::new("Adeline's Human attacks a planeswalker")
        .battlefield(P0, &[names::ADELINE_RESPLENDENT_CATHAR])
        .battlefield(P1, &[Prop::Planeswalker])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::ADELINE_RESPLENDENT_CATHAR),
            ME.answers(DecisionKind::AttackTarget, Pick::DefendingPlayer),
            ME.chooses(DecisionKind::AttackTarget, Prop::Planeswalker),
            ME.attacking(tokens::HUMAN_TOKEN, 1),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(18),
            THEM.has_loyalty(Prop::Planeswalker, 2),
        ]);

pub const ADELINE_TOKEN: &Scenario = &Scenario::new("Adeline's Human arrives attacking")
    .battlefield(P0, &[names::ADELINE_RESPLENDENT_CATHAR, Prop::Flier])
    .actions(&[
        ME.pt(names::ADELINE_RESPLENDENT_CATHAR, 2, 4),
        advance_to(1, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::ADELINE_RESPLENDENT_CATHAR),
        ME.done_attacking(),
        stack(1),
        Resolve,
        ME.attacking(tokens::HUMAN_TOKEN, 1),
        ME.tapped(tokens::HUMAN_TOKEN, 1),
        ME.tapped(names::ADELINE_RESPLENDENT_CATHAR, 0),
        ME.pt(names::ADELINE_RESPLENDENT_CATHAR, 3, 4),
        advance_to(1, TurnStep::EndOfCombat),
        THEM.life(16),
    ]);

pub const EMPEROR_EXILES_ATTACKER: &Scenario =
    &Scenario::new("flashed in during combat, −2 exiles the tapped attacker")
        .at(2, P1, TurnStep::BeginningOfCombat)
        .hand(P0, &[names::THE_WANDERING_EMPEROR])
        .battlefield(P0, &[names::PLAINS; 4])
        .battlefield(P1, &[Prop::Vanilla])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .actions(&[
            ME.passes(),
            THEM.attacks_with(Prop::Vanilla),
            ME.casts(names::THE_WANDERING_EMPEROR),
            ME.activates(names::THE_WANDERING_EMPEROR, 3), // -2
            THEM.has(Zone::Exile, Prop::Vanilla, 1),
            ME.life(22),
            ME.has_loyalty(names::THE_WANDERING_EMPEROR, 1),
            advance_to(3, TurnStep::Upkeep),
            ME.deciding(DecisionKind::Priority, 1),
            advance_to(3, TurnStep::PrecombatMain),
            ME.deciding(DecisionKind::Priority, 4),
        ]);

pub const EMPEROR_GROWS_CREATURE: &Scenario =
    &Scenario::new("+1 grows a creature and gives it first strike")
        .battlefield(P0, &[names::THE_WANDERING_EMPEROR])
        .battlefield(P1, &[Prop::Vanilla])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .actions(&[
            ME.activates(names::THE_WANDERING_EMPEROR, 1), // +1
            ME.target_count(1),
            Resolve,
            THEM.pt(Prop::Vanilla, 3, 2),
            ME.has_loyalty(names::THE_WANDERING_EMPEROR, 4),
            advance_to(2, TurnStep::Upkeep),
            THEM.pt(Prop::Vanilla, 3, 2),
        ]);

pub const SPHINX: &Scenario = &Scenario::new("Sphinx dies as a card, then as a token")
    .battlefield(P0, &[names::SPHINX_OF_FALSE_CONCLUSIONS])
    .battlefield(P1, &[Prop::Flier, Prop::Flier])
    .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
    .library(P1, &[names::PLAINS])
    .actions(&[
        advance_to(1, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::SPHINX_OF_FALSE_CONCLUSIONS),
        Resolve,
        ME.has(Zone::Graveyard, names::ISLAND, 1),
        ME.passes(),
        THEM.chooses(DecisionKind::Blocker, Prop::Flier),
        THEM.answers(DecisionKind::Blocker, Pick::Done),
        advance_to(1, TurnStep::CombatDamage),
        Resolve,
        ME.token_pt(names::SPHINX_OF_FALSE_CONCLUSIONS, 4, 2),
        ME.has(Zone::Graveyard, names::SPHINX_OF_FALSE_CONCLUSIONS, 1),
        advance_to(2, TurnStep::BeginningOfCombat),
        THEM.passes(),
        THEM.done_attacking(),
        advance_to(3, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::SPHINX_OF_FALSE_CONCLUSIONS),
        Resolve,
        ME.passes(),
        THEM.chooses(DecisionKind::Blocker, Prop::Flier),
        advance_to(3, TurnStep::EndOfCombat),
        ME.controls(names::SPHINX_OF_FALSE_CONCLUSIONS, 0),
        THEM.has(Zone::Graveyard, Prop::Flier, 2),
        THEM.life(20),
    ]);

pub const TEFERI_DIES_IN_COMBAT: &Scenario =
    &Scenario::new("combat damage removes Teferi while other attacks remain")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[Prop::Flier, Prop::Vanilla])
        .battlefield(P1, &[names::TEFERI_TIME_RAVELER, Prop::Planeswalker])
        .library(P1, &[names::PLAINS, names::ISLAND])
        .actions(&[
            THEM.activates(names::TEFERI_TIME_RAVELER, 2), // −3
            THEM.target_count(0),
            THEM.has_loyalty(names::TEFERI_TIME_RAVELER, 1),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(Prop::Flier),
            ME.chooses(DecisionKind::AttackTarget, names::TEFERI_TIME_RAVELER),
            ME.attacks_with(Prop::Vanilla),
            ME.answers(DecisionKind::AttackTarget, Pick::DefendingPlayer),
            advance_to(3, TurnStep::EndOfCombat),
            THEM.has(Zone::Graveyard, names::TEFERI_TIME_RAVELER, 1),
            THEM.has_loyalty(Prop::Planeswalker, 3),
            THEM.life(18),
        ]);

pub const TWO_PLANESWALKERS_ATTACKED: &Scenario =
    &Scenario::new("combat damage removes Teferi while the other planeswalker is attacked")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[Prop::Flier, Prop::Vanilla])
        .battlefield(P1, &[names::TEFERI_TIME_RAVELER, Prop::Planeswalker])
        .library(P1, &[names::PLAINS, names::ISLAND])
        .actions(&[
            THEM.activates(names::TEFERI_TIME_RAVELER, 2), // −3
            THEM.target_count(0),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(Prop::Flier),
            ME.chooses(DecisionKind::AttackTarget, names::TEFERI_TIME_RAVELER),
            ME.attacks_with(Prop::Vanilla),
            ME.chooses(DecisionKind::AttackTarget, Prop::Planeswalker),
            advance_to(3, TurnStep::EndOfCombat),
            THEM.has(Zone::Graveyard, names::TEFERI_TIME_RAVELER, 1),
            THEM.has_loyalty(Prop::Planeswalker, 1),
            THEM.life(20),
        ]);
