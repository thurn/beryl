// Generated from the card store by `just card header`: notes go below the end marker.
//! Pithing Needle {1}
//! Artifact
//!
//! As this artifact enters, choose a card name.
//! Activated abilities of sources with the chosen name can't be activated unless they're mana
//! abilities.
// End of the generated card header.

use crate::prelude::*;

/// "As this artifact enters, choose a card name." A replacement effect of its own entering
/// ([CR 614.12a]): "If a replacement effect that modifies how a permanent enters the
/// battlefield requires a choice, that choice is made before the permanent enters the
/// battlefield." The names offered are those of the game's cards, and "another card" for
/// every other name, which stops nothing in this game.
///
/// "Activated abilities of sources with the chosen name can't be activated unless they're mana
/// abilities." A rule change for every player ([CR 602.5]): "A player can't begin to activate
/// an ability that's prohibited from being activated." Loyalty abilities are activated
/// abilities ([CR 606.4]): "The cost to activate a loyalty ability of a permanent is to put on
/// or remove from that permanent a certain number of loyalty counters, as shown by the
/// loyalty symbol in the ability's cost." Abilities already on the stack are not affected.
///
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
/// [CR 602.5]: https://yawgatog.com/resources/magic-rules/#R6025
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
    ];
}
