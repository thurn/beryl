//! The kinds of counter: one list, from which the vocabulary's type and every table of
//! their names is written.

/// The kinds of counter the pool's cards use, each with its name in the rules
/// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)): "Counters with the same
/// name or description are interchangeable." The name is the kind's identity wherever a
/// counter is written down: in a card's text, a save, a transcript.
///
/// A new kind is an entry here and nothing else: [`CounterKind`], its names and every
/// table another crate keeps by kind are written from the list. It hands its entries to the
/// macro named, so a table elsewhere is one more macro over the same entries. A `const fn`
/// cannot enumerate an enum's variants.
#[macro_export]
macro_rules! counter_kinds {
    ($with:ident) => {
        $with! {
            /// [CR 122.1a](https://yawgatog.com/resources/magic-rules/#R1221a): "A +X/+Y
            /// counter on a creature or on a creature card in a zone other than the
            /// battlefield, where X and Y are numbers, adds X to that object's power and Y to
            /// that object's toughness."
            PlusOne = "+1/+1",
            /// [CR 122.1a](https://yawgatog.com/resources/magic-rules/#R1221a): "Similarly,
            /// -X/-Y counters subtract from power and toughness."
            MinusOne = "-1/-1",
            /// [CR 122.1e](https://yawgatog.com/resources/magic-rules/#R1221e): "The number
            /// of loyalty counters on a planeswalker on the battlefield indicates how much
            /// loyalty it has."
            Loyalty = "loyalty",
            Time = "time",
            Story = "story",
            Chorus = "chorus",
        }
    };
}

/// Writes [`CounterKind`] from the list.
macro_rules! vocabulary {
    ($($(#[$doc:meta])* $variant:ident = $name:literal),* $(,)?) => {
        /// A kind of counter
        /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)): "Counters with
        /// the same name or description are interchangeable." The kinds are the entries of
        /// [`counter_kinds`], in its order.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum CounterKind {
            $($(#[$doc])* $variant),*
        }

        impl CounterKind {
            /// Every kind, in the order of the list.
            pub const ALL: &[CounterKind] = &[$(CounterKind::$variant),*];

            /// Its name in the rules, as in "a time counter" (CR 122.1).
            pub const fn name(self) -> &'static str {
                match self {
                    $(CounterKind::$variant => $name),*
                }
            }
        }
    };
}

counter_kinds!(vocabulary);

impl CounterKind {
    /// The kind the rules call `name`.
    pub fn named(name: &str) -> Option<CounterKind> {
        CounterKind::ALL.iter().copied().find(|k| k.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::CounterKind;

    /// CR 122.1: a kind is its name, so no two entries of the list share one, and each
    /// kind is found again by its own.
    #[test]
    fn a_kind_is_found_by_its_name() {
        for &kind in CounterKind::ALL {
            assert_eq!(CounterKind::named(kind.name()), Some(kind));
        }
        assert_eq!(CounterKind::named("no such"), None);
    }
}
