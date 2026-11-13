// Generated from the card store by `just card header`: notes go below the end marker.
//! Currency Converter {1}
//! Artifact
//!
//! Whenever you discard a card, you may exile that card from your graveyard.
//! {2}, {T}: Draw a card, then discard a card.
//! {T}: Put a card exiled with this artifact into its owner's graveyard. If it's a land card,
//! create a Treasure token. If it's a nonland card, create a 2/2 black Rogue creature token.
// End of the generated card header.

use crate::prelude::*;
use crate::tokens::TREASURE;

/// The Rogue token's name: its subtype plus the word "Token"
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)).
pub use crate::tokens::ROGUE_TOKEN;

/// The Rogue token: a 2/2 black Rogue creature token.
pub static ROGUE: CardDef = Token::RogueToken.card();

/// "That card from your graveyard" is the card as it was put into the graveyard: once it has
/// left, it is a new object ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007))
/// and nothing is asked. A discarded card that a replacement effect put somewhere else, as
/// madness exiles it, was never in the graveyard and is not exiled with this. The exile is linked to the last ability
/// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
/// refers only to cards in the exile zone that were put there as a result of an instruction
/// to exile them in the first ability."
///
/// Draws, then discards ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)):
/// "The controller of the spell or ability follows its instructions in the order written."
///
/// One of the cards exiled with this Converter, its controller's choice, goes to its owner's
/// graveyard (CR 400.3); what it is decides the token. Off the stack and the battlefield a
/// modal double-faced card has only its front face's characteristics
/// ([CR 712.6a](https://yawgatog.com/resources/magic-rules/#R7126a)), so Sink into Stupor is
/// a nonland card. With no card exiled with it there is no "it", and no token.
pub static CARD: CardDef = Printed::CurrencyConverter
    .abilities(&[
        Ability::triggered(&Trigger::discards(Players::You))
            .effect(&may(you(), &exile_linked(that_card_from_your_graveyard()))),
        Ability::activated(Cost::of(ManaCost::generic(2), &[CostPart::TAP]))
            .effect(&sequence(&[draw(you(), 1), discard(you(), 1)])),
        Ability::activated(Cost::TAP).effect(&sequence(&[
            choose(you(), exiled_with_this(), Prompt::new(Purpose::Choose).most(1)),
            if_then(
                if_you_do(),
                &sequence(&[
                    put_into_graveyard(those()),
                    if_else(
                        Condition::Is(those(), Filter::Types(Types::LAND)),
                        &create_tokens(you(), &TREASURE, 1),
                        &create_tokens(you(), &ROGUE, 1),
                    ),
                ]),
            ),
        ])),
    ])
    .card()
    .creating(&[&TREASURE, &ROGUE]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    use super::ROGUE_TOKEN;
    use crate::tokens::TREASURE_TOKEN;

    const CONVERTER: &str = Card::CurrencyConverter;

    pub const SCENARIOS: &[Scenario] = &[
        // The loot discards Oust, which the trigger exiles. Next turn the Converter puts it into
        // the graveyard: a nonland card, so a Rogue.
        Scenario::new("Currency Converter exiles its own loot's discard for a Rogue")
            // The Converter and the {2} for its loot.
            .battlefield(P0, &[CONVERTER, Card::Island, Card::Island])
            .hand(P0, &[Card::Oust])
            .library(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(CONVERTER, 1),
                Resolve,
                ME.picks(Card::Oust),
                Resolve,
                ME.yes(),
                ME.has(Zone::Exile, Card::Oust, 1),
                ME.has(Zone::Graveyard, Card::Oust, 0),
                ME.has(Zone::Hand, Card::Plains, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(CONVERTER, 2),
                Resolve,
                ME.has(Zone::Graveyard, Card::Oust, 1),
                ME.controls(ROGUE_TOKEN, 1),
                ME.token_pt(ROGUE_TOKEN, 2, 2),
                ME.controls(TREASURE_TOKEN, 0),
            ]),
        // Jace's loot discards an Island; the Converter may exile it but doesn't, so its {T} finds
        // nothing to return and makes nothing.
        Scenario::new("Currency Converter declines to exile another loot's discard")
            .interaction(&[Card::JaceVrynsProdigy])
            .battlefield(P0, &[CONVERTER, Card::JaceVrynsProdigy])
            .hand(P0, &[Card::Island])
            .library(P0, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(Card::JaceVrynsProdigy, 0),
                Resolve,
                ME.picks(Card::Island),
                Resolve,
                ME.no(),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.activates(CONVERTER, 2),
                Resolve,
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.controls(TREASURE_TOKEN, 0),
                ME.controls(ROGUE_TOKEN, 0),
            ]),
        // Thirst for Discovery discards Tundra and Counterspell, one at a time: one trigger for
        // each card, both exiled.
        Scenario::new("Currency Converter exiles Thirst for Discovery's discards")
            .interaction(&[Card::ThirstForDiscovery])
            // The Converter and the {2}{U} for Thirst for Discovery.
            .battlefield(P0, &[CONVERTER, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::ThirstForDiscovery])
            .library(
                P0,
                &[Card::Tundra, Prop::Artifact, Card::Counterspell, Card::Island],
            )
            .actions(&[
                ME.casts(Card::ThirstForDiscovery),
                Resolve,
                ME.picks(Card::Tundra),
                ME.picks(Card::Counterspell),
                // The two triggers carry different cards, so their order is a choice.
                ME.chooses(DecisionKind::TriggerOrder, CONVERTER),
                Resolve,
                ME.yes(),
                Resolve,
                ME.yes(),
                ME.has(Zone::Exile, Card::Tundra, 1),
                ME.has(Zone::Exile, Card::Counterspell, 1),
                ME.has(Zone::Hand, Prop::Artifact, 1),
            ]),
        // Jace's loot discards an Island and the Converter's own loot Oust: the Converter exiles
        // both. Next turn it returns the land of the two, for a Treasure.
        Scenario::new("Currency Converter chooses among its exiled cards")
            .interaction(&[Card::JaceVrynsProdigy])
            // The Converter, the {2} for its loot, and Jace, Vryn's Prodigy to loot too.
            .battlefield(
                P0,
                &[CONVERTER, Card::Island, Card::Island, Card::JaceVrynsProdigy],
            )
            .hand(P0, &[Card::Island, Card::Oust])
            .library(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(Card::JaceVrynsProdigy, 0),
                Resolve,
                ME.picks(Card::Island),
                Resolve,
                ME.yes(),
                ME.activates(CONVERTER, 1),
                Resolve,
                ME.picks(Card::Oust),
                Resolve,
                ME.yes(),
                ME.has(Zone::Exile, Card::Island, 1),
                ME.has(Zone::Exile, Card::Oust, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(CONVERTER, 2),
                Resolve,
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Card::Island),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.has(Zone::Exile, Card::Oust, 1),
                ME.controls(TREASURE_TOKEN, 1),
                ME.controls(ROGUE_TOKEN, 0),
            ]),
        // P1 returns the Converter to P0's hand in response to its {T}. The ability still refers
        // to the cards that Converter exiled (CR 607.2a), so Oust goes to the graveyard for a
        // Rogue. The creature makes the target a choice, which the oracles are then told.
        Scenario::new("Currency Converter's ability returns a card after the Converter left")
            // The Converter, the {2} for its loot, and a tapped creature, which can't attack on
            // the first turn.
            .battlefield(P0, &[CONVERTER, Card::Island, Card::Island])
            .permanents(P0, &[Permanent::new(Prop::SmallCreature).tapped()])
            .hand(P0, &[Card::Oust, Prop::SmallCreature])
            .library(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .battlefield(P1, &[Card::Island; 3])
            .hand(P1, &[Card::SinkIntoStupor])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(CONVERTER, 1),
                Resolve,
                ME.picks(Card::Oust),
                Resolve,
                ME.yes(),
                advance_to(3, TurnStep::PrecombatMain),
                // The two-drop in P0's hand, with flash, makes each of P0's priorities a decision.
                ME.activates(CONVERTER, 2),
                ME.passes(),
                THEM.casts(Card::SinkIntoStupor),
                THEM.targets(CONVERTER),
                Resolve,
                ME.has(Zone::Hand, CONVERTER, 1),
                Resolve,
                ME.has(Zone::Graveyard, Card::Oust, 1),
                ME.controls(ROGUE_TOKEN, 1),
            ]),
        // P1 returns the Converter to P0's hand after it exiled Oust. Recast, it is a new object
        // (CR 400.7), and Oust was exiled with the old one: its {T} finds nothing.
        Scenario::new("Currency Converter's exiled cards stay with the Converter that exiled them")
            // The Converter, the {2} for its loot and the {1} to cast it again.
            .battlefield(P0, &[CONVERTER, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::Oust])
            .library(P0, &[Card::Plains, Card::Plains])
            .battlefield(P1, &[Card::Island; 3])
            .hand(P1, &[Card::SinkIntoStupor])
            .actions(&[
                ME.activates(CONVERTER, 1),
                Resolve,
                ME.picks(Card::Oust),
                Resolve,
                ME.yes(),
                ME.passes(),
                THEM.casts(Card::SinkIntoStupor),
                ME.has(Zone::Hand, CONVERTER, 1),
                ME.casts(CONVERTER),
                Resolve,
                ME.activates(CONVERTER, 2),
                Resolve,
                ME.has(Zone::Exile, Card::Oust, 1),
                ME.controls(ROGUE_TOKEN, 0),
            ]),
    ];
}
