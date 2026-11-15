// Generated from the card store by `just card header`: notes go below the end marker.
//! Painter's Servant {2}
//! Artifact Creature — Scarecrow, 1/3
//!
//! As this creature enters, choose a color.
//! All cards that aren't on the battlefield, spells, and permanents are the chosen color in
//! addition to their other colors.
// End of the generated card header.

use crate::prelude::*;

/// "As this creature enters, choose a color" is a replacement effect that asks as the
/// creature enters ([CR 614.12a]): "If a replacement effect that modifies how a permanent
/// enters the battlefield requires a choice, that choice is made before the permanent enters
/// the battlefield."
///
/// The second ability is one layer 5 effect on every object but abilities on the stack
/// ([CR 613.1e]): "Layer 5: Color-changing effects are applied." The permanents get it from
/// the layers. A card in any other zone and a spell get it when their colors are read, which
/// is when it applies to them ([CR 611.3b]): "The effect applies at all times that the
/// permanent generating it is on the battlefield or the object generating it is in the
/// appropriate zone."
///
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
/// [CR 613.1e]: https://yawgatog.com/resources/magic-rules/#R6131e
/// [CR 611.3b]: https://yawgatog.com/resources/magic-rules/#R6113b
pub static CARD: CardDef = Printed::PaintersServant
    .abilities(&[
        Ability::replacing(&Replacement::choose_as_enters(ChosenKind::Color)),
        Ability::static_ability(&LayerSpec {
            affected: Objects::Everywhere(Filter::Any),
            condition: None,
            modifications: &[Modification::AddChosenColor],
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const PAINTER: &str = Card::PaintersServant;
    const SCIMITAR: &str = Card::LeoninScimitar;
    const BOLT: &str = Card::LightningBolt;

    /// P0 casts Painter's Servant with two Plains and chooses `color` as it enters.
    const fn paints(color: Colors) -> [Action; 3] {
        [ME.casts(PAINTER), Resolve, ME.chooses_color(color)]
    }

    const RED: [Action; 3] = paints(Colors::RED);
    const BLUE: [Action; 3] = paints(Colors::BLUE);

    pub const SCENARIOS: &[Scenario] = &[
        // Ruling: "This ability affects every card in every game zone, all tokens on the
        // battlefield, and all spell copies on the stack, regardless of who controls or owns
        // them." A colorless object "will simply be the new color", and a colored one keeps
        // its colors ("it adds another color").
        Scenario::new("the chosen color is added to every card and permanent")
            .battlefield(P0, &[Card::Plains, Card::Plains, SCIMITAR])
            // The Plains in P0's hand keeps P0's main phase a decision at the end.
            .hand(P0, &[PAINTER, Card::Counterspell, Card::Plains])
            .graveyard(P0, &[Card::Island])
            .graveyard(P1, &[Card::Island])
            .actions(&[
                ME.colored(Zone::Graveyard, Card::Island, Colors::NONE),
                RED[0],
                RED[1],
                RED[2],
                ME.colored(Zone::Battlefield, PAINTER, Colors::RED),
                ME.colored(Zone::Battlefield, SCIMITAR, Colors::RED),
                ME.colored(
                    Zone::Hand,
                    Card::Counterspell,
                    Colors::BLUE.union(Colors::RED),
                ),
                ME.colored(Zone::Graveyard, Card::Island, Colors::RED),
                THEM.colored(Zone::Graveyard, Card::Island, Colors::RED),
            ]),
        // A spell is one of the objects too: Lightning Bolt on the stack is blue and red.
        Scenario::new("a spell on the stack has the chosen color too")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Mountain])
            .hand(P0, &[PAINTER, BOLT])
            .actions(&[
                BLUE[0],
                BLUE[1],
                BLUE[2],
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.colored(Zone::Stack, BOLT, Colors::BLUE.union(Colors::RED)),
                Resolve,
                THEM.life(17),
                ME.colored(Zone::Graveyard, BOLT, Colors::BLUE.union(Colors::RED)),
            ]),
        // Ruling: "The effects of multiple Painter's Servants are cumulative." P1's chooses
        // too, from the other seat.
        Scenario::new("two Painter's Servants add both their colors")
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[PAINTER])
            .battlefield(P1, &[Card::Plains, Card::Plains])
            .hand(P1, &[PAINTER, Card::Plains])
            .graveyard(P0, &[Card::Island])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                RED[0],
                RED[1],
                RED[2],
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(PAINTER),
                Resolve,
                THEM.chooses_color(Colors::GREEN),
                ME.colored(
                    Zone::Graveyard,
                    Card::Island,
                    Colors::RED.union(Colors::GREEN),
                ),
                THEM.colored(Zone::Battlefield, PAINTER, Colors::RED.union(Colors::GREEN)),
            ]),
        // Circle of Protection: Red's "a red source of your choice": with red chosen every
        // permanent is red, a colorless artifact among them.
        Scenario::new("with red chosen, a colorless artifact is a red source")
            .interaction(&[Card::CircleOfProtectionRed])
            .battlefield(
                P0,
                &[
                    Card::CircleOfProtectionRed,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    SCIMITAR,
                ],
            )
            .hand(P0, &[PAINTER])
            .actions(&[
                RED[0],
                RED[1],
                RED[2],
                ME.activates(Card::CircleOfProtectionRed, 0),
                ME.picks(SCIMITAR),
                ME.tapped(Card::Plains, 3),
            ]),
        // Subtlety's evoke cost exiles "a blue card from your hand": with blue chosen, a
        // Plains is one.
        Scenario::new("with blue chosen, Subtlety's evoke cost exiles any card")
            .interaction(&[Card::Subtlety])
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[PAINTER, Card::Subtlety, Card::Plains])
            .actions(&[
                BLUE[0],
                BLUE[1],
                BLUE[2],
                Action::Cast {
                    player: P0,
                    card: Card::Subtlety,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                // It resolves with no other spell to wait for, and its two triggers are
                // ordered: the sacrifice resolves first.
                ME.chooses(DecisionKind::TriggerOrder, Card::Subtlety),
                Resolve,
                Resolve,
                ME.has(Zone::Exile, Card::Plains, 1),
                ME.has(Zone::Graveyard, Card::Subtlety, 1),
            ]),
    ];
}
