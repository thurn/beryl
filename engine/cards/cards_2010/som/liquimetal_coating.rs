// Generated from the card store by `just card header`: notes go below the end marker.
//! Liquimetal Coating {2}
//! Artifact
//!
//! {T}: Target permanent becomes an artifact in addition to its other types until end of turn.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::LiquimetalCoating
    .abilities(&[Ability::activated(Cost::TAP)
        .target(&TargetSpec::permanent(Filter::Any))
        .effect(&modify(
            target(),
            &[Modification::AddTypes(Types::ARTIFACT)],
            Duration::EndOfTurn,
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const COATING: &str = Card::LiquimetalCoating;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Liquimetal Coating leaves a creature's color unchanged")
            .battlefield(P0, &[COATING])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.activates(COATING, 0),
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.colored(Zone::Battlefield, Prop::Vanilla, Colors::BLUE),
            ]),
        Scenario::new("Liquimetal Coating can target an artifact")
            .battlefield(P0, &[COATING])
            .battlefield(P1, &[Prop::Artifact])
            .actions(&[
                ME.activates(COATING, 0),
                ME.targets(Prop::Artifact),
                Resolve,
                THEM.colored(Zone::Battlefield, Prop::Artifact, Colors::WHITE),
            ]),
        // [CR 514.2]: "Until end of turn" effects end as the turn's cleanup step does. In P1's
        // turn the land is not an artifact again, so P1's Wear (destroy target artifact) has no
        // legal target and the land stays.
        //
        // [CR 514.2]: https://yawgatog.com/resources/magic-rules/#R5142
        Scenario::new("Liquimetal Coating's artifact type ends with the turn")
            .interaction(&[Card::WearTear, Card::Mountain])
            .at(1, P0, TurnStep::PrecombatMain)
            .battlefield(P0, &[COATING, Card::Mountain, Card::Mountain])
            .battlefield(P1, &[Card::Plains, Card::Mountain, Card::Mountain])
            .hand(P1, &[Card::WearTear])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(COATING, 0),
                ME.targets(Card::Plains),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Card::WearTear),
                THEM.controls(Card::Plains, 1),
            ]),
    ];
}
