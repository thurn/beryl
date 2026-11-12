//! Reprieve. {1}{W} Instant. "Return target spell to its owner's hand. Draw a card."

use beryl_card::effect::build::{TARGET, YOU, draw, return_to_hand, sequence};
use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Colors, Face, ManaCost, TargetKind, TargetSpec, Types,
};
use beryl_engine::filter::Filter;

use crate::names;

/// The spell leaves the stack without resolving, to its owner's hand
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." The costs paid for it, X and kicker included, are not refunded. Then
/// Reprieve's controller draws: an illegal target would have stopped the whole spell
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)), "If all its targets
/// ... are now illegal, the spell or ability doesn't resolve."
///
/// A copy of a spell returned this way would cease to exist
/// ([CR 704.5e](https://yawgatog.com/resources/magic-rules/#R7045e)): "If a copy of a spell
/// is in a zone other than the stack, it ceases to exist." No card here copies a spell yet.
pub static CARD: CardDef = CardDef {
    name: names::REPRIEVE,
    faces: &[Face {
        cost: ManaCost::new(1, [1, 0, 0, 0, 0, 0]),
        types: Types::INSTANT,
        colors: Colors::WHITE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any)],
            body: Behavior::Data(&sequence(&[return_to_hand(TARGET), draw(YOU, 1)])),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::{DecisionKind, Seat, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    const fn count(player: Seat, zone: Zone, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: Some(card),
            count,
        })
    }

    const P1_LANDS: &[Permanent] = &[Permanent::new(names::PLAINS), Permanent::new(names::ISLAND)];

    pub const SCENARIOS: &[Scenario] = &[
        // Thieving Skydiver kicked for X = 1 taps three of four Islands. Returned to P0's hand,
        // it never enters, and the mana spent on it, X included, stays spent.
        Scenario::new("Reprieve returns a spell with X to its owner's hand and draws")
            .battlefield(P0, &[Permanent::new(names::ISLAND); 4])
            .hand(P0, &[names::THIEVING_SKYDIVER, names::PLAINS])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, P1_LANDS)
            .hand(P1, &[names::REPRIEVE])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: names::THIEVING_SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::answer(P0, DecisionKind::X, Pick::Number { n: 1 }),
                // P0 has nothing to do with the stack full; the Plains to play keeps the game in
                // P0's main phase once it is empty.
                Action::cast(P1, names::REPRIEVE),
                count(P0, Zone::Hand, names::THIEVING_SKYDIVER, 1),
                count(P0, Zone::Battlefield, names::THIEVING_SKYDIVER, 0),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::ISLAND,
                    count: 3,
                }),
                count(P1, Zone::Hand, names::PLAINS, 1),
                count(P1, Zone::Graveyard, names::REPRIEVE, 1),
            ]),
        // P1 counters Mana Crypt; P0 returns the Counterspell to P1's hand, and the Crypt
        // resolves.
        Scenario::new("Reprieve on a counterspell saves the spell it targets")
            .battlefield(P0, &[Permanent::new(names::PLAINS); 2])
            .hand(P0, &[names::MANA_CRYPT, names::REPRIEVE])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, &[Permanent::new(names::ISLAND); 2])
            .hand(P1, &[names::COUNTERSPELL])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::cast(P0, names::MANA_CRYPT),
                Action::pass(P0),
                Action::cast(P1, names::COUNTERSPELL),
                Action::cast(P0, names::REPRIEVE),
                Action::answer(
                    P0,
                    DecisionKind::Target,
                    Pick::Card {
                        card: names::COUNTERSPELL,
                    },
                ),
                Action::Resolve,
                count(P1, Zone::Hand, names::COUNTERSPELL, 1),
                count(P0, Zone::Hand, names::ISLAND, 1),
                Action::Resolve,
                count(P0, Zone::Battlefield, names::MANA_CRYPT, 1),
            ]),
    ];
}
