//! Mana: costs, the kinds of mana, and what a mana ability produces.

use crate::continuous::RuleChange;
use crate::effect::Amount;
use crate::filter::Filter;

/// A mana cost: generic mana plus colored and colorless symbols, hybrid symbols, and any {X}.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ManaCost {
    pub generic: u8,
    /// Symbols by [`ManaColor`] index: W, U, B, R, G, C.
    pub symbols: [u8; 6],
    /// How many {X} symbols it has
    /// ([CR 107.3a](https://yawgatog.com/resources/magic-rules/#R1073a)): their value is
    /// announced as the spell is cast or the ability activated.
    pub x: u8,
    /// Its two-color hybrid symbols, such as {W/B}.
    pub hybrid: Hybrid,
}

impl ManaCost {
    pub const NONE: ManaCost = ManaCost {
        generic: 0,
        symbols: [0; 6],
        x: 0,
        hybrid: Hybrid::NONE,
    };

    /// A cost of generic mana and colored or colorless symbols, by [`ManaColor`] index.
    pub const fn new(generic: u8, symbols: [u8; 6]) -> ManaCost {
        ManaCost {
            generic,
            symbols,
            x: 0,
            hybrid: Hybrid::NONE,
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
        hybrid: Hybrid::NONE,
    };

    /// The amount of mana paying it takes once X is `x`.
    pub fn total(self, x: u8) -> u32 {
        u32::from(self.generic) + self.symbol_count() + u32::from(self.x) * u32::from(x)
    }

    /// How many colored and colorless symbols it has, each hybrid symbol one of them
    /// ([CR 107.4e]): "A hybrid mana symbol is also a colored mana symbol".
    ///
    /// [CR 107.4e]: https://yawgatog.com/resources/magic-rules/#R1074e
    #[inline]
    pub fn symbol_count(self) -> u32 {
        let single = self.symbols.iter().map(|&n| u32::from(n)).sum::<u32>();
        if self.hybrid.is_empty() {
            single
        } else {
            single + self.hybrid.total_cold()
        }
    }

    /// Both costs together, X included: what
    /// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)) totals as "the mana
    /// cost or alternative cost ..., plus all additional costs and cost increases".
    #[must_use]
    pub const fn plus(self, other: ManaCost) -> ManaCost {
        let mut sum = self;
        sum.generic = sum.generic.saturating_add(other.generic);
        let mut index = 0;
        while index < sum.symbols.len() {
            sum.symbols[index] = sum.symbols[index].saturating_add(other.symbols[index]);
            index += 1;
        }
        sum.x = sum.x.saturating_add(other.x);
        sum.hybrid = sum.hybrid.plus(other.hybrid);
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
    ///
    /// A hybrid symbol counts one ([CR 202.3f]): "When calculating the mana value of an object
    /// with a hybrid mana symbol in its mana cost, use the largest component of each hybrid
    /// symbol." Both halves of a two-color hybrid symbol are one mana.
    ///
    /// [CR 202.3f]: https://yawgatog.com/resources/magic-rules/#R2023f
    pub fn mana_value(self) -> u8 {
        let value = self.generic + self.symbols.iter().sum::<u8>();
        if self.hybrid.is_empty() {
            value
        } else {
            // At most seven of each of ten pairs.
            value + u8::try_from(self.hybrid.total_cold()).unwrap_or(u8::MAX)
        }
    }

    /// The colors of its colored symbols, a bit per [`ManaColor`] index, colorless {C}
    /// excluded: both halves of each hybrid symbol ([CR 202.2d]): "An object with one or more
    /// hybrid mana symbols ... in its mana cost is all of the colors of those mana symbols".
    ///
    /// [CR 202.2d]: https://yawgatog.com/resources/magic-rules/#R2022d
    pub const fn color_bits(self) -> u8 {
        let mut bits = self.hybrid.colors();
        let mut c = 0;
        while c < 5 {
            if self.symbols[c] > 0 {
                bits |= 1 << c;
            }
            c += 1;
        }
        bits
    }

    /// This cost with every colored and hybrid symbol moved into its generic part, leaving
    /// `{C}` symbols untouched: what a player pays when they may spend mana as though it were
    /// mana of any color (see [CR 107.4c], see [CR 107.4e]).
    ///
    /// [CR 107.4c]: https://yawgatog.com/resources/magic-rules/#R1074c
    /// [CR 107.4e]: https://yawgatog.com/resources/magic-rules/#R1074e
    #[must_use]
    pub fn as_any_color(self) -> ManaCost {
        let colored = self.symbols[..5]
            .iter()
            .copied()
            .fold(0u8, u8::saturating_add);
        let hybrid = if self.hybrid.is_empty() {
            0
        } else {
            u8::try_from(self.hybrid.total_cold()).unwrap_or(u8::MAX)
        };
        let mut symbols = [0u8; 6];
        symbols[5] = self.symbols[5];
        ManaCost {
            generic: self.generic.saturating_add(colored).saturating_add(hybrid),
            symbols,
            x: self.x,
            hybrid: Hybrid::NONE,
        }
    }
}

/// Every field, but `hybrid` only where the cost has a hybrid symbol: a cost of none hashes as
/// it did before costs held them, so the states the benchmark corpora pin by their hashes
/// still hash the same, and the many hashes of decisions that carry a cost hash no more.
impl std::hash::Hash for ManaCost {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.generic.hash(h);
        self.symbols.hash(h);
        self.x.hash(h);
        if !self.hybrid.is_empty() {
            self.hybrid.hash(h);
        }
    }
}

/// Every field, but `hybrid` only where the cost has a hybrid symbol: a cost of none prints as
/// it did before costs held them, which names of mutants and test expectations quote.
impl std::fmt::Debug for ManaCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("ManaCost");
        d.field("generic", &self.generic)
            .field("symbols", &self.symbols)
            .field("x", &self.x);
        if !self.hybrid.is_empty() {
            d.field("hybrid", &self.hybrid);
        }
        d.finish()
    }
}

