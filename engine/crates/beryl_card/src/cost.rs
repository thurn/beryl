//! Costs: what activating an ability takes, and the alternative and additional costs a spell
//! may be cast with.

use crate::characteristics::Types;
use crate::counter::CounterKind;
use crate::filter::Filter;
use crate::ids::Zone;
use crate::mana::ManaCost;

mod casting;
pub use casting::{CastingKind, CastingKinds, CastingOption, Castings, ESCAPE_EXILES};

/// A cost ([CR 118.1](https://yawgatog.com/resources/magic-rules/#R1181)): "A cost is an
/// action or payment necessary to take another action or to stop another action from taking
/// place." Mana, and the parts that are paid some other way, in the order the card prints
/// them. An activation cost is one
/// ([CR 602.1a](https://yawgatog.com/resources/magic-rules/#R6021a)): "The activation cost
/// is everything before the colon (:)."
///
/// An ability holds its cost, and the engine reads every ability of a face for each decision
/// it builds. So the parts lie behind a reference, and a cost keeps which kinds of part it
/// has, found as it is built: most costs have none, and what the engine asks of every cost
/// (does it tap its source?) is then one bit, with no part read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cost {
    pub mana: ManaCost,
    parts: &'static [CostPart],
    /// A bit of [`kinds`] for each kind of part among `parts`.
    kinds: u8,
}

/// The kinds of part a [`Cost`] says it has.
mod kinds {
    pub(super) const TAP: u8 = 1 << 0;
    pub(super) const SACRIFICE: u8 = 1 << 1;
    pub(super) const DISCARD: u8 = 1 << 2;
    /// Another thing done to the object the cost is on.
    pub(super) const THIS: u8 = 1 << 3;
    pub(super) const LOYALTY: u8 = 1 << 4;
    pub(super) const CHOSEN: u8 = 1 << 5;
    /// A payment of life or a choice of costs.
    pub(super) const OTHER: u8 = 1 << 6;
    /// Discarding the whole hand.
    pub(super) const HAND: u8 = 1 << 7;
}

/// One part of a cost that is not mana: what is done, and to what.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CostPart {
    /// Done to the object the cost is on: "{T}", "Sacrifice this artifact", "Discard this
    /// card", "Remove a story counter from this artifact".
    This(CostAction),
    /// Done to objects the player paying chooses: "Sacrifice a creature", "Exile six cards
    /// from your graveyard", crew's "Tap any number of other untapped creatures you control
    /// with total power N or greater".
    Chosen(CostAction, Chosen),
    /// A loyalty ability's cost: loyalty counters to put on (positive) or remove (negative)
    /// ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)): "The cost to activate
    /// a loyalty ability of a permanent is to put on or remove from that permanent a certain
    /// number of loyalty counters". `Loyalty(0)` is a loyalty ability's cost too.
    Loyalty(i8),
    /// "Pay N life" ([CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194)): "If a
    /// player pays life, the payment is subtracted from their life total".
    Life(u8),
    /// One of these costs, as its payer chooses: "reveal an Elemental card from your hand or
    /// pay {3}", or to forage
    /// ([CR 701.61a](https://yawgatog.com/resources/magic-rules/#R70161a)): "To forage means
    /// 'Exile three cards from your graveyard or sacrifice a Food.'"
    OneOf(&'static [Cost]),
    /// "Discard your hand": every card in the hand of the player paying, each moved as
    /// discarding moves a card ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)):
    /// "To discard a card, move it from its owner's hand to that player's graveyard." No card
    /// is chosen, and an empty hand pays it.
    DiscardHand,
}

