//! Fractured Identity. {3}{W}{U} Sorcery. "Exile target nonland permanent. Each player other
//! than its controller creates a token that's a copy of it."

use beryl_engine::Zone;
use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaCost, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::{Reason, TokenEntry};
use beryl_engine::filter::Filter;
use beryl_engine::state::TargetPlace;
use beryl_engine::{Layout, bodies};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::FRACTURED_IDENTITY,
    faces: &[Face {
        cost: ManaCost::new(3, [1, 1, 0, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::WHITE.union(Colors::BLUE),
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Not(&Filter::Types(Types::LAND)),
            )
            .removal()],
            body: bodies!(fracture),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Its controller" and "it" are the permanent as it last existed on the battlefield
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect requires
/// information from the game (such as the number of creatures on the battlefield), the answer
/// is determined only once, when the effect is applied. If the effect requires information
/// from a specific object, including the source of the ability itself, the effect uses the
/// current information of that object if it's in the public zone it was expected to be in;
/// if it's no longer in that zone, ... the effect uses the object's last known information."
/// So an exiled token, which ceases to exist, is copied all the same (CR 111.12). Each copy
/// has the permanent's copiable values ([`beryl_engine::State::copiable`], CR 707.2): not its
/// counters, so a planeswalker's copy enters with its printed loyalty (CR 306.5b).
fn fracture<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(permanent) = ctx.target_object(TargetPlace::FIRST) else {
        return Ok(());
    };
    let controller = ctx.state().controller(permanent.slot);
    let what = ctx.state().copiable(permanent.slot);
    ctx.move_to(permanent, Zone::Exile, Reason::Exile);
    // CR 111.2: "The player who creates a token is its owner. The token enters the
    // battlefield under that player's control."
    let others = ctx.state().players().without(controller);
    for player in ctx.state().players_in_turn_order(others) {
        ctx.create_tokens_of_for(player, what, 1, TokenEntry::default());
    }
    Ok(())
}

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::state::CounterKind;

    use beryl_engine::{Seat, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::names;
    use crate::white::the_wandering_emperor::SAMURAI_TOKEN;

    const IDENTITY: &str = names::FRACTURED_IDENTITY;

    const RAVELER: &str = names::TEFERI_TIME_RAVELER;

    const EMPEROR: &str = names::THE_WANDERING_EMPEROR;

    const JACE: &str = names::JACE_VRYNS_PRODIGY;

    /// Jace, Vryn's Prodigy's back face (CR 712.6d).
    const TELEPATH: &str = "Jace, Telepath Unbound";

    const FIVE_LANDS: &[Permanent] = &[
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
    ];

    const fn controls(player: Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Controls {
            player,
            card,
            count,
        })
    }

    const fn loyalty(player: Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Counters {
            player,
            card,
            kind: CounterKind::Loyalty,
            count,
        })
    }

    /// P0 casts Fractured Identity on P1's one nonland permanent, `target`; P0 gets a copy of it,
    /// which `then` checks.
    const fn fracture_of(
        name: &'static str,
        target: &'static [Permanent],
        then: &'static [Action],
    ) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, FIVE_LANDS)
            .hand(P0, &[IDENTITY])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, target)
            .library(P1, &[names::ISLAND])
            .actions(then)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Counters are not copied: P1's Teferi at 6 loyalty becomes P0's at 4.
        fracture_of(
            "Fractured Identity on a planeswalker: the copy has its printed loyalty",
            &[Permanent::new(RAVELER).counters(CounterKind::Loyalty, 6)],
            &[
                Action::cast(P0, IDENTITY),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(RAVELER),
                    count: 1,
                }),
                controls(P1, RAVELER, 0),
                controls(P0, RAVELER, 1),
                loyalty(P0, RAVELER, 4),
            ],
        ),
        fracture_of(
            "Fractured Identity on a double-faced card front face up",
            &[Permanent::new(JACE)],
            &[
                Action::cast(P0, IDENTITY),
                Action::Resolve,
                controls(P1, JACE, 0),
                controls(P0, JACE, 1),
                Action::assert(Assertion::Creature {
                    player: P0,
                    card: JACE,
                    creature: true,
                }),
            ],
        ),
        // CR 707.8a: the copy of a transformed permanent enters with its back face up.
        fracture_of(
            "Fractured Identity on a transformed double-faced card",
            &[Permanent::new(JACE).transformed()],
            &[
                Action::cast(P0, IDENTITY),
                Action::Resolve,
                controls(P1, TELEPATH, 0),
                controls(P0, JACE, 0),
                controls(P0, TELEPATH, 1),
                loyalty(P0, TELEPATH, 5),
            ],
        ),
        // P0 targets their own Samurai token: it is exiled and ceases to exist, and P1, the one
        // other player, gets the copy, a 2/2.
        Scenario::new("Fractured Identity on its caster's own token")
            .battlefield(
                P0,
                &[
                    Permanent::new(EMPEROR),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[IDENTITY])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::activate(P0, EMPEROR, 2),
                Action::Resolve,
                controls(P0, SAMURAI_TOKEN, 1),
                Action::cast(P0, IDENTITY),
                Action::choose(P0, beryl_engine::DecisionKind::Target, SAMURAI_TOKEN),
                Action::Resolve,
                controls(P0, SAMURAI_TOKEN, 0),
                controls(P0, EMPEROR, 1),
                controls(P1, SAMURAI_TOKEN, 1),
                Action::assert(Assertion::Pt {
                    player: P1,
                    card: SAMURAI_TOKEN,
                    token: true,
                    power: 2,
                    toughness: 2,
                }),
            ]),
    ];
}
