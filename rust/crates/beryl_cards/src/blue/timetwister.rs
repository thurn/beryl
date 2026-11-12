//! Timetwister. {2}{U} Sorcery. "Each player shuffles their hand and graveyard into their
//! library, then draws seven cards. (Then put Timetwister into its owner's graveyard.)"

use beryl_engine::card::{Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaCost, Types};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Reason;
use beryl_engine::{Layout, bodies};

use beryl_engine::Zone;

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::TIMETWISTER,
    faces: &[Face {
        cost: ManaCost::new(2, [0, 1, 0, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::BLUE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            body: bodies!(twist),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn twist<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    wheel(ctx);
    Ok(())
}

/// "Each player shuffles their hand and graveyard into their library, then draws seven
/// cards." The cards go to their libraries together, each to its owner's
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)), and each library is
/// shuffled ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)). Then each
/// player draws seven, one card at a time
/// ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)), in APNAP order
/// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)): "the active player ...
/// makes any choices required, then the next player in turn order ... Then the actions happen
/// simultaneously." No draw involves a choice, so the order changes nothing: a player who draws
/// from an empty library loses as state-based actions are next checked either way (CR 704.5b).
pub(crate) fn wheel<L: Layout>(ctx: &mut EffectCtx<'_, L>) {
    let s = ctx.state();
    let players = s.players_in_turn_order(s.players());
    let cards = s.in_zone(Zone::Hand) | s.in_zone(Zone::Graveyard);
    ctx.move_all(cards, Zone::Library, Reason::Effect);
    for p in players.clone() {
        ctx.shuffle(p);
    }
    for p in players {
        ctx.draw(p, 7);
    }
}

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::{Seat, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::names;

    const fn count(player: Seat, zone: Zone, card: Option<&'static str>, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card,
            count,
        })
    }

    const THREE_ISLANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
    ];

    /// Each player ends with seven cards in hand and the rest of hand, graveyard and library in
    /// the library; Timetwister alone goes to the graveyard, after the shuffle.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Timetwister with empty graveyards")
            .battlefield(P0, THREE_ISLANDS)
            .hand(P0, &[names::TIMETWISTER, names::PLAINS])
            .library(P0, &[names::PLAINS; 7])
            .hand(P1, &[names::ISLAND, names::ISLAND])
            .library(P1, &[names::ISLAND; 6])
            .actions(&[
                Action::cast(P0, names::TIMETWISTER),
                count(P0, Zone::Hand, None, 7),
                count(P0, Zone::Library, None, 1),
                count(P0, Zone::Graveyard, Some(names::TIMETWISTER), 1),
                count(P0, Zone::Graveyard, None, 1),
                count(P1, Zone::Hand, None, 7),
                count(P1, Zone::Library, None, 1),
            ]),
        Scenario::new("Timetwister shuffles graveyards away")
            .battlefield(P0, THREE_ISLANDS)
            .hand(P0, &[names::TIMETWISTER])
            .graveyard(P0, &[names::MANA_TITHE, names::OUST])
            .library(P0, &[names::PLAINS; 5])
            .graveyard(P1, &[names::MANA_DRAIN])
            .library(P1, &[names::ISLAND; 8])
            .actions(&[
                Action::cast(P0, names::TIMETWISTER),
                count(P0, Zone::Hand, None, 7),
                count(P0, Zone::Library, None, 0),
                count(P0, Zone::Graveyard, None, 1),
                count(P1, Zone::Hand, None, 7),
                count(P1, Zone::Graveyard, None, 0),
                count(P1, Zone::Library, None, 2),
            ]),
    ];
}