/// What paying a cost part does to an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CostAction {
    /// [CR 701.26a](https://yawgatog.com/resources/magic-rules/#R70126a): "To tap a
    /// permanent, turn it sideways from an upright position. Only untapped permanents can be
    /// tapped." Of the object the cost is on, the tap symbol
    /// ([CR 107.5](https://yawgatog.com/resources/magic-rules/#R1075)): "The tap symbol in an
    /// activation cost means 'Tap this permanent.'"
    Tap,
    /// [CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a): "To sacrifice a
    /// permanent, its controller moves it from the battlefield directly to its owner's
    /// graveyard."
    Sacrifice,
    /// [CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a): "To discard a card,
    /// move it from its owner's hand to that player's graveyard." Only a card in a hand can
    /// be discarded, so an ability that discards the card it is on is activated from its
    /// owner's hand, as cycling is.
    Discard,
    /// [CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a): "To exile an
    /// object, move it to the exile zone from wherever it is."
    Exile,
    /// [CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a): "To reveal a card,
    /// show that card to all players for a brief time."
    Reveal,
    /// "Remove a [kind] counter from" it.
    RemoveCounter(CounterKind),
}

/// The objects a cost part is paid with, which the player paying chooses as they pay
/// ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)): "The player pays the
/// total cost."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chosen {
    /// Where they are: permanents the player controls for the battlefield, and for a hand
    /// or a graveyard the cards in the player's own.
    pub zone: Zone,
    /// Which of the objects there may pay.
    pub filter: Filter,
    /// "Another", "other": never the object the cost is on.
    pub other: bool,
    /// How many.
    pub measure: Measure,
}

/// How many objects pay a cost part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Measure {
    /// This many.
    Count(u8),
    /// Any number with at least this much total power, as crew's
    /// ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a)): "'Crew N' means
    /// 'Tap any number of other untapped creatures you control with total power N or
    /// greater: This permanent becomes an artifact creature until end of turn.'"
    TotalPower(u8),
    /// Any number up to this, each rather than one generic mana of the cost, as a
    /// waterbend cost's
    /// ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a)): "'Waterbend
    /// [cost]' means 'Pay [cost]. For each generic mana in that cost, you may tap an untapped
    /// artifact or creature you control rather than pay that mana.'" Only that much may be
    /// paid by tapping ([CR 701.67b](https://yawgatog.com/resources/magic-rules/#R70167b)):
    /// "the alternate method to pay for mana described in rule 701.67a may be used only to
    /// pay for the amount of generic mana in the waterbend cost".
    ForGeneric(u8),
    /// Any number, each rather than one mana of the total cost: one generic mana, or one
    /// colored mana of a color the object has, as convoke's
    /// ([CR 702.51a](https://yawgatog.com/resources/magic-rules/#R70251a)): "For each colored
    /// mana in this spell's total cost, you may tap an untapped creature of that color you
    /// control rather than pay that mana. For each generic mana in this spell's total cost,
    /// you may tap an untapped creature you control rather than pay that mana." An object
    /// never pays a colorless symbol ({C}) this way.
    ForMana,
}

impl CostPart {
    /// "{T}" (CR 107.5).
    pub const TAP: CostPart = CostPart::This(CostAction::Tap);
    /// "Sacrifice this permanent".
    pub const SACRIFICE: CostPart = CostPart::This(CostAction::Sacrifice);
    /// "Discard this card".
    pub const DISCARD: CostPart = CostPart::This(CostAction::Discard);

    /// "Remove a [kind] counter from this permanent".
    pub const fn remove(kind: CounterKind) -> CostPart {
        CostPart::This(CostAction::RemoveCounter(kind))
    }

    /// Crew's cost (CR 702.122a): "Tap any number of other untapped creatures you control
    /// with total power N or greater".
    pub const fn crew(n: u8) -> CostPart {
        CostPart::Chosen(
            CostAction::Tap,
            Chosen {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
                other: true,
                measure: Measure::TotalPower(n),
            },
        )
    }