/// The two-color hybrid symbols of a cost, counted by pair ([CR 107.4e]): "Each one represents
/// a cost that can be paid in one of two ways, as represented by the two halves of the symbol.
/// A hybrid symbol such as {W/U} can be paid with either white or blue mana".
///
/// Three bits count each of the ten pairs of [`Hybrid::PAIRS`], up to seven symbols of a pair:
/// Figure of Destiny's six {R/W} are the most a printed cost has. The counts take four bytes,
/// which keeps [`ManaCost`] small enough for a decision's prompt (`Prompt`) and an ability
/// to keep their sizes. A cost of no hybrid symbol is the zero word, so a payment checks
/// [`Hybrid::is_empty`] once and goes on as before.
///
/// Monocolored hybrid symbols ({2/W}, {C/W}) and Phyrexian symbols are not held yet: a card
/// with one is not complete ([`crate::mana_notation::read`] refuses it) until a card that
/// needs it adds it here.
///
/// [CR 107.4e]: https://yawgatog.com/resources/magic-rules/#R1074e
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Hybrid([u8; 4]);

/// Each pair with its count, such as `Hybrid { W/B: 2 }`.
impl std::fmt::Debug for Hybrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const LETTERS: [char; 5] = ['W', 'U', 'B', 'R', 'G'];
        f.write_str("Hybrid {")?;
        for (i, (pair, n)) in self.pairs().enumerate() {
            let mut halves = (0..5).filter(|&c| pair & (1 << c) != 0).map(|c| LETTERS[c]);
            let (a, b) = (halves.next().unwrap_or('?'), halves.next().unwrap_or('?'));
            let sep = if i == 0 { " " } else { ", " };
            write!(f, "{sep}{a}/{b}: {n}")?;
        }
        f.write_str(" }")
    }
}

impl Hybrid {
    pub const NONE: Hybrid = Hybrid([0; 4]);

    /// The ten pairs, each as a bit per [`ManaColor`] index, in the order Oracle text writes
    /// them: the allied pairs {W/U}, {U/B}, {B/R}, {R/G}, {G/W}, then the enemy pairs {W/B},
    /// {U/R}, {B/G}, {R/W}, {G/U}.
    pub const PAIRS: [u8; 10] = crate::mana_notation::HYBRID_PAIRS;

    /// The most symbols of one pair it counts.
    pub const MOST: u8 = 7;

    const fn word(self) -> u32 {
        u32::from_le_bytes(self.0)
    }

    const fn of_word(word: u32) -> Hybrid {
        Hybrid(word.to_le_bytes())
    }

