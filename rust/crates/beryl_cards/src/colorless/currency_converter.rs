//! Currency Converter. {1} Artifact. "Whenever you discard a card, you may exile that card
//! from your graveyard. {2}, {T}: Draw a card, then discard a card. {T}: Put a card exiled
//! with this artifact into its owner's graveyard. If it's a land card, create a Treasure
//! token. If it's a nonland card, create a 2/2 black Rogue creature token."

use beryl_card::Effect;
use beryl_card::effect::build::{YOU, discard, draw, sequence};
use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Colors, Cost, Face, Flow, ManaCost, Needs, Place,
    Prompt, Purpose, Subtype, Subtypes, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::{Reason, TokenEntry};
use beryl_engine::{Bitboard, DecisionKind, TurnStep, Zone};
use beryl_engine::{Layout, bodies};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;
use crate::tokens::{TREASURE, TREASURE_TOKEN};

/// The Rogue token's name: its subtype plus the word "Token"
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)).
pub const ROGUE_TOKEN: &str = "Rogue Token";

/// The Rogue token: a 2/2 black Rogue creature token.
pub static ROGUE: CardDef = CardDef {
    name: ROGUE_TOKEN,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Rogue]),
        colors: Colors::BLACK,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "That card" is the card as it was put into the graveyard: once it has left, it is a new
/// object ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) and nothing is
/// asked. The exile is linked to the last ability
/// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
/// refers only to cards in the exile zone that were put there as a result of an instruction
/// to exile them in the first ability."
fn exile_discarded<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(card) = ctx.trigger().object else {
        return Ok(());
    };
    let s = ctx.state();
    if !s.is_current(card) || s.object(card.slot).zone != Zone::Graveyard {
        return Ok(());
    }
    let exile = Prompt::new(Purpose::Move).places(Place::Exile, Place::None);
    if ctx.choose_yes_no(ctx.controller(), exile, Bitboard::single(card.slot))? {
        ctx.exile_linked(card);
    }
    Ok(())
}

/// Draws, then discards ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)):
/// "The controller of the spell or ability follows its instructions in the order written."
static LOOT: Effect = sequence(&[draw(YOU, 1), discard(YOU, 1)]);

/// One of the cards exiled with this Converter, its controller's choice, goes to its owner's
/// graveyard (CR 400.3); what it is decides the token. Off the stack and the battlefield a
/// modal double-faced card has only its front face's characteristics
/// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a)), so Sink into Stupor is
/// a nonland card.
fn convert<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let exiled = ctx.exiled_with_source();
    if exiled.is_empty() {
        return Ok(());
    }
    let slot = ctx.choose_object(
        ctx.controller(),
        exiled,
        Prompt::new(Purpose::Choose).most(1),
    )?;
    let card = ctx.state().object_ref(slot);
    // A card in exile has its front face's characteristics (CR 712.8a).
    let face = ctx.state().copiable(slot).face(ctx.table());
    let land = face.types.contains(Types::LAND);
    ctx.move_to(card, Zone::Graveyard, Reason::Effect);
    if land {
        ctx.create_tokens(&TREASURE, 1, TokenEntry::default());
    } else {
        ctx.create_tokens(&ROGUE, 1, TokenEntry::default());
    }
    Ok(())
}

pub static CARD: CardDef = CardDef {
    name: names::CURRENCY_CONVERTER,
    faces: &[Face {
        cost: ManaCost::generic(1),
        types: Types::ARTIFACT,
        abilities: &[
            Ability {
                may_pause: true,
                ..Ability::triggered(Trigger::YouDiscard, bodies!(exile_discarded))
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost {
                    mana: ManaCost::generic(2),
                    ..Cost::TAP
                },
                body: Behavior::Data(&LOOT),
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::TAP,
                may_pause: true,
                body: bodies!(convert),
                needs: Needs::ExiledWithSource,
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[&TREASURE, &ROGUE],
};

const CONVERTER: &str = names::CURRENCY_CONVERTER;

const fn count(player: beryl_engine::Seat, zone: Zone, card: &'static str, count: u8) -> Action {
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

/// The Converter, the {2} for its loot, and a tapped Jace, Vryn's Prodigy, which can neither
/// loot nor attack on the first turn.
const LOOTING_BESIDE_JACE: &[Permanent] = &[
    PERMANENT,
    ISLAND,
    ISLAND,
    Permanent::new(names::JACE_VRYNS_PRODIGY).tapped(),
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
            Action::choose(P0, DecisionKind::Discard, names::OUST),
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
            Action::choose(P0, DecisionKind::Discard, names::ISLAND),
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
        .battlefield(P0, CASTING_THIRST)
        .hand(P0, &[names::THIRST_FOR_DISCOVERY])
        .library(
            P0,
            &[
                names::TUNDRA,
                names::MANA_CRYPT,
                names::COUNTERSPELL,
                names::ISLAND,
            ],
        )
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::THIRST_FOR_DISCOVERY),
            Action::Resolve,
            Action::choose(P0, DecisionKind::Discard, names::TUNDRA),
            Action::choose(P0, DecisionKind::Discard, names::COUNTERSPELL),
            // The two triggers carry different cards, so their order is a choice.
            Action::choose(P0, DecisionKind::TriggerOrder, CONVERTER),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            count(P0, Zone::Exile, names::TUNDRA, 1),
            count(P0, Zone::Exile, names::COUNTERSPELL, 1),
            count(P0, Zone::Hand, names::MANA_CRYPT, 1),
        ]),
    // Jace's loot discards an Island and the Converter's own loot Oust: the Converter exiles
    // both. Next turn it returns the land of the two, for a Treasure.
    Scenario::new("Currency Converter chooses among its exiled cards")
        .battlefield(P0, LOOTING_WITH_JACE)
        .hand(P0, &[names::ISLAND, names::OUST])
        .library(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
        .library(P1, &[names::ISLAND, names::ISLAND])
        .actions(&[
            Action::activate(P0, names::JACE_VRYNS_PRODIGY, 0),
            Action::Resolve,
            Action::choose(P0, DecisionKind::Discard, names::ISLAND),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::activate(P0, CONVERTER, 1),
            Action::Resolve,
            Action::choose(P0, DecisionKind::Discard, names::OUST),
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
    // Rogue. Jace makes the target a choice, which the oracles are then told.
    Scenario::new("Currency Converter's ability returns a card after the Converter left")
        .battlefield(P0, LOOTING_BESIDE_JACE)
        .hand(P0, &[names::OUST])
        .library(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
        .battlefield(P1, ISLANDS)
        .hand(P1, &[names::SINK_INTO_STUPOR])
        .library(P1, &[names::ISLAND, names::ISLAND])
        .actions(&[
            Action::activate(P0, CONVERTER, 1),
            Action::Resolve,
            Action::choose(P0, DecisionKind::Discard, names::OUST),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
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
            Action::choose(P0, DecisionKind::Discard, names::OUST),
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
