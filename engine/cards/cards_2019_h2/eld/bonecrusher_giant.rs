// Generated from the card store by `just card header`: notes go below the end marker.
//! Bonecrusher Giant {2}{R}
//! Creature — Giant, 4/3
//!
//! Whenever this creature becomes the target of a spell, this creature deals 2 damage to that
//! spell's controller.
//!
//! Stomp {1}{R}
//! Instant — Adventure
//!
//! Damage can't be prevented this turn. Stomp deals 2 damage to any target.
// End of the generated card header.

use crate::prelude::*;

static NO_PREVENTION: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::PreventDamage,
    when: When::Never,
};

pub static CARD: CardDef = card_with_shape(
    Card::BonecrusherGiant,
    PrintedShape::BonecrusherGiant,
    &[
        Printed::BonecrusherGiant
            .abilities(&[Ability::triggered(&Trigger::BECOMES_TARGET_OF_SPELL)
                .effect(&deal_damage_to(Players::ControllerOf(that_card()), 2))]),
        Printed::Stomp.abilities(&[Ability::spell().target(&TargetSpec::any_target()).effect(
            &sequence(&[
                change_rule(&NO_PREVENTION, Duration::EndOfTurn),
                deal_damage_to_any_target(2),
            ]),
        )]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GIANT: &str = Card::BonecrusherGiant;
    const BOLT: &str = Card::LightningBolt;
    const COUNTER: &str = Prop::Instant;
    const CIRCLE: &str = Card::CircleOfProtectionRed;
    const MOTHER: &str = Card::MotherOfRunes;
    const MOUNTAIN: &str = Card::Mountain;
    const ISLAND: &str = Card::Island;

    const ADVENTURE: Scenario = Scenario::setup()
        .hand(P0, &[GIANT])
        .battlefield(P0, &[MOUNTAIN; 5]);

    pub const SCENARIOS: &[Scenario] = &[
        ADVENTURE
            .titled("Stomp resolves and Bonecrusher Giant can be cast from exile")
            .actions(&[
                ME.casts_face(GIANT, FaceIx::BACK),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(18),
                ME.has(Zone::Exile, GIANT, 1),
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(GIANT),
                Resolve,
                ME.controls(GIANT, 1),
                ME.has(Zone::Exile, GIANT, 0),
            ]),
        Scenario::new("Bonecrusher Giant can be cast directly as a creature")
            .hand(P0, &[GIANT])
            .battlefield(P0, &[MOUNTAIN; 3])
            .actions(&[ME.casts(GIANT), Resolve, ME.pt(GIANT, 4, 3), THEM.life(20)]),
        ADVENTURE
            .titled("A countered Stomp goes to the graveyard")
            .hand(P1, &[COUNTER])
            .battlefield(P1, &[ISLAND; 2])
            .actions(&[
                ME.casts_face(GIANT, FaceIx::BACK),
                ME.targets_player(THEM),
                THEM.casts(COUNTER),
                ME.has(Zone::Graveyard, GIANT, 1),
                ME.has(Zone::Exile, GIANT, 0),
                THEM.life(20),
            ]),
        ADVENTURE
            .titled("Stomp deals damage through Circle of Protection Red")
            .interaction(&[CIRCLE])
            .battlefield(P1, &[CIRCLE, Card::Plains])
            .actions(&[
                ME.casts_face(GIANT, FaceIx::BACK),
                ME.targets_player(THEM),
                THEM.activates(CIRCLE, 0),
                advance_to(1, TurnStep::BeginningOfCombat),
                THEM.life(18),
            ]),
        Scenario::new("An activated ability targeting Bonecrusher Giant does not trigger it")
            .interaction(&[MOTHER])
            .battlefield(P0, &[GIANT, MOTHER])
            .actions(&[
                ME.activates(MOTHER, 0),
                ME.targets(GIANT),
                Resolve,
                ME.chooses_color(Colors::BLUE),
                ME.life(20),
            ]),
        Scenario::new("Bonecrusher Giant damages the controller of the spell targeting it")
            .interaction(&[BOLT])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[GIANT])
            .battlefield(P1, &[MOUNTAIN])
            .hand(P1, &[BOLT])
            .actions(&[
                THEM.casts(BOLT),
                THEM.targets(GIANT),
                stack(2),
                Resolve,
                THEM.life(18),
                ME.controls(GIANT, 1),
                Resolve,
                ME.has(Zone::Graveyard, GIANT, 1),
            ]),
        Scenario::new("Bonecrusher Giant damages a countered spell's controller")
            .interaction(&[BOLT])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[GIANT, ISLAND, ISLAND])
            .hand(P0, &[COUNTER])
            .battlefield(P1, &[MOUNTAIN])
            .hand(P1, &[BOLT])
            .actions(&[
                THEM.casts(BOLT),
                THEM.targets(GIANT),
                ME.casts(COUNTER),
                THEM.has(Zone::Graveyard, BOLT, 1),
                THEM.life(18),
                ME.controls(GIANT, 1),
            ]),
        ADVENTURE
            .titled("Stomp with an illegal target leaves damage preventable")
            .interaction(&[MOTHER, CIRCLE, BOLT])
            .hand(P0, &[GIANT, BOLT])
            .battlefield(P1, &[MOTHER, CIRCLE, Card::Plains, Prop::Vanilla])
            .actions(&[
                ME.casts_face(GIANT, FaceIx::BACK),
                ME.targets(Prop::Vanilla),
                ME.passes(),
                THEM.activates(MOTHER, 0),
                THEM.targets(Prop::Vanilla),
                Resolve,
                THEM.chooses_color(Colors::RED),
                Resolve,
                ME.has(Zone::Graveyard, GIANT, 1),
                ME.casts(BOLT),
                ME.targets_player(THEM),
                THEM.activates(CIRCLE, 0),
                advance_to(1, TurnStep::BeginningOfCombat),
                THEM.life(20),
            ]),
        ADVENTURE
            .titled("Damage can be prevented again after Stomp's turn")
            .interaction(&[CIRCLE, BOLT])
            .hand(P0, &[GIANT, BOLT])
            .battlefield(P1, &[CIRCLE, Card::Plains])
            .actions(&[
                ME.casts_face(GIANT, FaceIx::BACK),
                ME.targets_player(THEM),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                ME.casts(BOLT),
                ME.targets_player(THEM),
                THEM.activates(CIRCLE, 0),
                Resolve,
                THEM.life(18),
            ]),
        ADVENTURE
            .titled("A countered Stomp exiled by Rest in Peace grants no casting permission")
            .interaction(&[Card::RestInPeace])
            .hand(P0, &[GIANT, MOUNTAIN])
            .hand(P1, &[COUNTER])
            .battlefield(P1, &[Card::RestInPeace, ISLAND, ISLAND])
            .actions(&[
                ME.casts_face(GIANT, FaceIx::BACK),
                ME.targets_player(THEM),
                THEM.casts(COUNTER),
                ME.has(Zone::Exile, GIANT, 1),
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        Scenario::new("A permission to cast a card for free can cast Stomp")
            .interaction(&[Card::MalcolmAlluringScoundrel])
            .permanents(
                P0,
                &[Permanent::new(Card::MalcolmAlluringScoundrel).counters(CounterKind::Chorus, 3)],
            )
            .hand(P0, &[GIANT])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::MalcolmAlluringScoundrel),
                advance_to(1, TurnStep::CombatDamage),
                Resolve,
                ME.picks(GIANT),
                ME.answers(
                    DecisionKind::CastOffer,
                    Pick::Cast {
                        card: GIANT,
                        face: FaceIx::BACK,
                        costs: Castings::NONE,
                    },
                ),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(16),
                ME.has(Zone::Exile, GIANT, 1),
            ]),
        Scenario::new("A spell and its copy each trigger Bonecrusher Giant")
            .interaction(&[BOLT, Card::Fork])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[GIANT])
            .battlefield(P1, &[MOUNTAIN; 3])
            .hand(P1, &[BOLT, Card::Fork])
            .actions(&[
                THEM.casts(BOLT),
                THEM.targets(GIANT),
                THEM.casts(Card::Fork),
                Resolve,
                THEM.targets(GIANT),
                Resolve,
                THEM.life(18),
                ME.controls(GIANT, 1),
                Resolve,
                ME.has(Zone::Graveyard, GIANT, 1),
                Resolve,
                THEM.life(16),
            ]),
        ADVENTURE
            .titled("A copy of Stomp resolves but leaves no card to cast")
            .interaction(&[Card::Fork])
            .hand(P1, &[Card::Fork])
            .battlefield(P1, &[MOUNTAIN; 5])
            .actions(&[
                ME.casts_face(GIANT, FaceIx::BACK),
                ME.targets_player(THEM),
                THEM.casts(Card::Fork),
                Resolve,
                THEM.targets_player(ME),
                Resolve,
                ME.life(18),
                THEM.has_cards(Zone::Exile, 0),
                Resolve,
                THEM.life(18),
                ME.has(Zone::Exile, GIANT, 1),
            ]),
    ];
}
