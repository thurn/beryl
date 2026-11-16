use super::{CREATURES_YOU_CONTROL, INSTANT_OR_SORCERY, NONLAND, OPPONENT_CONTROLS};
use crate::{Filter, Players, TargetKind, TargetSpec, Types};

/// "target permanent"
pub const TARGET_PERMANENT: TargetSpec = TargetSpec::permanent(Filter::Any);

/// "another target permanent"
pub const ANOTHER_TARGET_PERMANENT: TargetSpec = TARGET_PERMANENT.other();

/// "target nonland permanent"
pub const NONLAND_PERMANENT: TargetSpec = TargetSpec::permanent(NONLAND);

/// "target land"
pub const TARGET_LAND: TargetSpec = TargetSpec::permanent(Filter::Types(Types::LAND));

/// "up to one target creature"
pub const UP_TO_ONE_TARGET_CREATURE: TargetSpec = TargetSpec::creature().optional();

/// "target tapped creature"
pub const TARGET_TAPPED_CREATURE: TargetSpec = TargetSpec::permanent(Filter::All(&[
    Filter::Types(Types::CREATURE),
    Filter::Tapped,
]));

/// "target creature you control"
pub const TARGET_CREATURE_YOU_CONTROL: TargetSpec = TargetSpec::permanent(CREATURES_YOU_CONTROL);

/// "target instant or sorcery card in your graveyard"
pub const TARGET_YOUR_INSTANT_OR_SORCERY_CARD: TargetSpec =
    TargetSpec::graveyard_card(Filter::All(&[
        INSTANT_OR_SORCERY,
        Filter::Whose(&Players::You),
    ]));

/// "target spell or nonland permanent an opponent controls"
pub const TARGET_OPPONENT_SPELL_OR_NONLAND_PERMANENT: TargetSpec = TargetSpec::one(
    TargetKind::SpellOrPermanent,
    Filter::All(&[OPPONENT_CONTROLS, NONLAND]),
);
