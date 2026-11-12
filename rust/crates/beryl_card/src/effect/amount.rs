//! Amounts: how many, or how much.

use crate::counter::CounterKind;
use crate::effect::Objects;

/// A number an instruction uses, read as the instruction is reached
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect requires
/// information from the game (such as the number of creatures on the battlefield), the answer
/// is determined only once, when the effect is applied." It is never less than zero
/// ([CR 107.1b](https://yawgatog.com/resources/magic-rules/#R1071b)): "If a calculation that
/// would determine the result of an effect yields a negative number, zero is used instead".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Amount {
    /// A number printed on the card: "two cards", "3 life".
    Fixed(u8),
    /// X ([CR 107.3](https://yawgatog.com/resources/magic-rules/#R1073)): "Many objects use
    /// the letter X as a placeholder for a number that needs to be determined." The value
    /// announced for the spell or ability
    /// ([CR 107.3a](https://yawgatog.com/resources/magic-rules/#R1073a)).
    X,
    /// "That many", "that much": how many or how much the instruction before did (the
    /// [module](crate::effect) says which instruction that is): the cards it drew or
    /// discarded, the life gained or lost, the counters put or removed, the objects it moved.
    ThatMany,
    /// "The number of [objects]", "for each [object]".
    Count(&'static Objects),
    /// "[n] for each ...": `n` times an amount, as "you lose 2 life for each card in your
    /// hand".
    Times(u8, &'static Amount),
    /// "Its power", or of several objects, their total power. A power below zero counts as it
    /// is until the result is used (CR 107.1b).
    Power(Objects),
    /// "Its toughness", or their total toughness.
    Toughness(Objects),
    /// "Its mana value"
    /// ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)): "The mana value of an
    /// object is a number equal to the total amount of mana in its mana cost, regardless of
    /// color." Or their total mana value.
    ManaValue(Objects),
    /// "The number of [kind] counters on [objects]".
    Counters(Objects, CounterKind),
}

impl Amount {
    /// Whether this means something only a resolving spell or ability has
    /// ([`Objects::needs_a_resolution`]): its X, or how much an instruction before did.
    pub const fn needs_a_resolution(&self) -> bool {
        match self {
            Amount::Fixed(_) => false,
            Amount::X | Amount::ThatMany => true,
            Amount::Count(objects) => objects.needs_a_resolution(),
            Amount::Times(_, amount) => amount.needs_a_resolution(),
            Amount::Power(objects)
            | Amount::Toughness(objects)
            | Amount::ManaValue(objects)
            | Amount::Counters(objects, _) => objects.needs_a_resolution(),
        }
    }
}

impl Amount {
    /// Whether it reads what a cost took ([`Objects::Paid`]).
    pub const fn names_paid(&self) -> bool {
        match self {
            Amount::Fixed(_) | Amount::X | Amount::ThatMany => false,
            Amount::Count(objects) => objects.names_paid(),
            Amount::Times(_, amount) => amount.names_paid(),
            Amount::Power(objects)
            | Amount::Toughness(objects)
            | Amount::ManaValue(objects)
            | Amount::Counters(objects, _) => objects.names_paid(),
        }
    }
}
