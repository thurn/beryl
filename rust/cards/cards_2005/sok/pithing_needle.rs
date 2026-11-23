// Generated from the card store by `just card header`: notes go below the end marker.
//! Pithing Needle {1}
//! Artifact
//!
//! As this artifact enters, choose a card name.
//! Activated abilities of sources with the chosen name can't be activated unless they're mana
//! abilities.
// End of the generated card header.

use crate::prelude::*;

/// The restriction covers loyalty abilities ([CR 606.4]) and abilities activated from the
/// hand, a graveyard, or exile. An ability already on the stack still resolves.
///
/// [CR 606.4]: https://yawgatog.com/resources/magic-rules/#R6064
pub static CARD: CardDef = Printed::PithingNeedle
    .abilities(&[
        Ability::replacing(&Replacement::choose_as_enters(ChosenKind::CardName)),
        Ability::rule_change(&RuleChange {
            players: Players::EachPlayer,
            what: Governed::ActivateAbilities {
                sources: Named::Chosen,
                mana_abilities: false,
            },
            when: When::Never,
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const NEEDLE: &str = Card::PithingNeedle;
    const MOTHER: &str = Card::MotherOfRunes;
    const JACE: &str = Prop::Planeswalker;
    const STRIP: &str = Card::StripMine;
    const IDENTITY: &str = Card::FracturedIdentity;
    const IMMORTAL: &str = Card::MisterImmortal;

    /// P0 holds Pithing Needle and an Island to play. The Island keeps P0's priority a
    /// decision once nothing else can be done: a player who could only pass isn't asked.
    const fn needle(name: &'static str) -> Scenario {
        Scenario::new(name).hand(P0, &[NEEDLE, Card::Island])
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Every player is stopped: P1 can't activate Mother of Runes in their upkeep, though
        // P0 chose the name. Its triggered and static abilities, which it has none of, would
        // work.
        needle("naming Mother of Runes stops its ability for each player")
            .interaction(&[MOTHER])
            .battlefield(P0, &[Card::Island, MOTHER])
            .battlefield(P1, &[MOTHER])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(MOTHER),
                ME.controls(NEEDLE, 1),
                // Passing or playing the Island: Mother's ability can't be activated.
                ME.deciding(DecisionKind::Priority, 2),
                ME.passes(),
                ME.done_attacking(),
                // P1, having drawn an Island, may pass or play it, and no more.
                advance_to(2, TurnStep::PrecombatMain),
                THEM.deciding(DecisionKind::Priority, 2),
            ]),
        // "Another card" is legal and stops nothing: Mother still gives protection.
        needle("naming another card stops nothing")
            .interaction(&[MOTHER])
            .battlefield(P0, &[Card::Island, MOTHER, Prop::Flier])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names_another_card(),
                ME.controls(NEEDLE, 1),
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Flier),
                Resolve,
                ME.chooses_color(Colors::RED),
                ME.tapped(MOTHER, 1),
            ]),
        Scenario::new("Pithing Needle can name either face of a double-faced card")
            .interaction(&[Card::GlasspoolMimic])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[NEEDLE, NEEDLE, Card::Island, Card::GlasspoolMimic])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.deciding(DecisionKind::ChooseObject, 5),
                ME.names(Card::GlasspoolMimic),
                ME.casts(NEEDLE),
                Resolve,
                ME.names("Glasspool Shore"),
                ME.controls(NEEDLE, 2),
            ]),
        // Loyalty abilities are activated abilities (CR 606.4).
        needle("naming a planeswalker stops its loyalty abilities")
            .battlefield(P0, &[Card::Island, JACE])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(JACE),
                ME.deciding(DecisionKind::Priority, 2),
                ME.has_loyalty(JACE, 3),
            ]),
        // The first ruling: of a card with a mana ability and another activated ability, the
        // mana ability can be activated. Strip Mine pays for a second Needle, and its other
        // ability, which would destroy P1's Plains, is not offered.
        needle("naming Strip Mine leaves its mana ability")
            .interaction(&[STRIP])
            .battlefield(P0, &[Card::Island, STRIP])
            .battlefield(P1, &[Card::Plains])
            .hand(P0, &[NEEDLE, NEEDLE, Card::Island])
            .actions(&[
                ME.casts(NEEDLE),
                ME.tapped(Card::Island, 1),
                Resolve,
                ME.names(STRIP),
                // Passing, playing the Island and casting the second Needle.
                ME.deciding(DecisionKind::Priority, 3),
                ME.casts(NEEDLE),
                ME.tapped(STRIP, 1),
                Resolve,
                ME.names(STRIP),
                ME.controls(NEEDLE, 2),
                ME.controls(STRIP, 1),
                THEM.controls(Card::Plains, 1),
            ]),
        // A token copy has the copiable values of what it copied, its name among them
        // ([CR 707.2]): "When copying an object, the copy acquires the copiable values of the
        // original object's characteristics". P0 takes a copy of P1's Jace with Fractured
        // Identity, then names Jace: the copy's loyalty abilities can't be activated.
        //
        // [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
        needle("naming a card stops a token copy of it")
            .interaction(&[IDENTITY])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[IDENTITY, NEEDLE, Card::Island])
            .battlefield(P1, &[JACE])
            .actions(&[
                // Jace is the one legal target.
                ME.casts(IDENTITY),
                Resolve,
                ME.controls(JACE, 1),
                ME.casts(NEEDLE),
                Resolve,
                ME.names(JACE),
                ME.deciding(DecisionKind::Priority, 2),
                ME.has_loyalty(JACE, 3),
            ]),
        // Cycling is an activated ability that works from the hand, and Needle stops it
        // wherever the card is (CR 602.5). Spara's Headquarters, a land, is still playable:
        // Needle stops activating, not casting or playing. P0 has pass and play-the-land, and
        // no cycling, with three Plains left untapped.
        needle("naming a card stops it from being cycled in hand")
            .interaction(&[Card::SparasHeadquarters])
            .battlefield(P0, &[Card::Plains; 4])
            .hand(P0, &[NEEDLE, Card::SparasHeadquarters])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(Card::SparasHeadquarters),
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        // The same, naming something else: cycling is offered as a third option.
        needle("naming another card leaves cycling")
            .interaction(&[Card::SparasHeadquarters])
            .battlefield(P0, &[Card::Plains; 4])
            .hand(P0, &[NEEDLE, Card::SparasHeadquarters])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names_another_card(),
                ME.deciding(DecisionKind::Priority, 3),
            ]),
        needle("naming Mister Immortal stops activation from the graveyard")
            .interaction(&[IMMORTAL])
            .battlefield(P0, &[Card::Forest; 4])
            .graveyard(P0, &[IMMORTAL])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(IMMORTAL),
                ME.deciding(DecisionKind::Priority, 2),
                ME.has(Zone::Graveyard, IMMORTAL, 1),
            ]),
        needle("naming Mister Immortal stops activation from exile")
            .interaction(&[IMMORTAL])
            .battlefield(P0, &[Card::Forest; 4])
            .exile(P0, &[IMMORTAL])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(IMMORTAL),
                ME.deciding(DecisionKind::Priority, 2),
                ME.has(Zone::Exile, IMMORTAL, 1),
            ]),
        // The restriction lasts only while Needle is on the battlefield (CR 611.3a, a static
        // ability's effect). On P1's turn Portable Hole exiles Needle, and P0 can activate
        // Mother of Runes again.
        Scenario::new("Pithing Needle leaving the battlefield ends its restriction")
            .interaction(&[MOTHER])
            .battlefield(P0, &[Card::Island, MOTHER])
            .hand(P0, &[NEEDLE, Card::Island])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Card::PortableHole])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(MOTHER),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Card::PortableHole),
                Resolve,
                THEM.targets(NEEDLE),
                Resolve,
                ME.controls(NEEDLE, 0),
                THEM.passes(),
                ME.activates(MOTHER, 0),
                Resolve,
                ME.chooses_color(Colors::RED),
                ME.tapped(MOTHER, 1),
            ]),
        // A mana ability that costs more than tapping is still a mana ability (CR 605.1a).
        // Lion's Eye Diamond can be activated, from the hand even, when named.
        needle("naming Lion's Eye Diamond leaves its mana ability")
            .interaction(&[Card::LionsEyeDiamond])
            .battlefield(
                P0,
                &[Card::Island, Card::LionsEyeDiamond, Card::LionsEyeDiamond],
            )
            .hand(P0, &[NEEDLE, Card::Plains])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(Card::LionsEyeDiamond),
                ME.activates(Card::LionsEyeDiamond, 0),
                ME.chooses_color(Colors::RED),
                ME.mana(ManaColor::Red, 3),
                ME.has(Zone::Graveyard, Card::LionsEyeDiamond, 1),
            ]),
        // The artifact has only a triggered ability. It is legal to name, and
        // its enter trigger still exiles P0's Needle.
        Scenario::new("Pithing Needle can name a card with only a triggered ability")
            .battlefield(P0, &[Card::Island])
            .hand(P0, &[NEEDLE, Card::Plains])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(Prop::Artifact),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Prop::Artifact),
                Resolve,
                Resolve,
                ME.has(Zone::Exile, NEEDLE, 1),
            ]),
        // Naming Thalia leaves her static cost increase in effect.
        Scenario::new("Pithing Needle leaves Thalia's spell tax in effect")
            .interaction(&[Card::ThaliaGuardianOfThraben, Card::LightningBolt])
            .battlefield(P0, &[Card::Mountain; 4])
            .hand(P0, &[NEEDLE, Card::LightningBolt])
            .battlefield(P1, &[Card::ThaliaGuardianOfThraben])
            .actions(&[
                ME.casts(NEEDLE),
                Resolve,
                ME.names(Card::ThaliaGuardianOfThraben),
                ME.casts(Card::LightningBolt),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 4),
                Resolve,
                THEM.life(17),
            ]),
    ];
}
