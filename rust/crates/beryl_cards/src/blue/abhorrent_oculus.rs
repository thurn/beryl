//! Abhorrent Oculus. {2}{U} Creature — Eye, 5/5. "As an additional cost to cast this spell,
//! exile six cards from your graveyard. Flying. At the beginning of each opponent's upkeep,
//! manifest dread."

use beryl_engine::card::{
    Ability, CardDef, Colors, Face, Flow, Keywords, ManaCost, Subtype, Subtypes, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Whose;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_engine::{Layout, bodies};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

/// Manifest dread ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): "Look
/// at the top two cards of your library. Manifest one of them, then put the cards you looked
/// at that were not manifested this way into your graveyard." The ability's controller does,
/// whether or not Oculus is still there
/// ([CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)).
fn manifest_dread<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let you = ctx.controller();
    ctx.manifest_dread(you)
}

/// The six cards are an additional cost
/// ([CR 118.8](https://yawgatog.com/resources/magic-rules/#R1188)): "Some spells and abilities
/// have additional costs. An additional cost is a cost listed in a spell's rules text, or
/// applied to a spell or ability from another effect, that its controller must pay at the same
/// time they pay the spell's mana cost or the ability's activation cost." The trigger is the
/// opponent's upkeep's alone
/// ([CR 503.1a](https://yawgatog.com/resources/magic-rules/#R5031a)): "Any abilities that
/// triggered during the untap step and any abilities that trigger at the beginning of the
/// upkeep are put onto the stack before the active player gets priority".
pub static CARD: CardDef = CardDef {
    name: names::ABHORRENT_OCULUS,
    faces: &[Face {
        cost: ManaCost::new(2, [0, 1, 0, 0, 0, 0]),
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Eye]),
        colors: Colors::BLUE,
        power: Some(5),
        toughness: Some(5),
        keywords: Keywords::FLYING,
        abilities: &[Ability {
            may_pause: true,
            ..Ability::triggered(
                Trigger::Step(TurnStep::Upkeep, Whose::Opponent),
                bodies!(manifest_dread),
            )
        }],
        exile_from_graveyard: 6,
        ..Face::EMPTY
    }],
    tokens: &[],
};

const OCULUS: &str = names::ABHORRENT_OCULUS;
const SNAPCASTER: &str = names::SNAPCASTER_MAGE;

const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];

/// Six graveyard cards and a seventh at the bottom, top first.
const SEVEN: &[&str] = &[
    names::COUNTERSPELL,
    names::MANA_DRAIN,
    names::STOCK_UP,
    names::REPRIEVE,
    names::OUST,
    names::THIRST_FOR_DISCOVERY,
    names::PLAINS,
];

/// From player 0's turn 1 to player 1's upkeep, where the manifest dread trigger waits.
const TO_THEIR_UPKEEP: Action = Action::AdvanceTo {
    turn: 2,
    step: TurnStep::Upkeep,
};

const fn face_down(card: &'static str, count: u8) -> Action {
    Action::assert(Assertion::FaceDown {
        player: P0,
        card,
        count,
    })
}

