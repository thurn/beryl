//! Currency Converter. {1} Artifact. "Whenever you discard a card, you may exile that card
//! from your graveyard. {2}, {T}: Draw a card, then discard a card. {T}: Put a card exiled
//! with this artifact into its owner's graveyard. If it's a land card, create a Treasure
//! token. If it's a nonland card, create a 2/2 black Rogue creature token."

use crate::names;
use crate::tokens::TREASURE;
use beryl_card::effect::build::{
    EXILED_WITH_THIS, IF_YOU_DO, THAT_CARD, THOSE, YOU, choose, create_tokens, discard, draw,
    exile_linked, if_else, if_then, may, put_into_graveyard, sequence,
};
use beryl_card::{
    Ability, CardDef, Colors, Condition, Cost, CostPart, Effect, Face, Filter, ManaCost, Players,
    Prompt, Purpose, Subtype, Subtypes, Trigger, Types, card, mana,
};

/// The Rogue token's name: its subtype plus the word "Token"
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)).
pub const ROGUE_TOKEN: &str = "Rogue Token";

/// The Rogue token: a 2/2 black Rogue creature token.
pub static ROGUE: CardDef = card(
    ROGUE_TOKEN,
    &[Face::EMPTY
        .types(Types::CREATURE)
        .subtypes(Subtypes::of(&[Subtype::Rogue]))
        .colors(Colors::BLACK)
        .power_toughness(2, 2)],
);

/// "That card" is the card as it was put into the graveyard: once it has left, it is a new
/// object ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) and nothing is
/// asked. The exile is linked to the last ability
/// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
/// refers only to cards in the exile zone that were put there as a result of an instruction
/// to exile them in the first ability."
static EXILE_DISCARDED: Effect = may(YOU, &exile_linked(THAT_CARD));

/// Draws, then discards ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)):
/// "The controller of the spell or ability follows its instructions in the order written."
static LOOT: Effect = sequence(&[draw(YOU, 1), discard(YOU, 1)]);

/// One of the cards exiled with this Converter, its controller's choice, goes to its owner's
/// graveyard (CR 400.3); what it is decides the token. Off the stack and the battlefield a
/// modal double-faced card has only its front face's characteristics
/// ([CR 712.6a](https://yawgatog.com/resources/magic-rules/#R7126a)), so Sink into Stupor is
/// a nonland card. With no card exiled with it there is no "it", and no token.
static CONVERT: Effect = sequence(&[
    choose(YOU, EXILED_WITH_THIS, Prompt::new(Purpose::Choose).most(1)),
    if_then(
        IF_YOU_DO,
        &sequence(&[
            put_into_graveyard(THOSE),
            if_else(
                Condition::Is(THOSE, Filter::Types(Types::LAND)),
                &create_tokens(YOU, &TREASURE, 1),
                &create_tokens(YOU, &ROGUE, 1),
            ),
        ]),
    ),
]);

