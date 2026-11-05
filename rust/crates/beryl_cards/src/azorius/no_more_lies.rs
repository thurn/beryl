//! No More Lies. {W}{U} Instant. "Counter target spell unless its controller pays {3}. If that
//! spell is countered this way, exile it instead of putting it into its owner's graveyard."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaCost, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::{DecisionKind, Seat, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::NO_MORE_LIES,
    faces: &[Face {
        cost: ManaCost::new(0, [1, 1, 0, 0, 0, 0]),
        types: Types::INSTANT,
        colors: Colors::WHITE.union(Colors::BLUE),
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any)],
            may_pause: true,
            body: counter_unless_pays_three,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Unless" as [CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a) reads it. The
/// second sentence modifies the first
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "later text on the card
/// may modify the meaning of earlier text (for example, ... 'Counter target spell. If that
/// spell is countered this way, put it on top of its owner's library instead of into its
/// owner's graveyard.')", so the countered spell goes straight to exile.
fn counter_unless_pays_three(ctx: &mut EffectCtx) -> Flow {
    let Some(spell) = ctx.target_object(0) else {
        return Ok(());
    };
    let Some(controller) = ctx.spell_controller(spell) else {
        return Ok(());
    };
    if !ctx.unless_pays(controller, ManaCost::generic(3))? {
        ctx.counter(spell, true);
    }
    Ok(())
}

const P0_LANDS: &[Permanent] = &[
    Permanent::new(names::PLAINS),
    Permanent::new(names::ISLAND),
    Permanent::new(names::STRIP_MINE),
];
const P1_LANDS: &[Permanent] = &[Permanent::new(names::PLAINS), Permanent::new(names::ISLAND)];

const fn count(player: Seat, zone: Zone, card: &'static str, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player,
        zone,
        card: Some(card),
        count,
    })
}

/// P1 has a card to draw, so the game waits in their turn 2 once the stack is empty. Mana
/// Crypt is the spell to counter, so P0's three lands stay free to pay.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("No More Lies: the controller pays {3}")
        .battlefield(P0, P0_LANDS)
        .hand(P0, &[names::MANA_CRYPT])
        .battlefield(P1, P1_LANDS)
        .hand(P1, &[names::NO_MORE_LIES])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::MANA_CRYPT),
            Action::pass(P0),
            Action::cast(P1, names::NO_MORE_LIES),
            Action::pass(P0),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            count(P0, Zone::Battlefield, names::MANA_CRYPT, 1),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::STRIP_MINE,
                count: 1,
            }),
            count(P1, Zone::Graveyard, names::NO_MORE_LIES, 1),
        ]),
    Scenario::new("No More Lies: declining to pay exiles the spell")
        .battlefield(P0, P0_LANDS)
        .hand(P0, &[names::MANA_CRYPT])
        .battlefield(P1, P1_LANDS)
        .hand(P1, &[names::NO_MORE_LIES])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::MANA_CRYPT),
            Action::pass(P0),
            Action::cast(P1, names::NO_MORE_LIES),
            Action::pass(P0),
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            count(P0, Zone::Exile, names::MANA_CRYPT, 1),
            count(P0, Zone::Graveyard, names::MANA_CRYPT, 0),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::STRIP_MINE,
                count: 0,
            }),
        ]),
    Scenario::new("No More Lies: a controller who can't pay isn't asked")
        .battlefield(P0, &[Permanent::new(names::PLAINS)])
        .hand(P0, &[names::MANA_CRYPT])
        .battlefield(P1, P1_LANDS)
        .hand(P1, &[names::NO_MORE_LIES])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::MANA_CRYPT),
            Action::cast(P1, names::NO_MORE_LIES),
            count(P0, Zone::Exile, names::MANA_CRYPT, 1),
            count(P0, Zone::Graveyard, names::MANA_CRYPT, 0),
        ]),
];
