//! A value a permanent chose as it entered: a color, a creature type or a card name
//! ([`beryl_card::Replacement::choose_as_enters`]).
//!
//! [CR 614.12a](https://yawgatog.com/resources/magic-rules/#R61412a): "If a replacement effect
//! that modifies how a permanent enters the battlefield requires a choice, that choice is made
//! before the permanent enters the battlefield." The permanent remembers it
//! ([`crate::Memo::Chosen`]) until it leaves the battlefield, and its abilities read it
//! ([`State::chosen`]).
//!
//! Each kind of value is a catalog, numbered so that a prompt's candidate is a number
//! ([`ChosenValue::candidates`]): a color by its place in [`Colors::EACH`], a creature type by
//! the game's number for it, a card name by the game index of a card with it. A creature type
//! or a name no card of the game has would have the same consequences as every other such
//! one, since nothing in the game reads it, so one candidate, [`ANOTHER`], stands for them all.

use beryl_card::{ChosenKind, Colors};

use crate::ids::CardId;
use crate::layout::{Layout, Small};
use crate::relayout::card;
use crate::table::GameTable;

/// The number of the candidate that stands for every creature type or card name the game has
/// no card of. No subtype number is this high, and no game index a name is offered by
/// ([`ChosenValue::candidates`]).
pub const ANOTHER: u8 = u8::MAX;

/// A value a permanent chose as it entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChosenValue<L: Layout = Small> {
    /// One color, one bit of the set.
    Color(Colors),
    /// A creature type, by the game's number for it, or `None` for a type no card of the game
    /// names.
    CreatureType(Option<u8>),
    /// A card name, by the game index of the first card with it, or `None` for a name no card
    /// of the game has.
    Name(Option<L::Card>),
}

/// Filler for a fixed-capacity list's unused places, never a value of its own.
impl<L: Layout> Default for ChosenValue<L> {
    fn default() -> Self {
        ChosenValue::Name(None)
    }
}

impl<L: Layout> ChosenValue<L> {
    /// The kind of value it is.
    #[inline]
    pub fn kind(self) -> ChosenKind {
        match self {
            ChosenValue::Color(_) => ChosenKind::Color,
            ChosenValue::CreatureType(_) => ChosenKind::CreatureType,
            ChosenValue::Name(_) => ChosenKind::CardName,
        }
    }

    /// The numbers of the values of `kind` a player may choose in the game of `t`, in the
    /// order a prompt lists them: the five colors; the creature types the game numbers, then
    /// [`ANOTHER`]; the names of the game's cards in game order, then [`ANOTHER`].
    pub fn candidates(t: &GameTable, kind: ChosenKind) -> impl Iterator<Item = u8> + '_ {
        let numbers: Box<dyn Iterator<Item = u8> + '_> = match kind {
            ChosenKind::Color => Box::new(0..5),
            ChosenKind::CreatureType => Box::new(t.subtypes().creature_types().map(|(n, _)| n)),
            // A layout whose card index is wider than a byte numbers a name past ANOTHER; no
            // pair of decklists gives a game that many cards.
            ChosenKind::CardName => Box::new(
                (t.names().iter()).filter_map(|&i| u8::try_from(i).ok().filter(|&n| n < ANOTHER)),
            ),
        };
        let another = (kind != ChosenKind::Color).then_some(ANOTHER);
        numbers.chain(another)
    }

    /// The value of `kind` that candidate number `n` stands for, if any
    /// ([`ChosenValue::candidates`]).
    pub fn of_number(kind: ChosenKind, n: u8) -> Option<ChosenValue<L>> {
        let named = (n != ANOTHER).then_some(n);
        Some(match kind {
            ChosenKind::Color => ChosenValue::Color(*Colors::EACH.get(usize::from(n))?),
            ChosenKind::CreatureType => ChosenValue::CreatureType(named),
            ChosenKind::CardName => match named {
                Some(n) => ChosenValue::Name(Some(L::Card::try_from_index(usize::from(n))?)),
                None => ChosenValue::Name(None),
            },
        })
    }

    /// The number of the candidate it is ([`ChosenValue::candidates`]).
    pub fn number(self) -> u8 {
        let byte = |i: usize| u8::try_from(i).expect("a chosen value's number fits a byte");
        match self {
            ChosenValue::Color(color) => byte(
                (Colors::EACH.iter())
                    .position(|&c| c == color)
                    .expect("one color"),
            ),
            ChosenValue::CreatureType(n) => n.unwrap_or(ANOTHER),
            ChosenValue::Name(card) => card.map_or(ANOTHER, |c| byte(c.index())),
        }
    }

    /// What it is called: "Red", "Goblin", "Counterspell", or for a value no card of the game
    /// has, "Another creature type" or "Another card".
    pub fn name(self, t: &GameTable) -> String {
        match self {
            ChosenValue::Color(color) => (Colors::EACH.iter().zip(Colors::NAMES))
                .find_map(|(&c, name)| (c == color).then_some(name))
                .unwrap_or("No color")
                .to_owned(),
            ChosenValue::CreatureType(Some(n)) => t.subtypes().subtype(n).map_or_else(
                || "Another creature type".to_owned(),
                |s| s.name().to_owned(),
            ),
            ChosenValue::CreatureType(None) => "Another creature type".to_owned(),
            ChosenValue::Name(Some(c)) => t.def(c).name.to_owned(),
            ChosenValue::Name(None) => "Another card".to_owned(),
        }
    }

    /// Whether it is a value: a color is exactly one.
    #[inline]
    pub(crate) fn holds(self) -> bool {
        match self {
            ChosenValue::Color(color) => color.0.is_power_of_two(),
            ChosenValue::CreatureType(_) | ChosenValue::Name(_) => true,
        }
    }

    /// The value in layout `M`: a card at the same index.
    #[inline]
    pub(crate) fn relayout<M: Layout>(self) -> ChosenValue<M> {
        match self {
            ChosenValue::Color(color) => ChosenValue::Color(color),
            ChosenValue::CreatureType(n) => ChosenValue::CreatureType(n),
            ChosenValue::Name(c) => ChosenValue::Name(c.map(card)),
        }
    }
}
