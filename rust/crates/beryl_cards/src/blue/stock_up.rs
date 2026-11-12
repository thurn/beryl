//! Stock Up. {2}{U} Sorcery. "Look at the top five cards of your library. Put two of them
//! into your hand and the rest on the bottom of your library in any order."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaCost, Part, Place, Prompt, Purpose,
    Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Reason;
use beryl_engine::{Bitboard, Zone};
use beryl_engine::{Layout, bodies};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::STOCK_UP,
    faces: &[Face {
        cost: ManaCost::new(2, [0, 1, 0, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::BLUE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            may_pause: true,
            body: bodies!(stock_up),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const LOOK: u8 = 5;

const TAKE: u8 = 2;

/// The two for the hand are chosen one at a time and go there together; with two or fewer
/// cards left to choose from, they all go unasked. The rest go to the bottom one at a time,
/// each under the ones before, which is every order
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two or
/// more cards in a specific position in a library at the same time, the owner of those cards
/// may arrange them in any order." With fewer than five cards in the library, the player
/// looks at them all.
fn stock_up<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let you = ctx.controller();
    let looked = ctx.top_of_library(you, LOOK);
    ctx.look_at(you, looked);
    let count = looked.count() as u8;
    // One prompt: the cards for the hand, then the order of the rest on the bottom.
    let arrange = Prompt::new(Purpose::Arrange).places(Place::Hand, Place::Bottom);
    let mut taken = Bitboard::EMPTY;
    if count <= TAKE {
        taken = looked;
    } else {
        for take in (1..=TAKE).rev() {
            let hand = arrange.count(count).most(take).part(Part::First);
            let slot = ctx.choose_object(you, looked & !taken, hand)?;
            taken.insert(slot);
        }
    }
    ctx.move_all(taken, Zone::Hand, Reason::Effect);
    let mut rest = looked & !taken;
    while !rest.is_empty() {
        let count = rest.count() as u8;
        let bottom = arrange.count(count).part(Part::Second);
        let slot = ctx.choose_object(you, rest, bottom)?;
        ctx.put_library_card_on_bottom(ctx.state().object_ref(slot));
        rest.remove(slot);
    }
    Ok(())
}

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];

    const fn in_hand(card: &'static str) -> Action {
        Action::assert(Assertion::Count {
            player: P0,
            zone: Zone::Hand,
            card: Some(card),
            count: 1,
        })
    }

    const fn take(card: &'static str) -> Action {
        Action::choose(P0, DecisionKind::ChooseObject, card)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // The artifact and Counterspell to the hand; Oust, then Plains, then Island to the
        // bottom, each under the one before. The sixth card, Tundra, is now on top.
        Scenario::new("Stock Up takes two of five and orders the rest on the bottom")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::STOCK_UP])
            .library(
                P0,
                &[
                    names::ISLAND,
                    names::PLAINS,
                    props::ARTIFACT,
                    names::COUNTERSPELL,
                    props::SORCERY,
                    names::TUNDRA,
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::STOCK_UP),
                Action::Resolve,
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    candidates: 5,
                }),
                take(props::ARTIFACT),
                take(names::COUNTERSPELL),
                take(props::SORCERY),
                take(names::PLAINS),
                in_hand(props::ARTIFACT),
                in_hand(names::COUNTERSPELL),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[names::TUNDRA, props::SORCERY, names::PLAINS, names::ISLAND],
                }),
            ]),
        // Four cards: two to the hand, and the order of the other two on the bottom.
        Scenario::new("Stock Up with four cards in the library")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::STOCK_UP])
            .library(
                P0,
                &[
                    names::PLAINS,
                    props::ARTIFACT,
                    names::ISLAND,
                    names::COUNTERSPELL,
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::STOCK_UP),
                Action::Resolve,
                take(props::ARTIFACT),
                take(names::COUNTERSPELL),
                take(names::ISLAND),
                in_hand(props::ARTIFACT),
                in_hand(names::COUNTERSPELL),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[names::ISLAND, names::PLAINS],
                }),
            ]),
        // Two cards: both go to the hand, with nothing to choose.
        Scenario::new("Stock Up with two cards in the library")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::STOCK_UP])
            .library(P0, &[props::ARTIFACT, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::STOCK_UP),
                Action::Resolve,
                in_hand(props::ARTIFACT),
                in_hand(names::PLAINS),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Library,
                    card: None,
                    count: 0,
                }),
            ]),
    ];
}
