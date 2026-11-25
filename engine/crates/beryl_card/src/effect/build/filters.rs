//! Builders for the filters card text says in one word: "nonland", "untapped", "creature".

use crate::characteristics::Types;
use crate::effect::Players;
use crate::filter::Filter;

/// "Creature": [`Filter::Types`] of [`Types::CREATURE`].
pub const fn creature() -> Filter {
    Filter::Types(Types::CREATURE)
}

/// "Land": [`Filter::Types`] of [`Types::LAND`].
pub const fn land() -> Filter {
    Filter::Types(Types::LAND)
}

/// "Nonland": not a land.
pub const fn nonland() -> Filter {
    Filter::Not(&Filter::Types(Types::LAND))
}

/// "Noncreature": not a creature.
pub const fn noncreature() -> Filter {
    Filter::Not(&Filter::Types(Types::CREATURE))
}

/// "Nontoken": not a token.
pub const fn nontoken() -> Filter {
    Filter::Not(&Filter::Token)
}

/// "Untapped": not tapped.
pub const fn untapped() -> Filter {
    Filter::Not(&Filter::Tapped)
}

/// "You control" on the battlefield and the stack, "your" elsewhere ("your graveyard"):
/// [`Filter::Whose`] of [`Players::You`].
pub const fn yours() -> Filter {
    Filter::Whose(&Players::You)
}
