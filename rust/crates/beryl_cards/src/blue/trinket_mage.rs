//! Trinket Mage. {2}{U} Creature — Human Wizard, 2/2. "When this creature enters, you may
//! search your library for an artifact card with mana value 1 or less, reveal that card, put
//! it into your hand, then shuffle."

use beryl_engine::card::{
    Ability, CardDef, Colors, Face, Flow, ManaCost, Subtype, Subtypes, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Reason;
use beryl_engine::filter::Filter;
use beryl_engine::{Ask, DecisionKind, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

/// "An artifact card with mana value 1 or less".
const TRINKET: Filter = Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::ManaValueAtMost(1)]);

/// "You may" covers the whole instruction, the shuffle with it: declining searches nothing
/// and shuffles nothing. Searching, the player may find nothing
/// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)) and shuffles all the
/// same ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To shuffle a
/// library ..., randomize the cards within it so that no player knows their order." The card
/// found is revealed ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)):
/// "To reveal a card, show that card to all players for a brief time."
fn tutor(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    if !ctx.choose_yes_no(you, Ask::Plain)? {
        return Ok(());
    }
    if let Some(slot) = ctx.search_library(you, TRINKET)? {
        ctx.reveal(slot);
        ctx.move_to(ctx.state().object_ref(slot), Zone::Hand, Reason::Effect);
    }
    ctx.shuffle(you);
    Ok(())
}

pub static CARD: CardDef = CardDef {
    name: names::TRINKET_MAGE,
    faces: &[Face {
        cost: ManaCost::new(2, [0, 1, 0, 0, 0, 0]),
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Human, Subtype::Wizard]),
        colors: Colors::BLUE,
        power: Some(2),
        toughness: Some(2),
        abilities: &[Ability {
            may_pause: true,
            ..Ability::triggered(Trigger::Enters, tutor)
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const MAGE: &str = names::TRINKET_MAGE;

const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];

const fn count(zone: Zone, card: Option<&'static str>, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player: P0,
        zone,
        card,
        count,
    })
}

/// Mana Crypt (mana value 0) and Portable Hole (1) qualify; Staff of the Storyteller is an
/// artifact of mana value 2, and Oust and the lands are not artifacts.
const LIBRARY: &[&str] = &[
    names::ISLAND,
    names::STAFF_OF_THE_STORYTELLER,
    names::PORTABLE_HOLE,
    names::OUST,
    names::MANA_CRYPT,
    names::PLAINS,
];

/// Casts the Mage and resolves it, then its trigger, up to the "you may".
const ENTER: [Action; 3] = [Action::cast(P0, MAGE), Action::Resolve, Action::Resolve];

pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Trinket Mage finds Mana Crypt")
        .hand(P0, &[MAGE])
        .battlefield(P0, ISLANDS)
        .library(P0, LIBRARY)
        .library(P1, &[names::PLAINS])
        .actions(&[
            ENTER[0],
            ENTER[1],
            ENTER[2],
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            // Finding nothing, the Crypt or the Hole.
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::ChooseUpTo,
                candidates: 3,
            }),
            Action::choose(P0, DecisionKind::ChooseUpTo, names::MANA_CRYPT),
            count(Zone::Hand, Some(names::MANA_CRYPT), 1),
            count(Zone::Library, None, 5),
            count(Zone::Battlefield, Some(MAGE), 1),
        ]),
    Scenario::new("Trinket Mage finds Portable Hole")
        .hand(P0, &[MAGE])
        .battlefield(P0, ISLANDS)
        .library(P0, LIBRARY)
        .library(P1, &[names::PLAINS])
        .actions(&[
            ENTER[0],
            ENTER[1],
            ENTER[2],
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::choose(P0, DecisionKind::ChooseUpTo, names::PORTABLE_HOLE),
            count(Zone::Hand, Some(names::PORTABLE_HOLE), 1),
            count(Zone::Library, Some(names::MANA_CRYPT), 1),
        ]),
    Scenario::new("Trinket Mage finds nothing although Mana Crypt is there")
        .hand(P0, &[MAGE])
        .battlefield(P0, ISLANDS)
        .library(P0, LIBRARY)
        .library(P1, &[names::PLAINS])
        .actions(&[
            ENTER[0],
            ENTER[1],
            ENTER[2],
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
            count(Zone::Hand, None, 0),
            count(Zone::Library, Some(names::MANA_CRYPT), 1),
            count(Zone::Library, None, 6),
        ]),
    // With no artifact of mana value 1 or less there is nothing to choose: the search finds
    // nothing without a question.
    Scenario::new("Trinket Mage searches a library without a trinket")
        .hand(P0, &[MAGE])
        .battlefield(P0, ISLANDS)
        .library(
            P0,
            &[names::STAFF_OF_THE_STORYTELLER, names::OUST, names::PLAINS],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            ENTER[0],
            ENTER[1],
            ENTER[2],
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            count(Zone::Hand, None, 0),
            count(Zone::Library, None, 3),
            Action::assert(Assertion::Stack { count: 0 }),
        ]),
    Scenario::new("Trinket Mage declines to search")
        .hand(P0, &[MAGE])
        .battlefield(P0, ISLANDS)
        .library(P0, LIBRARY)
        .library(P1, &[names::PLAINS])
        .actions(&[
            ENTER[0],
            ENTER[1],
            ENTER[2],
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            count(Zone::Hand, None, 0),
            // Unsearched and unshuffled.
            Action::assert(Assertion::Library {
                player: P0,
                top: LIBRARY,
            }),
        ]),
];