    /// The part of "Waterbend [cost]" paid by tapping (CR 701.67a), for a cost with `n`
    /// generic mana: "For each generic mana in that cost, you may tap an untapped artifact
    /// or creature you control rather than pay that mana."
    pub const fn waterbend(n: u8) -> CostPart {
        CostPart::Chosen(
            CostAction::Tap,
            Chosen {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::ARTIFACT.union(Types::CREATURE)),
                other: false,
                measure: Measure::ForGeneric(n),
            },
        )
    }

    /// "Exile N [filter] cards from your [zone]".
    pub const fn exile(count: u8, zone: Zone, filter: Filter) -> CostPart {
        CostPart::Chosen(
            CostAction::Exile,
            Chosen {
                zone,
                filter,
                other: false,
                measure: Measure::Count(count),
            },
        )
    }

    /// "Sacrifice a [filter]".
    pub const fn sacrifice(filter: Filter) -> CostPart {
        CostPart::Chosen(
            CostAction::Sacrifice,
            Chosen {
                zone: Zone::Battlefield,
                filter,
                other: false,
                measure: Measure::Count(1),
            },
        )
    }

    /// "Sacrifice another [filter]".
    pub const fn sacrifice_another(filter: Filter) -> CostPart {
        CostPart::Chosen(
            CostAction::Sacrifice,
            Chosen {
                zone: Zone::Battlefield,
                filter,
                other: true,
                measure: Measure::Count(1),
            },
        )
    }

    /// "Discard a [filter] card".
    pub const fn discard(filter: Filter) -> CostPart {
        CostPart::Chosen(
            CostAction::Discard,
            Chosen {
                zone: Zone::Hand,
                filter,
                other: false,
                measure: Measure::Count(1),
            },
        )
    }

    /// "Reveal a [filter] card from your hand".
    pub const fn reveal(filter: Filter) -> CostPart {
        CostPart::Chosen(
            CostAction::Reveal,
            Chosen {
                zone: Zone::Hand,
                filter,
                other: false,
                measure: Measure::Count(1),
            },
        )
    }
}

/// [`CostPart::Loyalty`] for every loyalty cost, by its value as a byte, and
/// [`CostPart::crew`] for every N. A cost holds its parts by reference, and a `const fn`
/// cannot make a reference to a value it builds from an argument, so [`Cost::loyalty`] and
/// [`Cost::crew`] point here instead.
static LOYALTY: [[CostPart; 1]; 256] = {
    let mut table = [[CostPart::Loyalty(0)]; 256];
    let mut n = 0u8;
    loop {
        table[n as usize] = [CostPart::Loyalty(i8::from_ne_bytes([n]))];
        if n == u8::MAX {
            break;
        }
        n += 1;
    }
    table
};
static CREW: [[CostPart; 1]; 256] = {
    let mut table = [[CostPart::crew(0)]; 256];
    let mut n = 0u8;
    loop {
        table[n as usize] = [CostPart::crew(n)];
        if n == u8::MAX {
            break;
        }
        n += 1;
    }
    table
};

impl Cost {
    pub const NONE: Cost = Cost::of(ManaCost::NONE, &[]);
    pub const TAP: Cost = Cost::of_parts(&[CostPart::TAP]);

    /// A cost of mana only.
    pub const fn mana(mana: ManaCost) -> Cost {
        Cost::of(mana, &[])
    }

    /// A cost of no mana and one part.
    pub const fn part(part: &'static CostPart) -> Cost {
        Cost::of_parts(std::slice::from_ref(part))
    }

    /// A cost of no mana: these parts, in the order printed.
    pub const fn of_parts(parts: &'static [CostPart]) -> Cost {
        Cost::of(ManaCost::NONE, parts)
    }

    /// Mana and parts, in the order printed.
    pub const fn of(mana: ManaCost, parts: &'static [CostPart]) -> Cost {
        let mut kinds = 0;
        let mut i = 0;
        while i < parts.len() {
            kinds |= match parts[i] {
                CostPart::This(CostAction::Tap) => kinds::TAP,
                CostPart::This(CostAction::Sacrifice) => kinds::SACRIFICE,
                CostPart::This(CostAction::Discard) => kinds::DISCARD,
                CostPart::This(_) => kinds::THIS,
                CostPart::Loyalty(_) => kinds::LOYALTY,
                CostPart::Chosen(..) => kinds::CHOSEN,
                CostPart::Life(_) | CostPart::OneOf(_) => kinds::OTHER,
                CostPart::DiscardHand => kinds::HAND,
            };
            i += 1;
        }
        Cost { mana, parts, kinds }
    }

