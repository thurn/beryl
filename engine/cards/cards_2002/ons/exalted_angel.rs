// Generated from the card store by `just card header`: notes go below the end marker.
//! Exalted Angel {4}{W}{W}
//! Creature — Angel, 4/5
//!
//! Flying
//! Whenever this creature deals damage, you gain that much life.
//! Morph {2}{W}{W} (You may cast this card face down as a 2/2 creature for {3}. Turn it face up any
//! time for its morph cost.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ExaltedAngel
    .keywords(Keywords::FLYING)
    .abilities(&[
        Ability::triggered(&Trigger::DEALS_DAMAGE).effect(&Effect::GainLife {
            player: Players::You,
            amount: Amount::Carried,
        }),
    ])
    .casting(&[CastingOption::Morph(Cost::mana(ManaCost::new(
        2,
        [2, 0, 0, 0, 0, 0],
    )))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ANGEL: &str = Card::ExaltedAngel;
    const TURN_UP: Action = Action::TurnFaceUp {
        player: P0,
        card: ANGEL,
    };
    const PLAINS: &str = Card::Plains;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("A face-down creature attacks before it turns face up")
            .at(1, P0, TurnStep::BeginningOfCombat)
            .permanents(P0, &[Permanent::new(ANGEL).face_down()])
            .battlefield(P0, &[PLAINS; 6])
            .actions(&[
                ME.pt(ANGEL, 2, 2),
                ME.passes(),
                ME.attacks_with(ANGEL),
                TURN_UP,
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 1 }),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(16),
                ME.life(24),
            ]),
        Scenario::new("A morph can turn face up in response to removal")
            .interaction(&[Card::LightningBolt])
            .hand(P0, &[ANGEL])
            .battlefield(
                P0,
                &[PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, PLAINS],
            )
            .hand(P1, &[Card::LightningBolt])
            .battlefield(P1, &[Card::Mountain])
            .actions(&[
                ME.casts_face(ANGEL, FaceIx::DOWN),
                Resolve,
                ME.passes(),
                THEM.casts(Card::LightningBolt),
                THEM.targets(ANGEL),
                TURN_UP,
                ME.pt(ANGEL, 4, 5),
                THEM.has(Zone::Graveyard, Card::LightningBolt, 1),
            ]),
        Scenario::new("A countered morph goes to the graveyard revealed")
            .hand(P0, &[ANGEL])
            .battlefield(P0, &[PLAINS, PLAINS, PLAINS])
            .hand(P1, &[Prop::Instant])
            .battlefield(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts_face(ANGEL, FaceIx::DOWN),
                ME.colored(Zone::Stack, ANGEL, Colors::NONE),
                THEM.casts(Prop::Instant),
                ME.has(Zone::Graveyard, ANGEL, 1),
            ]),
        Scenario::new("A morph that dies face down goes to the graveyard revealed")
            .interaction(&[Card::LightningBolt])
            .hand(P0, &[ANGEL])
            .battlefield(P0, &[PLAINS, PLAINS, PLAINS])
            .hand(P1, &[Card::LightningBolt])
            .battlefield(P1, &[Card::Mountain])
            .actions(&[
                ME.casts_face(ANGEL, FaceIx::DOWN),
                Resolve,
                ME.passes(),
                THEM.casts(Card::LightningBolt),
                THEM.targets(ANGEL),
                ME.has(Zone::Graveyard, ANGEL, 1),
            ]),
        Scenario::new("Exalted Angel gains life only when the damage trigger resolves")
            .interaction(&[Card::LightningBolt])
            .at(1, P0, TurnStep::BeginningOfCombat)
            .life(P0, 3)
            .battlefield(P0, &[ANGEL])
            .hand(P1, &[Card::LightningBolt])
            .battlefield(P1, &[Card::Mountain])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                THEM.passes(),
                ME.attacks_with(ANGEL),
                advance_to(1, TurnStep::CombatDamage),
                THEM.life(16),
                ME.life(3),
                ME.passes(),
                THEM.casts(Card::LightningBolt),
                THEM.targets_player(ME),
                wins(THEM),
            ]),
    ];
}