pub const SCENARIOS: &[Scenario] = &[
    // With five cards in the graveyard the additional cost can't be paid (CR 118.3), so
    // only passing and the land are offered.
    Scenario::new("Abhorrent Oculus can't be cast with five cards in the graveyard")
        .battlefield(P0, ISLANDS)
        .hand(P0, &[OCULUS, names::ISLAND])
        .graveyard(P0, SEVEN.split_at(5).0)
        .actions(&[Action::assert(Assertion::Decision {
            player: P0,
            kind: DecisionKind::Priority,
            candidates: 2,
        })]),
    // The cards are chosen from the bottom of the graveyard up, so the first pick says which
    // of the bottom two stays; the other five go unasked.
    Scenario::new("Abhorrent Oculus exiles six of seven graveyard cards as it's cast")
        .battlefield(P0, ISLANDS)
        .hand(P0, &[OCULUS])
        .graveyard(P0, SEVEN)
        .actions(&[
            Action::cast(P0, OCULUS),
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::ExileCost,
                candidates: 2,
            }),
            Action::choose(P0, DecisionKind::ExileCost, names::THIRST_FOR_DISCOVERY),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(names::PLAINS),
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Exile,
                card: None,
                count: 6,
            }),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::ISLAND,
                count: 3,
            }),
            Action::Resolve,
            Action::assert(Assertion::Pt {
                player: P0,
                card: OCULUS,
                token: false,
                power: 5,
                toughness: 5,
            }),
        ]),
    // Snapcaster Mage is manifested and Counterspell, the other card looked at, goes to the
    // graveyard. Player 0's own upkeep triggers nothing.
    Scenario::new("Abhorrent Oculus manifests dread on each opponent's upkeep only")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[Permanent::new(OCULUS)])
        .library(
            P0,
            &[
                SNAPCASTER,
                names::COUNTERSPELL,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            TO_THEIR_UPKEEP,
            Action::assert(Assertion::Stack { count: 1 }),
            Action::Resolve,
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::ChooseObject,
                candidates: 2,
            }),
            Action::choose(P0, DecisionKind::ChooseObject, SNAPCASTER),
            face_down(SNAPCASTER, 1),
            // CR 708.2a: a 2/2 creature, whatever its card.
            Action::assert(Assertion::Pt {
                player: P0,
                card: SNAPCASTER,
                token: false,
                power: 2,
                toughness: 2,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(names::COUNTERSPELL),
                count: 1,
            }),
            Action::assert(Assertion::Library {
                player: P0,
                top: &[names::ISLAND, names::ISLAND],
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::Upkeep,
            },
            Action::assert(Assertion::Stack { count: 0 }),
        ]),
    // CR 701.62a: with one card to look at, that one is manifested, unasked.
    Scenario::new("Manifest dread with one card in the library manifests it")
        .at(1, P0, TurnStep::PostcombatMain)
        // A playable instant-speed action lets the Playground pause on the
        // result during the opponent's turn, before the next empty draw.
        .battlefield(
            P0,
            &[
                Permanent::new(OCULUS),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P0, &[names::SNAPCASTER_MAGE])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::PLAINS])
        .actions(&[
            TO_THEIR_UPKEEP,
            Action::Resolve,
            face_down(names::ISLAND, 1),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Library,
                card: None,
                count: 0,
            }),
        ]),
    Scenario::new("Manifest dread with an empty library does nothing")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[Permanent::new(OCULUS)])
        .library(P1, &[names::PLAINS])
        .actions(&[
            TO_THEIR_UPKEEP,
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Battlefield,
                card: None,
                count: 1,
            }),
            Action::assert(Assertion::Stack { count: 0 }),
        ]),
    // CR 116.2b: player 0 turns it face up in player 1's upkeep, paying {1}{U}. Snapcaster
    // Mage's enters trigger, which Counterspell in the graveyard would give a target, doesn't
    // trigger (CR 708.8).
    Scenario::new("A manifested creature turns face up for its mana cost, entering no more")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(OCULUS),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .graveyard(P0, &[names::COUNTERSPELL])
        .library(P0, &[SNAPCASTER, names::ISLAND])
        .library(P1, &[names::PLAINS])
        .actions(&[
            TO_THEIR_UPKEEP,
            Action::Resolve,
            Action::choose(P0, DecisionKind::ChooseObject, SNAPCASTER),
            Action::TurnFaceUp {
                player: P0,
                card: SNAPCASTER,
            },
            face_down(SNAPCASTER, 0),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::ISLAND,
                count: 2,
            }),
            Action::assert(Assertion::Pt {
                player: P0,
                card: SNAPCASTER,
                token: false,
                power: 2,
                toughness: 1,
            }),
            Action::assert(Assertion::Stack { count: 0 }),
        ]),
    // CR 701.40b: a face-down Counterspell is a 2/2 creature, but its card isn't a creature
    // card, so it can't be turned face up: player 0 may only pass or cast Snapcaster Mage.
    Scenario::new("A manifested noncreature card can't be turned face up")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(OCULUS),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P0, &[SNAPCASTER])
        .library(P0, &[names::COUNTERSPELL, names::ISLAND])
        .library(P1, &[names::PLAINS])
        .actions(&[
            TO_THEIR_UPKEEP,
            Action::Resolve,
            Action::choose(P0, DecisionKind::ChooseObject, names::COUNTERSPELL),
            Action::assert(Assertion::Creature {
                player: P0,
                card: names::COUNTERSPELL,
                creature: true,
            }),
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::Priority,
                candidates: 2,
            }),
        ]),
    // CR 708.9: Oust puts the face-down Snapcaster Mage second from the top of its owner's
    // library, revealed as it goes. Its controller gains the 3 life.
    Scenario::new("A face-down permanent leaving the battlefield is revealed")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[Permanent::new(OCULUS)])
        .library(P0, &[SNAPCASTER, names::ISLAND, names::ISLAND])
        .battlefield(P1, &[Permanent::new(names::PLAINS)])
        .hand(P1, &[names::OUST])
        .library(P1, &[names::PLAINS])
        .actions(&[
            TO_THEIR_UPKEEP,
            Action::Resolve,
            Action::choose(P0, DecisionKind::ChooseObject, SNAPCASTER),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::PrecombatMain,
            },
            Action::cast(P1, names::OUST),
            Action::answer(P1, DecisionKind::Target, Pick::Card { card: SNAPCASTER }),
            Action::Resolve,
            Action::assert(Assertion::Library {
                player: P0,
                top: &[names::ISLAND, SNAPCASTER],
            }),
            Action::assert(Assertion::Life {
                player: P0,
                life: 23,
            }),
            // On to combat: the tests read what each player knows here.
            Action::pass(P1),
        ]),
    // CR 708.2: the copy Fractured Identity makes of the face-down Snapcaster Mage is a 2/2
    // with no text, for player 1, the one player other than its controller.
    Scenario::new("Fractured Identity copies a face-down permanent as a 2/2")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[Permanent::new(OCULUS)])
        .library(P0, &[SNAPCASTER, names::ISLAND, names::ISLAND])
        .battlefield(
            P1,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P1, &[names::FRACTURED_IDENTITY])
        .library(P1, &[names::PLAINS])
        .actions(&[
            TO_THEIR_UPKEEP,
            Action::Resolve,
            Action::choose(P0, DecisionKind::ChooseObject, SNAPCASTER),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::PrecombatMain,
            },
            Action::cast(P1, names::FRACTURED_IDENTITY),
            Action::answer(P1, DecisionKind::Target, Pick::Card { card: SNAPCASTER }),
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Exile,
                card: Some(SNAPCASTER),
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Battlefield,
                card: None,
                count: 6,
            }),
            // On to combat: the tests read the copy here.
            Action::pass(P1),
        ]),
];