    /// The index in [`Hybrid::PAIRS`] of the pair of `colors`, two bits of
    /// [`ManaColor`] indices.
    pub const fn pair(colors: u8) -> Option<usize> {
        let mut i = 0;
        while i < Hybrid::PAIRS.len() {
            if Hybrid::PAIRS[i] == colors {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Whether it has no symbol.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.word() == 0
    }

    /// How many symbols of the pair at `pair` in [`Hybrid::PAIRS`] it has.
    pub const fn count(self, pair: usize) -> u8 {
        ((self.word() >> (3 * pair)) & 0b111) as u8
    }

    /// The same with `n` symbols of the pair at `pair`, at most [`Hybrid::MOST`].
    #[must_use]
    pub const fn with_count(self, pair: usize, n: u8) -> Hybrid {
        let n = if n > Hybrid::MOST { Hybrid::MOST } else { n };
        let shift = 3 * pair;
        Hybrid::of_word((self.word() & !(0b111 << shift)) | ((n as u32) << shift))
    }

    /// How many symbols it has in all: the counts' low, middle and high bits, each set of
    /// bits counted at once.
    pub const fn total(self) -> u32 {
        const LOW: u32 = 0b001_001_001_001_001_001_001_001_001_001;
        let word = self.word();
        (word & LOW).count_ones()
            + 2 * (word & (LOW << 1)).count_ones()
            + 4 * (word & (LOW << 2)).count_ones()
    }

    /// The colors of its symbols, a bit per [`ManaColor`] index.
    pub const fn colors(self) -> u8 {
        let mut bits = 0;
        let mut i = 0;
        while i < Hybrid::PAIRS.len() {
            if self.count(i) > 0 {
                bits |= Hybrid::PAIRS[i];
            }
            i += 1;
        }
        bits
    }

    /// [`Hybrid::total`], out of line: the costs a game with no hybrid symbol pays read only
    /// [`Hybrid::is_empty`].
    #[cold]
    #[inline(never)]
    fn total_cold(self) -> u32 {
        self.total()
    }

    /// Both costs' symbols, each pair's count at most [`Hybrid::MOST`].
    #[must_use]
    pub const fn plus(self, other: Hybrid) -> Hybrid {
        if other.is_empty() {
            return self;
        }
        let mut sum = self;
        let mut i = 0;
        while i < Hybrid::PAIRS.len() {
            sum = sum.with_count(i, self.count(i) + other.count(i));
            i += 1;
        }
        sum
    }

    /// Each pair it has symbols of, as a bit per [`ManaColor`] index, with how many.
    pub fn pairs(self) -> impl Iterator<Item = (u8, u8)> {
        (0..Hybrid::PAIRS.len())
            .map(move |i| (Hybrid::PAIRS[i], self.count(i)))
            .filter(|&(_, n)| n > 0)
    }

    /// The same with one symbol of `colors`' pair fewer.
    ///
    /// # Panics
    ///
    /// If it has no symbol of that pair.
    #[must_use]
    pub fn without_one(self, colors: u8) -> Hybrid {
        let pair = Hybrid::pair(colors).expect("a hybrid pair");
        let n = self.count(pair);
        assert!(n > 0, "a hybrid symbol of the pair");
        self.with_count(pair, n - 1)
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
    pub const fn index(self) -> usize {
        self as usize
    }
}

// `ManaColor::index` is the discriminant, so `ManaColor::ALL` lists the kinds in declaration
// order.
const _: () = {
    let mut i = 0;
    while i < ManaColor::ALL.len() {
        assert!(ManaColor::ALL[i].index() == i);
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
            colors: 1 << color.index(),
            amount: 1,
        }
    }

    /// `amount` mana of one kind, as "Add {U}{U}." says.
    pub const fn of(color: ManaColor, amount: u8) -> ManaSet {
        ManaSet {
            colors: 1 << color.index(),
            amount,
        }
    }

    /// `amount` mana of any one color, as "Add three mana of any one color." says: the
    /// five colors, not colorless.
    pub const fn any_color(amount: u8) -> ManaSet {
        ManaSet {
            colors: 0b1_1111,
            amount,
        }
    }

    pub fn includes(self, color: ManaColor) -> bool {
        self.colors & (1 << color.index()) != 0
    }
}

/// One part of what a mana ability makes: `amount` units of mana of the kinds in `kinds`. A
/// mana ability's product is a list of terms ([`crate::Ability::product`]), so "Add {W}{U}."
/// is a term of {W} and a term of {U}, and "Add {B} for each Swamp you control." is one term
/// whose amount counts the Swamps.
///
/// Where `kinds` has more than one kind, `uniform` says how the kinds are chosen
/// ([CR 106.1b]): "There are six types of mana: white, blue, black, red, green, and
/// colorless." With
/// `uniform` false each unit may be any kind of `kinds`, chosen apart, as "Add {W}{W}, {W}{U},
/// or {U}{U}." makes two units each white or blue. With `uniform` true all its units are one
/// kind chosen from `kinds`, as "Add three mana of any one color." says. Of a term of one
/// kind, `uniform` says nothing, and the builders set it false.
///
/// [CR 106.1b]: https://yawgatog.com/resources/magic-rules/#R1061b
/// The kinds of mana a [`Term`] can produce: either a fixed set of mana types or any type
/// that a permanent matching a filter could produce ([CR 106.7]).
///
/// [CR 106.7]: https://yawgatog.com/resources/magic-rules/#R1067
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kinds {
    /// A fixed set of mana types, a bit per [`ManaColor`].
    Fixed(u8),
    /// Any type of mana that a permanent matching the filter could produce ([CR 106.7]):
    /// "Some abilities produce mana based on the type of mana another permanent or permanents
    /// 'could produce.' The type of mana a permanent could produce at any time includes any
    /// type of mana that an ability of that permanent would produce if the ability were to
    /// resolve at that time, taking into account any applicable replacement effects in any
    /// possible order."
    CouldProduce(&'static Filter),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Term {
    /// The kinds of mana this term can produce.
    pub kinds: Kinds,
    pub amount: Amount,
    pub uniform: bool,
}

impl Term {
    /// `amount` mana of one kind, as "{U}{U}" in "Add {U}{U}." says.
    pub const fn of(color: ManaColor, amount: u8) -> Term {
        Term {
            kinds: Kinds::Fixed(1 << color.index()),
            amount: Amount::Fixed(amount),
            uniform: false,
        }
    }

    /// One mana of one kind, as "{W}" in "Add {W}{U}." says.
    pub const fn one(color: ManaColor) -> Term {
        Term::of(color, 1)
    }

    /// `amount` mana of one kind for each of what `amount` counts, as "Add {B} for each Swamp
    /// you control." says.
    pub const fn counted(color: ManaColor, amount: Amount) -> Term {
        Term {
            kinds: Kinds::Fixed(1 << color.index()),
            amount,
            uniform: false,
        }
    }

    /// `amount` units, each of any kind of `kinds` chosen apart, as "Add {W}{W}, {W}{U}, or
    /// {U}{U}." says of two units of {W} and {U}.
    pub const fn each_of(kinds: u8, amount: u8) -> Term {
        Term {
            kinds: Kinds::Fixed(kinds),
            amount: Amount::Fixed(amount),
            uniform: false,
        }
    }

    /// One mana of any type that a permanent matching `filter` could produce ([CR 106.7]).
    ///
    /// [CR 106.7]: https://yawgatog.com/resources/magic-rules/#R1067
    pub const fn could_produce(filter: &'static Filter) -> Term {
        Term {
            kinds: Kinds::CouldProduce(filter),
            amount: Amount::Fixed(1),
            uniform: false,
        }
    }

    /// The term of a [`ManaSet`]: `amount` mana of one kind of its colors.
    pub const fn of_set(set: ManaSet) -> Term {
        Term {
            kinds: Kinds::Fixed(set.colors),
            amount: Amount::Fixed(set.amount),
            uniform: set.colors.count_ones() > 1,
        }
    }

    /// The [`ManaSet`] that says the same as this term, if one does: a fixed amount, of one
    /// kind, of units of one kind together ([`Term::uniform`]), or a single unit.
    pub const fn as_set(self) -> Option<ManaSet> {
        let (Kinds::Fixed(colors), Amount::Fixed(amount)) = (self.kinds, self.amount) else {
            return None;
        };
        if colors.is_power_of_two() || self.uniform || amount == 1 {
            Some(ManaSet { colors, amount })
        } else {
            None
        }
    }
}

/// What the mana a mana ability makes may be spent on, and what it does to the spell it pays
/// for ([CR 106.6]): "Some spells or abilities that produce mana restrict how that mana can be
/// spent, have an additional effect that affects the spell or ability that mana is spent on,
/// ... This doesn't affect the mana's type." "Spend this mana only to cast a creature spell
/// of the chosen type, and that spell can't be countered" is the creature spells, of the
/// chosen type, with "can't be countered" as the rider.
///
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
#[derive(Clone, Copy, Debug)]
pub struct ManaRestriction {
    /// The spells it may be spent to cast, read of each spell's own characteristics.
    pub cast: Filter,
    /// Whether the spell must also have the creature type the mana's source chose as it
    /// entered ([`crate::Replacement::choose_as_enters`]). The type is remembered with the
    /// mana as it is made: the source may be gone by the time the mana is spent.
    pub of_chosen_type: bool,
    /// What it does to the spell it is spent on: a rule change on that spell, for as long as
    /// the spell is on the stack.
    pub rider: Option<&'static RuleChange>,
}

/// A mana cost in Scryfall's notation, parsed at compile time: `mana("{1}{U}")`. For costs
/// that are not a card's printed mana cost, such as a kicker cost or what "unless they pay"
/// asks for.
///
/// It reads the symbols a [`ManaCost`] can hold, as [`crate::mana_notation::read`] says.
///
/// # Panics
///
/// On anything else, monocolored hybrid, Phyrexian and snow symbols included: in a `const` or
/// `static`, at compile time.
pub const fn mana(notation: &str) -> ManaCost {
    match crate::mana_notation::read(notation) {
        // The notation's symbols are in the order W, U, B, R, G, C, as `ManaColor` indexes
        // them.
        Ok(read) => ManaCost {
            generic: read.generic,
            symbols: read.symbols,
            x: read.x,
            hybrid: Hybrid(read.hybrid.to_le_bytes()),
        },
        Err(why) => panic!("{}", why),
    }
}

/// The intrinsic mana ability of each basic land type, in the order of
/// [`crate::BasicLandTypes`] and of the mana each makes, W, U, B, R, G ([CR 305.6]): "An
/// object with the land card type and a basic land type has the intrinsic ability '{T}: Add
/// [mana symbol],' even if the text box doesn't actually contain that text or the object has
/// no text box. For Plains, [mana symbol] is {W}; for Islands, {U}; for Swamps, {B}; for
/// Mountains, {R}; and for Forests, {G}." The engine activates one of these for a land that
/// has the ability without its face writing it out ([`crate::Keywords::PLAINS_MANA`]).
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
pub static INTRINSIC_MANA: [crate::Ability; 5] = [
    crate::Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
    crate::Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
    crate::Ability::tap_for_mana(ManaSet::one(ManaColor::Black)),
    crate::Ability::tap_for_mana(ManaSet::one(ManaColor::Red)),
    crate::Ability::tap_for_mana(ManaSet::one(ManaColor::Green)),
];

#[cfg(test)]
mod tests {
    use super::{Hybrid, ManaCost, mana};

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
                hybrid: Hybrid::NONE,
            }
        );
    }

