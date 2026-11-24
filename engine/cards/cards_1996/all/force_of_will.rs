// Generated from the card store by `just card header`: notes go below the end marker.
//! Force of Will {3}{U}{U}
//! Instant
//!
//! You may pay 1 life and exile a blue card from your hand rather than pay this spell's mana cost.
//! Counter target spell.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ForceOfWill
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&counter(target()))])
    .casting(&[CastingOption::Alternative {
        cost: ManaCost::NONE,
        parts: &[
            CostPart::Life(1),
            CostPart::exile(1, Zone::Hand, Filter::Colors(Colors::BLUE)),
        ],
    }])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const FORCE: &str = Card::ForceOfWill;
    const TRINISPHERE: &str = Card::Trinisphere;
    const THALIA: &str = Card::ThaliaGuardianOfThraben;
    const PAINTER: &str = Card::PaintersServant;

    /// P0 casts Force of Will for its alternative cost.
    const FOR_ALTERNATIVE: Action = Action::Cast {
        player: P0,
        card: FORCE,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };

    pub const SCENARIOS: &[Scenario] = &[
        // The ruling: the total starts from the alternative cost, of no mana, which
        // Trinisphere raises to three. Three Islands pay it, besides the life and the card.
        // Trinisphere raises the opponent's Portable Hole to three too.
        Scenario::new("Force of Will for its alternative cost under Trinisphere costs three")
            .interaction(&[TRINISPHERE])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[TRINISPHERE, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[FORCE, Prop::UntargetedSpell])
            .battlefield(P1, &[Card::Plains; 3])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                THEM.casts(Prop::Artifact),
                FOR_ALTERNATIVE,
                ME.tapped(Card::Island, 3),
                ME.life(19),
                ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
            ]),
        // Cast for mana, Force of Will is a noncreature spell, so Thalia makes it {4}{U}{U}.
        Scenario::new("Force of Will for mana under Thalia costs one more")
            .interaction(&[THALIA])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    THALIA,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[FORCE])
            .battlefield(P1, &[Card::Plains, Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                THEM.casts(Prop::Artifact),
                ME.casts(FORCE),
                ME.tapped(Card::Island, 6),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
            ]),
        // With blue chosen for Painter's Servant, the Plains in P0's hand is blue, so it pays
        // for Force of Will, which counters P0's own Portable Hole.
        Scenario::new("Force of Will exiles a card Painter's Servant made blue")
            .interaction(&[PAINTER])
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[PAINTER, Prop::Artifact, FORCE, Card::Plains])
            .actions(&[
                ME.casts(PAINTER),
                Resolve,
                ME.chooses_color(Colors::BLUE),
                ME.casts(Prop::Artifact),
                FOR_ALTERNATIVE,
                ME.life(19),
                ME.has(Zone::Exile, Card::Plains, 1),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.has(Zone::Graveyard, FORCE, 1),
            ]),
        // Its pitch cost changes what is paid, while Mana Drain still sees mana value five.
        Scenario::new("a pitched Force of Will gives Mana Drain five mana")
            .interaction(&[Card::ManaDrain])
            .battlefield(P0, &[Card::Plains, Card::Island, Card::Island])
            .hand(P0, &[Prop::Artifact, Card::ManaDrain, Card::Plains])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[FORCE, Prop::UntargetedSpell])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.passes(),
                Action::Cast {
                    player: P1,
                    card: FORCE,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.casts(Card::ManaDrain),
                ME.targets(FORCE),
                THEM.has(Zone::Graveyard, FORCE, 1),
                ME.controls(Prop::Artifact, 1),
                advance_to(1, TurnStep::PostcombatMain),
                ME.passes(),
                ME.mana(ManaColor::Colorless, 5),
            ]),
    ];
}
