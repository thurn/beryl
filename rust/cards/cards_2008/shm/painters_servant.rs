// Generated from the card store by `just card header`: notes go below the end marker.
//! Painter's Servant {2}
//! Artifact Creature — Scarecrow, 1/3
//!
//! As this creature enters, choose a color.
//! All cards that aren't on the battlefield, spells, and permanents are the chosen color in
//! addition to their other colors.
// End of the generated card header.

use crate::prelude::*;

/// The color effect applies to cards outside the battlefield, including in libraries and
/// exile, and to spells on the stack. It also applies to a spell copy, which is a spell
/// ([CR 707.10]).
///
/// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
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
    use crate::tokens::FOOD_TOKEN;

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
        // [CR 611.3b]: the effect applies only while Painter's Servant is on the battlefield,
        // so once Lightning Bolt kills it every card is back to its own colors, itself included.
        //
        // [CR 611.3b]: https://yawgatog.com/resources/magic-rules/#R6113b
        Scenario::new("the colors are gone when Painter's Servant leaves")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Mountain])
            .hand(P0, &[PAINTER, BOLT])
            .graveyard(P1, &[Card::Island])
            .actions(&[
                RED[0],
                RED[1],
                RED[2],
                THEM.colored(Zone::Graveyard, Card::Island, Colors::RED),
                ME.casts(BOLT),
                ME.targets(PAINTER),
                Resolve,
                ME.has(Zone::Graveyard, PAINTER, 1),
                ME.colored(Zone::Graveyard, PAINTER, Colors::NONE),
                THEM.colored(Zone::Graveyard, Card::Island, Colors::NONE),
            ]),
        // "All cards that aren't on the battlefield": libraries and exile too.
        Scenario::new("the chosen color is added to cards in libraries and exile")
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[PAINTER, Card::Plains])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Swamp, Card::Swamp])
            .exile(P1, &[Card::Island])
            .actions(&[
                RED[0],
                RED[1],
                RED[2],
                ME.colored(Zone::Library, Card::Island, Colors::RED),
                THEM.colored(Zone::Library, Card::Swamp, Colors::RED),
                THEM.colored(Zone::Exile, Card::Island, Colors::RED),
            ]),
        // The token enters red under Painter's Servant. Oko's later effect sets its
        // color to green and overwrites the color Painter added.
        Scenario::new("Oko overwrites the color Painter's Servant adds to a Food token")
            .interaction(&[Card::OkoThiefOfCrowns])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::OkoThiefOfCrowns])
            .hand(P0, &[PAINTER])
            .actions(&[
                RED[0],
                RED[1],
                RED[2],
                ME.activates(Card::OkoThiefOfCrowns, 0),
                Resolve,
                ME.colored(Zone::Battlefield, FOOD_TOKEN, Colors::RED),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(Card::OkoThiefOfCrowns, 1),
                ME.targets(FOOD_TOKEN),
                Resolve,
                ME.colored(Zone::Battlefield, FOOD_TOKEN, Colors::GREEN),
            ])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island]),
        // The artifact starts colorless in hand and is white when cast,
        // so the Feather's cast trigger sees a white spell.
        Scenario::new("Painter's Servant makes an artifact trigger Angel's Feather")
            .interaction(&[Card::AngelsFeather])
            .battlefield(P0, &[Card::Plains; 3])
            .battlefield(P1, &[Card::AngelsFeather])
            .hand(P0, &[PAINTER, Prop::Equipment])
            .actions(&[
                ME.colored(Zone::Hand, Prop::Equipment, Colors::NONE),
                ME.casts(PAINTER),
                Resolve,
                ME.chooses_color(Colors::WHITE),
                ME.casts(Prop::Equipment),
                Resolve,
                THEM.yes(),
                THEM.life(21),
                Resolve,
                ME.colored(Zone::Battlefield, Prop::Equipment, Colors::WHITE),
            ]),
        // A copy on the stack gets the chosen color as well as its own red.
        Scenario::new("Painter's Servant adds blue to Fork's spell copy")
            .interaction(&[Card::Fork, BOLT])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Mountain])
            .hand(P0, &[PAINTER, BOLT])
            .battlefield(P1, &[Card::Mountain; 2])
            .hand(P1, &[Card::Fork])
            .actions(&[
                BLUE[0],
                BLUE[1],
                BLUE[2],
                ME.casts(BOLT),
                ME.targets_player(THEM),
                THEM.casts(Card::Fork),
                Resolve,
                THEM.targets_player(ME),
                THEM.colored(Zone::Stack, BOLT, Colors::RED.union(Colors::BLUE)),
                Resolve,
                ME.life(17),
            ]),
    ];
}