    #[test]
    fn mana_reads_hybrid_symbols() {
        let lurrus = mana("{1}{W/B}{W/B}");
        let wb = Hybrid::pair(0b00101).expect("W/B");
        assert_eq!(lurrus.hybrid.count(wb), 2);
        assert_eq!(lurrus.hybrid.total(), 2);
        assert_eq!(lurrus.mana_value(), 3);
        assert_eq!(lurrus.total(0), 3);
        assert_eq!(lurrus.color_bits(), 0b00101);
        assert_eq!(mana("{B/W}"), mana("{W/B}"));
        let leyline = mana("{G/W}{G/U}{B/G}{R/G}");
        assert_eq!(leyline.hybrid.pairs().count(), 4);
        assert_eq!(leyline.color_bits(), 0b11111);
        assert_eq!(lurrus.plus(lurrus).hybrid.count(wb), 4);
        assert_eq!(
            format!("{lurrus:?}"),
            "ManaCost { generic: 1, symbols: [0, 0, 0, 0, 0, 0], x: 0, hybrid: Hybrid { W/B: 2 } }"
        );
        assert_eq!(
            format!("{:?}", mana("{U}")),
            "ManaCost { generic: 0, symbols: [0, 1, 0, 0, 0, 0], x: 0 }"
        );
    }

    #[test]
    #[should_panic(expected = "a mana symbol a cost cannot hold")]
    fn mana_refuses_monocolored_hybrid() {
        mana("{2/W}");
    }

    #[test]
    #[should_panic(expected = "a mana symbol a cost cannot hold")]
    fn mana_refuses_phyrexian() {
        mana("{W/P}");
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
