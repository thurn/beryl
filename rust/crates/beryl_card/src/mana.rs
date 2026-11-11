//! Mana: costs, the kinds of mana, and what a mana ability produces.

/// A mana cost: generic mana plus colored and colorless symbols, and any {X}.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaCost {
    pub generic: u8,
    /// Symbols by [`ManaColor`] index: W, U, B, R, G, C.
    pub symbols: [u8; 6],
    /// How many {X} symbols it has
    /// ([CR 107.3a](https://yawgatog.com/resources/magic-rules/#R1073a)): their value is
    /// announced as the spell is cast or the ability activated.
    pub x: u8,
}

impl ManaCost {
    pub const NONE: ManaCost = ManaCost {
        generic: 0,
        symbols: [0; 6],
        x: 0,
    };

    /// A cost of generic mana and colored or colorless symbols, by [`ManaColor`] index.
    pub const fn new(generic: u8, symbols: [u8; 6]) -> ManaCost {
        ManaCost {
            generic,
            symbols,
            x: 0,
        }
    }

    /// A cost of generic mana only, such as {3}.
    pub const fn generic(n: u8) -> ManaCost {
        ManaCost::new(n, [0; 6])
    }

    /// A cost of {X} alone, such as Thieving Skydiver's kicker.
    pub const X: ManaCost = ManaCost {
        generic: 0,
        symbols: [0; 6],
        x: 1,
    };

    /// The amount of mana paying it takes once X is `x`.
    pub fn total(self, x: u8) -> u32 {
        u32::from(self.generic)
            + self.symbols.iter().map(|&n| u32::from(n)).sum::<u32>()
            + u32::from(self.x) * u32::from(x)
    }

    /// Both costs together, X included: what
    /// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)) totals as "the mana
    /// cost or alternative cost ..., plus all additional costs and cost increases".
    #[must_use]
    pub fn plus(self, other: ManaCost) -> ManaCost {
        let mut sum = self;
        sum.generic = sum.generic.saturating_add(other.generic);
        for (a, b) in sum.symbols.iter_mut().zip(other.symbols) {
            *a = a.saturating_add(b);
        }
        sum.x = sum.x.saturating_add(other.x);
        sum
    }

    /// This cost with X replaced by its announced value, as generic mana
    /// ([CR 107.4b](https://yawgatog.com/resources/magic-rules/#R1074b)): "Numerical symbols
    /// (such as {1}) and variable symbols (such as {X}) represent generic mana in costs."
    #[must_use]
    pub fn with_x(self, x: u8) -> ManaCost {
        ManaCost {
            generic: self.generic.saturating_add(self.x.saturating_mul(x)),
            x: 0,
            ..self
        }
    }

    pub fn is_zero(self) -> bool {
        self.total(0) == 0 && self.x == 0
    }

    /// The mana value ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)): "The
    /// mana value of an object is a number equal to the total amount of mana in its mana cost,
    /// regardless of color."
    pub fn mana_value(self) -> u8 {
        self.generic + self.symbols.iter().sum::<u8>()
    }
}

/// A kind of mana: the five colors and colorless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ManaColor {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl ManaColor {
    /// Every kind, in the order W, U, B, R, G, C that indexes [`ManaCost::symbols`].
    pub const ALL: [ManaColor; 6] = [
        ManaColor::White,
        ManaColor::Blue,
        ManaColor::Black,
        ManaColor::Red,
        ManaColor::Green,
        ManaColor::Colorless,
    ];

    /// Its index in [`ManaColor::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

// `ManaColor::index` is the discriminant, so `ManaColor::ALL` lists the kinds in declaration
// order.
const _: () = {
    let mut i = 0;
    while i < ManaColor::ALL.len() {
        assert!(ManaColor::ALL[i] as usize == i);
        i += 1;
    }
};

/// What a mana ability can produce: a set of [`ManaColor`]s and how much.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaSet {
    /// A bit per [`ManaColor`].
    pub colors: u8,
    pub amount: u8,
}

