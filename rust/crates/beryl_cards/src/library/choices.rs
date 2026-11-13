//! Choices made while a spell or ability resolves
//! ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)).

use beryl_engine::{DecisionKind, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::{names, props};

const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

/// A player who may choose a card may choose none, and then nothing is done to any
/// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a
/// spell or ability offers any choices other than choices already made as part of casting
/// the spell, activating the ability, or otherwise putting the spell or ability on the
/// stack, the player announces these while applying the effect."
pub const SCENARIOS: &[Scenario] = &[Scenario::new("A player who may choose a card chooses none")
    .hand(P0, &[SPELLBINDER])
    .permanents(P0, &[Permanent::new(names::PLAINS); 3])
    .library(P0, &[names::PLAINS])
    .hand(P1, &[props::ARTIFACT, names::ISLAND])
    .library(P1, &[names::ISLAND])
    .actions(&[
        Action::cast(P0, SPELLBINDER),
        Action::Resolve,
        Action::Resolve,
        Action::answer(P0, DecisionKind::YesNo, Pick::No),
        Action::assert(Assertion::Count {
            player: P1,
            zone: Zone::Hand,
            card: Some(props::ARTIFACT),
            count: 1,
        }),
        Action::assert(Assertion::Count {
            player: P1,
            zone: Zone::Exile,
            card: None,
            count: 0,
        }),
    ])];
