// Generated from the card store by `just card header`: notes go below the end marker.
//! Force of Will {3}{U}{U}
//! Instant
//!
//! You may pay 1 life and exile a blue card from your hand rather than pay this spell's mana cost.
//! Counter target spell.
// End of the generated card header.

use crate::prelude::*;

/// Its alternative cost is life and a card: one life ([CR 119.4]), "If a player pays life,
/// the payment is subtracted from their life total", and a blue card from the hand, which its
/// caster chooses as the cost is paid. It is paid rather than the mana cost ([CR 118.9]), "an
/// alternative cost is a cost listed in a spell's text ... that its controller may pay rather
/// than paying the spell's mana cost", so an effect that raises the total still applies to it
/// ([CR 601.2f]): "The total cost is the mana cost or alternative cost (as determined in rule
/// 601.2b), plus all additional costs and cost increases, and minus all cost reductions."
///
/// [CR 119.4]: https://yawgatog.com/resources/magic-rules/#R1194
/// [CR 118.9]: https://yawgatog.com/resources/magic-rules/#R1189
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
pub static CARD: CardDef = Printed::ForceOfWill
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&COUNTER_TARGET_SPELL)])
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
        // Five Islands pay {3}{U}{U}; with no other blue card in hand, the alternative cost
        // is not offered.
        Scenario::new("Force of Will cast for its mana cost counters a spell")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Island; 5])
            .hand(P0, &[FORCE])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                THEM.casts(Prop::Artifact),
                // Passing and the cast for mana.
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(FORCE),
                ME.tapped(Card::Island, 5),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.life(20),
                ME.has(Zone::Graveyard, FORCE, 1),
            ]),
        // No lands: one life and the one blue card in hand, Timetwister, pay for it.
        Scenario::new("Force of Will for 1 life and a blue card")
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[FORCE, Prop::UntargetedSpell])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                THEM.casts(Prop::Artifact),
                FOR_ALTERNATIVE,
                ME.life(19),
                ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.has(Zone::Graveyard, FORCE, 1),
            ]),
        // Two blue cards and a Plains in hand: P0 chooses between the blue cards alone.
        Scenario::new("Force of Will: the caster chooses which blue card to exile")
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(
                P0,
                &[
                    FORCE,
                    Prop::UntargetedSpell,
                    Card::Counterspell,
                    Card::Plains,
                ],
            )
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                THEM.casts(Prop::Artifact),
                FOR_ALTERNATIVE,
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Card::Counterspell),
                ME.has(Zone::Exile, Card::Counterspell, 1),
                ME.has(Zone::Hand, Prop::UntargetedSpell, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
            ]),
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
    ];
}
