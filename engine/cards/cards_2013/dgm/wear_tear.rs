// Generated from the card store by `just card header`: notes go below the end marker.
//! Wear {1}{R}
//! Instant
//!
//! Destroy target artifact.
//! Fuse (You may cast one or both halves of this card from your hand.)
//!
//! Tear {W}
//! Instant
//!
//! Destroy target enchantment.
//! Fuse (You may cast one or both halves of this card from your hand.)
// End of the generated card header.

use crate::prelude::*;

const ARTIFACT: TargetSpec = TargetSpec::permanent(Filter::Types(Types::ARTIFACT));
const ENCHANTMENT: TargetSpec = TargetSpec::permanent(Filter::Types(Types::ENCHANTMENT));
const WEAR: Face = Printed::Wear.abilities(&[Ability::spell()
    .target(&ARTIFACT)
    .effect(&destroy(target()))]);
const TEAR: Face = Printed::Tear.abilities(&[Ability::spell()
    .target(&ENCHANTMENT)
    .effect(&destroy(target()))]);

pub static CARD: CardDef = card_with_shape(
    Card::WearTear,
    PrintedShape::Wear,
    &[
        WEAR,
        TEAR,
        Face::fused(
            Card::WearTear,
            &WEAR,
            &TEAR,
            &[Ability::spell()
                .targeting(&[ARTIFACT, ENCHANTMENT])
                .effect(&sequence(&[destroy(target()), destroy(nth_target(1))]))],
        ),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPLIT: &str = Card::WearTear;
    const ARTIFACT: &str = Prop::Artifact;
    const ENCHANTMENT: &str = Card::BloodMoon;
    const RED: &str = Card::Mountain;
    const WHITE: &str = Card::Plains;

    const fn fuses() -> Action {
        Action::Cast {
            player: P0,
            card: SPLIT,
            face: FaceIx::FUSED,
            costs: Castings::FUSE,
        }
    }

    const fn setup(name: &'static str) -> Scenario {
        Scenario::new(name)
            .interaction(&[ENCHANTMENT])
            .battlefield(P0, &[RED, RED, WHITE])
            .hand(P0, &[SPLIT])
            .battlefield(P1, &[ARTIFACT, ENCHANTMENT, Card::Island, Card::Island])
            .hand(P1, &[Prop::Instant])
    }

    const REGENERATION: &str = Prop::Regeneration;
    const THOPTER: &str = Card::Ornithopter;

    const fn regenerating_targets(name: &'static str) -> Scenario {
        setup(name)
            .interaction(&[ENCHANTMENT, Card::Opalescence])
            .battlefield(P1, &[THOPTER, ENCHANTMENT, Card::Opalescence, WHITE, WHITE])
            .hand(P1, &[REGENERATION, REGENERATION])
    }

    pub const SCENARIOS: &[Scenario] = &[
        setup("Wear destroys only the artifact").actions(&[
            ME.casts(SPLIT),
            ME.colored(Zone::Stack, "Wear", Colors::RED),
            Resolve,
            THEM.controls(ARTIFACT, 0),
            THEM.controls(ENCHANTMENT, 1),
        ]),
        setup("Tear destroys only the enchantment").actions(&[
            ME.casts_face(SPLIT, FaceIx::BACK),
            ME.colored(Zone::Stack, "Tear", Colors::WHITE),
            Resolve,
            THEM.controls(ARTIFACT, 1),
            THEM.controls(ENCHANTMENT, 0),
        ]),
        regenerating_targets("Wear allows its target to regenerate").actions(&[
            ME.casts(SPLIT),
            THEM.casts(REGENERATION),
            THEM.targets(THOPTER),
            Resolve,
            Resolve,
            THEM.tapped(THOPTER, 1),
        ]),
        regenerating_targets("Tear allows its target to regenerate").actions(&[
            ME.casts_face(SPLIT, FaceIx::BACK),
            ME.targets(ENCHANTMENT),
            THEM.casts(REGENERATION),
            THEM.targets(ENCHANTMENT),
            Resolve,
            Resolve,
            THEM.tapped(ENCHANTMENT, 1),
        ]),
        regenerating_targets("Both targets of fused Wear // Tear can regenerate")
            .battlefield(
                P0,
                &[RED, RED, WHITE, Card::Swamp, Card::Swamp, Card::Swamp],
            )
            .hand(P0, &[SPLIT, Prop::FlashCreature])
            .actions(&[
                fuses(),
                ME.targets(ENCHANTMENT),
                ME.passes(),
                THEM.casts(REGENERATION),
                THEM.targets(THOPTER),
                THEM.casts(REGENERATION),
                THEM.targets(ENCHANTMENT),
                Resolve,
                Resolve,
                Resolve,
                THEM.tapped(THOPTER, 1),
                THEM.tapped(ENCHANTMENT, 1),
            ]),
        setup("Fused Wear // Tear destroys an artifact before its enchantment target")
            .interaction(&[ENCHANTMENT, Card::RestInPeace])
            .battlefield(
                P1,
                &[
                    ARTIFACT,
                    Prop::Equipment,
                    ENCHANTMENT,
                    Card::RestInPeace,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.colored(Zone::Hand, SPLIT, Colors::RED.union(Colors::WHITE)),
                fuses(),
                ME.targets(ARTIFACT),
                ME.targets(Card::RestInPeace),
                ME.colored(Zone::Stack, SPLIT, Colors::RED.union(Colors::WHITE)),
                ME.tapped(RED, 2),
                ME.tapped(WHITE, 1),
                Resolve,
                THEM.has(Zone::Exile, ARTIFACT, 1),
                THEM.controls(Card::RestInPeace, 0),
                THEM.controls(Prop::Equipment, 1),
                THEM.controls(ENCHANTMENT, 1),
            ]),
        setup("Fused Wear // Tear can target the same permanent twice")
            .interaction(&[ENCHANTMENT, Card::LiquimetalCoating])
            .battlefield(P0, &[RED, RED, WHITE, Card::LiquimetalCoating])
            .battlefield(P1, &[ENCHANTMENT])
            .hand(P1, &[])
            .actions(&[
                ME.activates(Card::LiquimetalCoating, 0),
                ME.targets(ENCHANTMENT),
                Resolve,
                fuses(),
                ME.targets(ENCHANTMENT),
                Resolve,
                THEM.controls(ENCHANTMENT, 0),
            ]),
        setup("Fused Wear // Tear destroys the enchantment when its artifact target leaves")
            .battlefield(
                P0,
                &[RED, RED, WHITE, Card::Swamp, Card::Swamp, Card::Swamp],
            )
            .hand(P0, &[SPLIT, Prop::FlashCreature])
            .battlefield(P1, &[ARTIFACT, ENCHANTMENT, RED, RED])
            .hand(P1, &[SPLIT])
            .actions(&[
                fuses(),
                ME.passes(),
                THEM.casts(SPLIT),
                Resolve,
                THEM.controls(ARTIFACT, 0),
                THEM.controls(ENCHANTMENT, 1),
                Resolve,
                THEM.controls(ENCHANTMENT, 0),
            ]),
        setup("Fork's copy of fused Wear // Tear destroys both kinds of permanent")
            .interaction(&[ENCHANTMENT, Card::Fork, Card::DressDown])
            .battlefield(
                P0,
                &[Prop::Equipment, Card::DressDown, RED, RED, RED, RED, WHITE],
            )
            .hand(P0, &[SPLIT, Card::Fork])
            .actions(&[
                fuses(),
                ME.targets(ARTIFACT),
                ME.targets(ENCHANTMENT),
                ME.casts(Card::Fork),
                Resolve,
                ME.targets(Prop::Equipment),
                ME.targets(Card::DressDown),
                Resolve,
                stack(1),
                Resolve,
                ME.controls(Prop::Equipment, 0),
                ME.controls(Card::DressDown, 0),
                THEM.controls(ARTIFACT, 0),
                THEM.controls(ENCHANTMENT, 0),
            ]),
        setup("Flashback offers only the individual halves of Wear // Tear")
            .interaction(&[ENCHANTMENT, Card::SnapcasterMage])
            .battlefield(P0, &[Card::Island, Card::Island, RED, RED, WHITE])
            .hand(P0, &[Card::SnapcasterMage])
            .graveyard(P0, &[SPLIT])
            .actions(&[
                ME.casts(Card::SnapcasterMage),
                Resolve,
                Resolve,
                ME.deciding(DecisionKind::Priority, 3),
                ME.casts(SPLIT),
                Resolve,
                THEM.controls(ARTIFACT, 0),
                THEM.controls(ENCHANTMENT, 1),
                ME.has(Zone::Exile, SPLIT, 1),
            ]),
        Scenario::new("Pithing Needle can name Tear")
            .interaction(&[Card::PithingNeedle])
            .battlefield(P0, &[Card::Island])
            .hand(P0, &[Card::PithingNeedle, SPLIT])
            .actions(&[
                ME.casts(Card::PithingNeedle),
                Resolve,
                ME.names("Tear"),
                ME.controls(Card::PithingNeedle, 1),
            ]),
        setup("Mana Drain reads fused Wear // Tear as mana value three")
            .interaction(&[ENCHANTMENT, Card::ManaDrain])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P1, &[ARTIFACT, ENCHANTMENT, Card::Island, Card::Island])
            .hand(P1, &[Card::ManaDrain, Card::Island])
            .actions(&[
                THEM.passes(),
                fuses(),
                THEM.casts(Card::ManaDrain),
                Resolve,
                THEM.controls(ARTIFACT, 1),
                THEM.controls(ENCHANTMENT, 1),
                advance_to(2, TurnStep::PostcombatMain),
                THEM.passes(),
                THEM.mana(ManaColor::Colorless, 3),
            ]),
    ];
}
