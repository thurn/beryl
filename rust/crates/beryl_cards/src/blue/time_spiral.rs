//! Time Spiral. {4}{U}{U} Sorcery. "Exile Time Spiral. Each player shuffles their hand and
//! graveyard into their library, then draws seven cards. You untap up to six lands."

use beryl_engine::Zone;
use beryl_engine::card::{Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaCost, Types};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Reason;
use beryl_engine::{Layout, bodies};

use crate::blue::timetwister::wheel;
use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::TIME_SPIRAL,
    faces: &[Face {
        cost: ManaCost::new(4, [0, 2, 0, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::BLUE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            may_pause: true,
            body: bodies!(spiral),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "You untap up to six lands."
const LANDS: u8 = 6;

/// The instructions in order ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)):
/// "The controller of the spell or ability follows its instructions in the order written."
/// Exiled first, Time Spiral is not shuffled away, and it is not put into a graveyard as the
/// final part of its resolution (CR 608.2n): it is no longer on the stack.
///
/// "Up to six lands" are any lands, whoever controls them
/// ([`EffectCtx::untap_up_to_lands`]).
fn spiral<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.move_to(ctx.source(), Zone::Exile, Reason::Exile);
    wheel(ctx);
    ctx.untap_up_to_lands(LANDS)
}

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::DecisionKind;
    use beryl_engine::Seat;
    use beryl_engine::Zone;

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    /// Six lands to cast it with, all tapped by the casting.
    const SIX_LANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
    ];

    const fn count(player: Seat, zone: Zone, card: Option<&'static str>, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card,
            count,
        })
    }

    const fn tapped(player: Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Tapped {
            player,
            card,
            count,
        })
    }

    const fn untap(card: &'static str) -> Action {
        Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Card { card })
    }

    /// P1's tapped Tundra is a land P0 may untap too. Interchangeable lands are one choice each
    /// time: after two Islands no Island is left to offer.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Time Spiral: exiled, the wheel, and chosen lands untapped")
            .battlefield(P0, SIX_LANDS)
            .hand(P0, &[names::TIME_SPIRAL, names::ISLAND])
            .graveyard(P0, &[names::PLAINS])
            .library(P0, &[names::ISLAND; 6])
            .battlefield(P1, &[Permanent::new(names::TUNDRA).tapped()])
            .hand(P1, &[names::PLAINS])
            .graveyard(P1, &[names::ISLAND])
            .library(P1, &[names::PLAINS; 6])
            .actions(&[
                Action::cast(P0, names::TIME_SPIRAL),
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseUpTo,
                    candidates: 4,
                }),
                untap(names::ISLAND),
                untap(names::ISLAND),
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseUpTo,
                    candidates: 3,
                }),
                untap(names::PLAINS),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                tapped(P0, names::ISLAND, 0),
                tapped(P0, names::PLAINS, 3),
                tapped(P1, names::TUNDRA, 1),
                count(P0, Zone::Exile, Some(names::TIME_SPIRAL), 1),
                count(P0, Zone::Graveyard, None, 0),
                count(P0, Zone::Hand, None, 7),
                count(P0, Zone::Library, None, 1),
                count(P1, Zone::Graveyard, None, 0),
                count(P1, Zone::Hand, None, 7),
                count(P1, Zone::Library, None, 1),
            ]),
        Scenario::new("Time Spiral with empty graveyards: six lands untapped, any player's")
            .battlefield(P0, SIX_LANDS)
            .hand(P0, &[names::TIME_SPIRAL])
            .library(P0, &[names::ISLAND; 7])
            .battlefield(P1, &[Permanent::new(names::TUNDRA).tapped()])
            .library(P1, &[names::PLAINS; 7])
            .actions(&[
                Action::cast(P0, names::TIME_SPIRAL),
                untap(names::TUNDRA),
                untap(names::PLAINS),
                untap(names::PLAINS),
                untap(names::PLAINS),
                untap(names::PLAINS),
                untap(names::ISLAND),
                // Six chosen: no more is asked.
                tapped(P0, names::ISLAND, 1),
                tapped(P0, names::PLAINS, 0),
                tapped(P1, names::TUNDRA, 0),
                count(P0, Zone::Graveyard, None, 0),
                count(P0, Zone::Hand, None, 7),
                count(P1, Zone::Hand, None, 7),
            ]),
    ];
}