impl ManaSet {
    pub const NONE: ManaSet = ManaSet {
        colors: 0,
        amount: 0,
    };

    /// One mana of one kind.
    pub const fn one(color: ManaColor) -> ManaSet {
        ManaSet {
            colors: 1 << color as u8,
            amount: 1,
        }
    }

    pub fn includes(self, color: ManaColor) -> bool {
        self.colors & (1 << color as u8) != 0
    }
}

/// A mana cost in Scryfall's notation, parsed at compile time: `mana("{1}{U}")`. For costs
/// that are not a card's printed mana cost, such as a kicker cost or what "unless they pay"
/// asks for.
///
/// It reads the symbols a [`ManaCost`] can hold
/// ([CR 107.4](https://yawgatog.com/resources/magic-rules/#R1074)): the colored and
/// colorless symbols `{W}`, `{U}`, `{B}`, `{R}`, `{G}` and `{C}`, numerical symbols such as
/// `{3}`, and the variable symbol `{X}`.
///
/// # Panics
///
/// On anything else, hybrid, Phyrexian and snow symbols included: in a `const` or `static`,
/// at compile time.
pub const fn mana(notation: &str) -> ManaCost {
    let bytes = notation.as_bytes();
    let mut cost = ManaCost::NONE;
    let mut i = 0;
    while i < bytes.len() {
        assert!(bytes[i] == b'{', "a mana symbol opens with a brace");
        i += 1;
        let start = i;
        let mut number: u32 = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            number = number * 10 + (bytes[i] - b'0') as u32;
            assert!(number <= 255, "more generic mana than a cost holds");
            i += 1;
        }
        if i > start {
            cost.generic += number as u8;
        } else {
            assert!(i < bytes.len(), "a mana symbol was left open");
            match bytes[i] {
                b'W' => cost.symbols[ManaColor::White as usize] += 1,
                b'U' => cost.symbols[ManaColor::Blue as usize] += 1,
                b'B' => cost.symbols[ManaColor::Black as usize] += 1,
                b'R' => cost.symbols[ManaColor::Red as usize] += 1,
                b'G' => cost.symbols[ManaColor::Green as usize] += 1,
                b'C' => cost.symbols[ManaColor::Colorless as usize] += 1,
                b'X' => cost.x += 1,
                _ => panic!("a mana symbol a cost cannot hold"),
            }
            i += 1;
        }
        assert!(
            i >= bytes.len() || bytes[i] != b'/',
            "a mana symbol a cost cannot hold"
        );
        assert!(
            i < bytes.len() && bytes[i] == b'}',
            "a mana symbol closes with a brace"
        );
        i += 1;
    }
    cost
}

#[cfg(test)]
mod tests {
    use super::{ManaCost, mana};

    #[test]
    fn mana_reads_scryfall_notation() {
        const DRAIN: ManaCost = mana("{1}{U}{U}");
        assert_eq!(DRAIN, ManaCost::new(1, [0, 2, 0, 0, 0, 0]));
        assert_eq!(mana(""), ManaCost::NONE);
        assert_eq!(mana("{0}"), ManaCost::NONE);
        assert_eq!(mana("{12}"), ManaCost::generic(12));
        assert_eq!(mana("{X}"), ManaCost::X);
        assert_eq!(
            mana("{X}{X}{2}{W}{B}{R}{G}{C}"),
            ManaCost {
                generic: 2,
                symbols: [1, 0, 1, 1, 1, 1],
                x: 2,
            }
        );
    }

    #[test]
    #[should_panic(expected = "a mana symbol a cost cannot hold")]
    fn mana_refuses_symbols_a_cost_cannot_hold() {
        mana("{W/U}");
    }

    #[test]
    #[should_panic(expected = "a mana symbol a cost cannot hold")]
    fn mana_refuses_snow() {
        mana("{S}");
    }

    #[test]
    #[should_panic(expected = "a mana symbol closes with a brace")]
    fn mana_refuses_an_open_symbol() {
        mana("{1}{U");
    }
}