pub static CARD: CardDef = card(
    names::CURRENCY_CONVERTER,
    &[Face::EMPTY
        .cost(mana("{1}"))
        .types(Types::ARTIFACT)
        .abilities(&[
            Ability::triggered(&Trigger::discards(Players::You)).effect(&EXILE_DISCARDED),
            Ability::activated(Cost::of(ManaCost::generic(2), &[CostPart::TAP])).effect(&LOOT),
            Ability::activated(Cost::TAP).effect(&CONVERT),
        ])],
)
.creating(&[&TREASURE, &ROGUE]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use super::ROGUE_TOKEN;
    use crate::tokens::TREASURE_TOKEN;
    use crate::{names, props};

    const CONVERTER: &str = names::CURRENCY_CONVERTER;

    const fn count(
        player: beryl_engine::Seat,
        zone: Zone,
        card: &'static str,
        count: u8,
    ) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: Some(card),
            count,
        })
    }

    const fn controls(card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Controls {
            player: P0,
            card,
            count,
        })
    }

    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];

    const ISLAND: Permanent = Permanent::new(names::ISLAND);

    const PERMANENT: Permanent = Permanent::new(CONVERTER);

    /// The Converter and the {2} for its loot.
    const LOOTING: &[Permanent] = &[PERMANENT, ISLAND, ISLAND];

    /// The Converter, the {2} for its loot and the {1} to cast it again.
    const LOOTING_AND_RECASTING: &[Permanent] = &[PERMANENT, ISLAND, ISLAND, ISLAND];

    /// The Converter and the {2}{U} for Thirst for Discovery.
    const CASTING_THIRST: &[Permanent] = &[PERMANENT, ISLAND, ISLAND, ISLAND];

    /// The Converter, the {2} for its loot, and Jace, Vryn's Prodigy to loot too.
    const LOOTING_WITH_JACE: &[Permanent] = &[
        PERMANENT,
        ISLAND,
        ISLAND,
        Permanent::new(names::JACE_VRYNS_PRODIGY),
    ];

    /// The Converter, the {2} for its loot, and a tapped creature, which can't attack on the
    /// first turn.
    const LOOTING_BESIDE_A_CREATURE: &[Permanent] = &[
        PERMANENT,
        ISLAND,
        ISLAND,
        Permanent::new(props::CREATURE).tapped(),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // The loot discards Oust, which the trigger exiles. Next turn the Converter puts it into
        // the graveyard: a nonland card, so a Rogue.
        Scenario::new("Currency Converter exiles its own loot's discard for a Rogue")
            .battlefield(P0, LOOTING)
            .hand(P0, &[names::OUST])
            .library(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::activate(P0, CONVERTER, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::OUST),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                count(P0, Zone::Exile, names::OUST, 1),
                count(P0, Zone::Graveyard, names::OUST, 0),
                count(P0, Zone::Hand, names::PLAINS, 1),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P0, CONVERTER, 2),
                Action::Resolve,
                count(P0, Zone::Graveyard, names::OUST, 1),
                controls(ROGUE_TOKEN, 1),
                Action::assert(Assertion::Pt {
                    player: P0,
                    card: ROGUE_TOKEN,
                    token: true,
                    power: 2,
                    toughness: 2,
                }),
                controls(TREASURE_TOKEN, 0),
            ]),
        // Jace's loot discards an Island; the Converter may exile it but doesn't, so its {T} finds
        // nothing to return and makes nothing.
        Scenario::new("Currency Converter declines to exile another loot's discard")
            .interaction(&[names::JACE_VRYNS_PRODIGY])
            .battlefield(
                P0,
                &[
                    Permanent::new(CONVERTER),
                    Permanent::new(names::JACE_VRYNS_PRODIGY),
                ],
            )
            .hand(P0, &[names::ISLAND])
            .library(P0, &[names::PLAINS, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::activate(P0, names::JACE_VRYNS_PRODIGY, 0),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::No),
                count(P0, Zone::Graveyard, names::ISLAND, 1),
                Action::activate(P0, CONVERTER, 2),
                Action::Resolve,
                count(P0, Zone::Graveyard, names::ISLAND, 1),
                controls(TREASURE_TOKEN, 0),
                controls(ROGUE_TOKEN, 0),
            ]),
        // Thirst for Discovery discards Tundra and Counterspell, one at a time: one trigger for
        // each card, both exiled.
        Scenario::new("Currency Converter exiles Thirst for Discovery's discards")
            .interaction(&[names::THIRST_FOR_DISCOVERY])
            .battlefield(P0, CASTING_THIRST)
            .hand(P0, &[names::THIRST_FOR_DISCOVERY])
            .library(
                P0,
                &[
                    names::TUNDRA,
                    props::ARTIFACT,
                    names::COUNTERSPELL,
                    names::ISLAND,
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::THIRST_FOR_DISCOVERY),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::TUNDRA),
                Action::choose(P0, DecisionKind::ChooseObject, names::COUNTERSPELL),
                // The two triggers carry different cards, so their order is a choice.
                Action::choose(P0, DecisionKind::TriggerOrder, CONVERTER),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                count(P0, Zone::Exile, names::TUNDRA, 1),
                count(P0, Zone::Exile, names::COUNTERSPELL, 1),
                count(P0, Zone::Hand, props::ARTIFACT, 1),
            ]),
        // Jace's loot discards an Island and the Converter's own loot Oust: the Converter exiles
        // both. Next turn it returns the land of the two, for a Treasure.
        Scenario::new("Currency Converter chooses among its exiled cards")
            .interaction(&[names::JACE_VRYNS_PRODIGY])
            .battlefield(P0, LOOTING_WITH_JACE)
            .hand(P0, &[names::ISLAND, names::OUST])
            .library(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::activate(P0, names::JACE_VRYNS_PRODIGY, 0),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::activate(P0, CONVERTER, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::OUST),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                count(P0, Zone::Exile, names::ISLAND, 1),
                count(P0, Zone::Exile, names::OUST, 1),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P0, CONVERTER, 2),
                Action::Resolve,
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    candidates: 2,
                }),
                Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
                count(P0, Zone::Graveyard, names::ISLAND, 1),
                count(P0, Zone::Exile, names::OUST, 1),
                controls(TREASURE_TOKEN, 1),
                controls(ROGUE_TOKEN, 0),
            ]),
        // P1 returns the Converter to P0's hand in response to its {T}. The ability still refers
        // to the cards that Converter exiled (CR 607.2a), so Oust goes to the graveyard for a
        // Rogue. The creature makes the target a choice, which the oracles are then told.
        Scenario::new("Currency Converter's ability returns a card after the Converter left")
            .battlefield(P0, LOOTING_BESIDE_A_CREATURE)
            .hand(P0, &[names::OUST, props::SMALL_CREATURE])
            .library(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
            .battlefield(P1, ISLANDS)
            .hand(P1, &[names::SINK_INTO_STUPOR])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::activate(P0, CONVERTER, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::OUST),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                // The two-drop in P0's hand, with flash, makes each of P0's priorities a decision.
                Action::activate(P0, CONVERTER, 2),
                Action::pass(P0),
                Action::cast(P1, names::SINK_INTO_STUPOR),
                Action::choose(P1, DecisionKind::Target, CONVERTER),
                Action::Resolve,
                count(P0, Zone::Hand, CONVERTER, 1),
                Action::Resolve,
                count(P0, Zone::Graveyard, names::OUST, 1),
                controls(ROGUE_TOKEN, 1),
            ]),
        // P1 returns the Converter to P0's hand after it exiled Oust. Recast, it is a new object
        // (CR 400.7), and Oust was exiled with the old one: its {T} finds nothing.
        Scenario::new("Currency Converter's exiled cards stay with the Converter that exiled them")
            .battlefield(P0, LOOTING_AND_RECASTING)
            .hand(P0, &[names::OUST])
            .library(P0, &[names::PLAINS, names::PLAINS])
            .battlefield(P1, ISLANDS)
            .hand(P1, &[names::SINK_INTO_STUPOR])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::activate(P0, CONVERTER, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::OUST),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::pass(P0),
                Action::cast(P1, names::SINK_INTO_STUPOR),
                count(P0, Zone::Hand, CONVERTER, 1),
                Action::cast(P0, CONVERTER),
                Action::Resolve,
                Action::activate(P0, CONVERTER, 2),
                Action::Resolve,
                count(P0, Zone::Exile, names::OUST, 1),
                controls(ROGUE_TOKEN, 0),
            ]),
    ];
}
