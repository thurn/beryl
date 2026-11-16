use crate::{Filter, Players, Supertypes, Types};

/// "nonland permanent"
pub const NONLAND: Filter = Filter::Not(&Filter::Types(Types::LAND));

/// "noncreature spell"
pub const NONCREATURE: Filter = Filter::Not(&Filter::Types(Types::CREATURE));

/// "nonbasic lands"
pub const NONBASIC_LANDS: Filter = Filter::All(&[
    Filter::Types(Types::LAND),
    Filter::Not(&Filter::Supertypes(Supertypes::BASIC)),
]);

/// "untapped"
pub const UNTAPPED: Filter = Filter::Not(&Filter::Tapped);

/// "permanent you control"
pub const YOU_CONTROL: Filter = Filter::Whose(&Players::You);

/// "permanent an opponent controls"
pub const OPPONENT_CONTROLS: Filter = Filter::Whose(&Players::EachOpponent);

/// "creatures you control"
pub const CREATURES_YOU_CONTROL: Filter =
    Filter::All(&[Filter::Types(Types::CREATURE), YOU_CONTROL]);

/// "lands you control"
pub const LANDS_YOU_CONTROL: Filter = Filter::All(&[Filter::Types(Types::LAND), YOU_CONTROL]);

/// "creature you don't control"
pub const CREATURE_YOU_DONT_CONTROL: Filter =
    Filter::All(&[Filter::Types(Types::CREATURE), Filter::Not(&YOU_CONTROL)]);

/// "instant or sorcery spell"
pub const INSTANT_OR_SORCERY: Filter = Filter::Types(Types::INSTANT.union(Types::SORCERY));