    /// A loyalty ability's cost.
    pub const fn loyalty(n: i8) -> Cost {
        Cost::of_parts(&LOYALTY[n.to_ne_bytes()[0] as usize])
    }

    /// Crew N's cost (CR 702.122a).
    pub const fn crew(n: u8) -> Cost {
        Cost::of_parts(&CREW[n as usize])
    }

    /// Its parts that are not mana, in the order printed.
    #[inline]
    pub const fn parts(&self) -> &'static [CostPart] {
        self.parts
    }

    /// Whether paying it does `action` to the object it is on.
    #[inline]
    pub fn does(&self, action: CostAction) -> bool {
        match action {
            CostAction::Tap => self.kinds & kinds::TAP != 0,
            CostAction::Sacrifice => self.kinds & kinds::SACRIFICE != 0,
            CostAction::Discard => self.kinds & kinds::DISCARD != 0,
            CostAction::Exile | CostAction::Reveal | CostAction::RemoveCounter(_) => {
                self.kinds & kinds::THIS != 0
                    && self
                        .parts
                        .iter()
                        .any(|part| matches!(part, CostPart::This(done) if *done == action))
            }
        }
    }

    /// Whether it is a loyalty ability's cost
    /// ([CR 606.1](https://yawgatog.com/resources/magic-rules/#R6061)).
    #[inline]
    pub const fn is_loyalty(&self) -> bool {
        self.kinds & kinds::LOYALTY != 0
    }

    /// Whether it has a part paid with objects its player chooses ([`CostPart::Chosen`]).
    #[inline]
    pub const fn chooses(&self) -> bool {
        self.kinds & kinds::CHOSEN != 0
    }

    /// Whether it has a payment of life or a choice of costs ([`CostPart::Life`],
    /// [`CostPart::OneOf`]): most costs have neither, so a caller may skip looking at the parts.
    #[inline]
    pub const fn pays_life_or_chooses(&self) -> bool {
        self.kinds & kinds::OTHER != 0
    }

    /// Whether paying it discards the hand of the player paying ([`CostPart::DiscardHand`]).
    #[inline]
    pub const fn discards_hand(&self) -> bool {
        self.kinds & kinds::HAND != 0
    }

    /// The loyalty counters it puts on or removes, if it is a loyalty ability's (CR 606.4).
    #[inline]
    pub fn loyalty_change(&self) -> Option<i8> {
        if !self.is_loyalty() {
            return None;
        }
        self.parts.iter().find_map(|part| match part {
            CostPart::Loyalty(n) => Some(*n),
            CostPart::This(_)
            | CostPart::Chosen(..)
            | CostPart::Life(_)
            | CostPart::OneOf(_)
            | CostPart::DiscardHand => None,
        })
    }

    /// The kind of counter it removes from the object it is on, if it removes one.
    #[inline]
    pub fn removes(&self) -> Option<CounterKind> {
        if self.kinds & kinds::THIS == 0 {
            return None;
        }
        self.parts.iter().find_map(|part| match part {
            CostPart::This(CostAction::RemoveCounter(kind)) => Some(*kind),
            CostPart::This(_)
            | CostPart::Chosen(..)
            | CostPart::Loyalty(_)
            | CostPart::Life(_)
            | CostPart::OneOf(_)
            | CostPart::DiscardHand => None,
        })
    }

    /// The measure of the objects its player chooses to pay it with, if it has such a part:
    /// of the first one.
    #[inline]
    pub fn measure(&self) -> Option<Measure> {
        if !self.chooses() {
            return None;
        }
        self.parts.iter().find_map(|part| match part {
            CostPart::Chosen(_, chosen) => Some(chosen.measure),
            CostPart::This(_)
            | CostPart::Loyalty(_)
            | CostPart::Life(_)
            | CostPart::OneOf(_)
            | CostPart::DiscardHand => None,
        })
    }
}
