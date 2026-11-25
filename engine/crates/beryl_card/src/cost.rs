//! Costs: what activating an ability takes, and the alternative and additional costs a spell
//! may be cast with.

use crate::characteristics::Types;
use crate::counter::CounterKind;
use crate::definition::Face;
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::{AbilityIx, CastingIx};
use crate::mana::ManaCost;

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

/// An alternative or additional cost printed on a face, which its caster may choose to pay as
/// they cast it ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastingOption {
    /// [CR 702.33a](https://yawgatog.com/resources/magic-rules/#R70233a): "'Kicker [cost]'
    /// means 'You may pay an additional [cost] as you cast this spell.'" `min_x` is the
    /// least X a kicker of {X} allows, 1 for "X can't be 0".
    Kicker { cost: ManaCost, min_x: u8 },
    /// [CR 702.175a](https://yawgatog.com/resources/magic-rules/#R702175a): "'Offspring
    /// [cost]' means 'You may pay an additional [cost] as you cast this spell'".
    Offspring(ManaCost),
    /// [CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a): "'Impending
    /// N--[cost]' means 'You may choose to pay [cost] rather than pay this spell's mana
    /// cost,' 'If you chose to pay this permanent's impending cost, it enters with N time
    /// counters on it,'".
    Impending { cost: ManaCost, counters: u8 },
    /// [CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a): "Overload [cost]"
    /// means "You may choose to pay [cost] rather than pay this spell's mana cost" and "If you
    /// chose to pay this spell's overload cost, change its text by replacing all instances of
    /// the word 'target' with the word 'each.'" The changed text is the face's spell ability
    /// at index `ability`, an untargeted [`AbilityKind::Spell`] the face lists after its own.
    Overload { cost: ManaCost, ability: AbilityIx },
    /// [CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a): "Evoke [cost]" means
    /// "You may cast this card by paying [cost] rather than paying its mana cost" and "When
    /// this permanent enters, if its evoke cost was paid, its controller sacrifices it." The
    /// cost is `cost` and `parts`, as Subtlety's "Evoke--Exile a blue card from your hand."
    /// The face lists [`Ability::evoke`] too.
    Evoke {
        cost: ManaCost,
        parts: &'static [CostPart],
    },
    /// An alternative cost no keyword names
    /// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)): "Some spells have
    /// alternative costs. An alternative cost is a cost listed in a spell's text, or applied
    /// to it from another effect, that its controller may pay rather than paying the spell's
    /// mana cost." The cost is `cost` and `parts`, as Force of Will's "You may pay 1 life and
    /// exile a blue card from your hand rather than pay this spell's mana cost."
    Alternative {
        cost: ManaCost,
        parts: &'static [CostPart],
    },
    /// Escape ([CR 702.138a]): "Escape represents a static ability that functions while the
    /// card with escape is in a player's graveyard. 'Escape [cost]' means 'You may cast this
    /// card from your graveyard by paying [cost] rather than paying its mana cost.'" No face
    /// lists it: it is the escape an effect grants a card in a graveyard, whose cost is the
    /// card's mana cost and `parts`, as Underworld Breach's "The escape cost is equal to the
    /// card's mana cost plus exile three other cards from your graveyard." A cast for it
    /// carries [`Castings::ESCAPE`], and [`Castings::paid`] yields it.
    ///
    /// [CR 702.138a]: https://yawgatog.com/resources/magic-rules/#R702138a
    Escape {
        cost: ManaCost,
        parts: &'static [CostPart],
    },
    /// Mutate ([CR 702.140a]): "'Mutate [cost]' means 'You may pay [cost] rather than pay this
    /// spell's mana cost. If you do, it becomes a mutating creature spell and targets a
    /// non-Human creature with the same owner as this spell.'" The mutating creature spell's target and what it does as it resolves are the
    /// face's spell ability at index `ability` ([`crate::Effect::Mutate`]), which the face
    /// lists after its own and which only a spell cast for its mutate cost has.
    ///
    /// [CR 702.140a]: https://yawgatog.com/resources/magic-rules/#R702140a
    Mutate { cost: ManaCost, ability: AbilityIx },
    /// Morph ([CR 702.37a]): "You may cast this card as a 2/2 face-down creature with
    /// no text, no name, no subtypes, and no mana cost by paying {3} rather than paying
    /// its mana cost." The listed cost turns the permanent face up (see CR 702.37e).
    ///
    /// [CR 702.37a]: https://yawgatog.com/resources/magic-rules/#R70237a
    Morph(Cost),
    /// Choose both halves of a split card when casting it from hand.
    Fuse,
    /// Prototype ([CR 702.160a]): "A player who casts a spell with prototype can choose to cast
    /// that card 'prototyped.' If they do, the alternative set of its power, toughness, and mana
    /// cost characteristics are used."
    ///
    /// [CR 702.160a]: https://yawgatog.com/resources/magic-rules/#R702160a
    Prototype {
        cost: ManaCost,
        power: i8,
        toughness: i8,
    },
}

/// The part of a granted escape cost that is not mana ([`CastingOption::Escape`]): "exile
/// three other cards from your graveyard". The one card that grants escape says three, so the
/// number is fixed here rather than carried by every cast.
pub static ESCAPE_EXILES: [CostPart; 1] = [CostPart::Chosen(
    CostAction::Exile,
    Chosen {
        zone: Zone::Graveyard,
        filter: Filter::Any,
        other: true,
        measure: Measure::Count(3),
    },
)];

/// Which keyword a [`CastingOption`] is, as an effect asks whether its cost was paid
/// ([`crate::Condition::Paid`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastingKind {
    Kicker,
    Offspring,
    Impending,
    Overload,
    Evoke,
    Alternative,
    /// Escape, as "it escaped" asks ([CR 702.138b]): "A spell or permanent 'escaped' if that
    /// spell or the spell that became that permanent as it resolved was cast from a graveyard
    /// with an escape ability."
    ///
    /// [CR 702.138b]: https://yawgatog.com/resources/magic-rules/#R702138b
    Escape,
    Mutate,
}

impl CastingKind {
    /// This kind's bit position in [`CastingKinds`].
    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// A set of [`CastingKind`]s: the keywords whose costs were paid for one spell, as the
/// permanent it becomes remembers them. A bit for each kind of the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CastingKinds(u8);

impl CastingKinds {
    pub const NONE: CastingKinds = CastingKinds(0);

    /// The set with `kind` in it too.
    #[inline]
    #[must_use]
    pub const fn with(self, kind: CastingKind) -> CastingKinds {
        let index = kind.index();
        assert!(
            index < u8::BITS as usize,
            "more kinds than the set has bits"
        );
        CastingKinds(self.0 | 1 << index)
    }

    #[inline]
    pub const fn contains(self, kind: CastingKind) -> bool {
        self.0 & CastingKinds::NONE.with(kind).0 != 0
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The set as a bitmask, a bit for each kind in the order of the list.
    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl CastingOption {
    /// "Overload [cost]", whose changed text is the face's ability at `ability`, counting
    /// from 0: [`CastingOption::Overload`], as a card file writes it.
    pub const fn overload(cost: ManaCost, ability: u8) -> CastingOption {
        CastingOption::Overload {
            cost,
            ability: AbilityIx::new(ability),
        }
    }

    /// "Mutate [cost]", whose mutating creature spell's ability is the face's ability at
    /// `ability`, counting from 0: [`CastingOption::Mutate`], as a card file writes it.
    pub const fn mutate(cost: ManaCost, ability: u8) -> CastingOption {
        CastingOption::Mutate {
            cost,
            ability: AbilityIx::new(ability),
        }
    }

    /// "Prototype [cost] — [power]/[toughness]": [`CastingOption::Prototype`].
    pub const fn prototype(cost: ManaCost, power: i8, toughness: i8) -> CastingOption {
        CastingOption::Prototype {
            cost,
            power,
            toughness,
        }
    }

    /// The face's ability that only a spell cast with this option paid has: an overloaded
    /// spell's changed text, or a mutating creature spell's target and merge.
    pub const fn ability(self) -> Option<AbilityIx> {
        match self {
            CastingOption::Overload { ability, .. } | CastingOption::Mutate { ability, .. } => {
                Some(ability)
            }
            CastingOption::Kicker { .. }
            | CastingOption::Offspring(_)
            | CastingOption::Impending { .. }
            | CastingOption::Evoke { .. }
            | CastingOption::Alternative { .. }
            | CastingOption::Escape { .. }
            | CastingOption::Morph(_)
            | CastingOption::Fuse
            | CastingOption::Prototype { .. } => None,
        }
    }

    /// The kind of cost paid by choosing this option. Morph, fuse and prototype select a
    /// different set of spell characteristics, which supply the base cost.
    pub const fn kind(self) -> Option<CastingKind> {
        Some(match self {
            CastingOption::Kicker { .. } => CastingKind::Kicker,
            CastingOption::Offspring(_) => CastingKind::Offspring,
            CastingOption::Impending { .. } => CastingKind::Impending,
            CastingOption::Overload { .. } => CastingKind::Overload,
            CastingOption::Evoke { .. } => CastingKind::Evoke,
            CastingOption::Alternative { .. } => CastingKind::Alternative,
            CastingOption::Escape { .. } => CastingKind::Escape,
            CastingOption::Mutate { .. } => CastingKind::Mutate,
            CastingOption::Morph(_) | CastingOption::Fuse | CastingOption::Prototype { .. } => {
                return None;
            }
        })
    }

    /// Whether it is paid rather than the mana cost
    /// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)) instead of on top of
    /// it. Prototype is not an alternative cost (see CR 702.160a).
    pub fn is_alternative(self) -> bool {
        matches!(
            self,
            CastingOption::Impending { .. }
                | CastingOption::Overload { .. }
                | CastingOption::Evoke { .. }
                | CastingOption::Alternative { .. }
                | CastingOption::Escape { .. }
                | CastingOption::Mutate { .. }
                | CastingOption::Morph(_)
        )
    }

    /// Its mana cost: all of it, or the mana part of an evoke or alternative cost.
    pub fn cost(self) -> ManaCost {
        match self {
            CastingOption::Kicker { cost, .. }
            | CastingOption::Offspring(cost)
            | CastingOption::Impending { cost, .. }
            | CastingOption::Overload { cost, .. }
            | CastingOption::Evoke { cost, .. }
            | CastingOption::Alternative { cost, .. }
            | CastingOption::Escape { cost, .. }
            | CastingOption::Mutate { cost, .. }
            | CastingOption::Prototype { cost, .. } => cost,
            CastingOption::Morph(_) => ManaCost::generic(3),
            CastingOption::Fuse => ManaCost::NONE,
        }
    }

    /// The parts of its cost that are not mana.
    pub fn parts(self) -> &'static [CostPart] {
        match self {
            CastingOption::Evoke { parts, .. }
            | CastingOption::Alternative { parts, .. }
            | CastingOption::Escape { parts, .. } => parts,
            CastingOption::Kicker { .. }
            | CastingOption::Offspring(_)
            | CastingOption::Impending { .. }
            | CastingOption::Overload { .. }
            | CastingOption::Mutate { .. }
            | CastingOption::Morph(_)
            | CastingOption::Fuse
            | CastingOption::Prototype { .. } => &[],
        }
    }
}

/// A set of a face's casting options ([`Face::casting`]): those chosen for one cast, as its
/// decision option and stack entry record them. A bit for each option the face lists, at
/// most [`Castings::MOST`], and [`Castings::ESCAPE`] for a cast with the escape an effect
/// grants the card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Castings(u8);

impl Castings {
    pub const NONE: Castings = Castings(0);
    /// The fuse option of the combined spell characteristics.
    pub const FUSE: Castings = Castings::only(0);

    /// The most options a face lists: the last bit is [`Castings::ESCAPE`].
    pub const MOST: usize = 7;

    /// Cast with the escape an effect grants the card ([`CastingOption::Escape`]), its cost
    /// the card's mana cost and [`ESCAPE_EXILES`].
    pub const ESCAPE: Castings = Castings(1 << Castings::MOST);

    /// The option at index `i` in [`Face::casting`] alone, counting from 0, as a scenario
    /// writes it.
    pub const fn only(i: u8) -> Castings {
        assert!((i as usize) < Castings::MOST, "seven options at most");
        Castings(1 << i)
    }

    /// Every set of `face`'s options, the empty one first.
    pub fn subsets(face: &Face) -> impl Iterator<Item = Castings> {
        let n = face.casting.len();
        debug_assert!(n <= Castings::MOST);
        (0..(1u16 << n)).map(|m| Castings(u8::try_from(m).expect("seven options at most")))
    }

    /// The same options, with the granted escape too.
    #[must_use]
    pub const fn with_escape(self) -> Castings {
        Castings(self.0 | Castings::ESCAPE.0)
    }

    /// Whether it is a cast with the escape an effect grants ([`Castings::ESCAPE`]).
    pub const fn escapes(self) -> bool {
        self.0 & Castings::ESCAPE.0 != 0
    }

    /// Whether it has the option at `option` in [`Face::casting`].
    pub fn contains(self, option: CastingIx) -> bool {
        usize::from(option.get()) < Castings::MOST && self.0 & (1 << option.get()) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The set as a bitmask over [`Face::casting`], the form scenario files are exported in.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// The indices in [`Face::casting`] of its options, in order: not the granted escape,
    /// which the face doesn't list.
    pub fn indices(self) -> impl Iterator<Item = CastingIx> {
        (0..u8::try_from(Castings::MOST).expect("the option count fits in a byte"))
            .filter(move |&i| self.0 & (1 << i) != 0)
            .map(CastingIx::new)
    }

    /// The options of `face` it has, in the face's order, and then the granted escape, its
    /// cost the face's mana cost ([`CastingOption::Escape`]).
    pub fn paid(self, face: &Face) -> impl Iterator<Item = CastingOption> + '_ {
        let escape = self.escapes().then_some(CastingOption::Escape {
            cost: face.cost,
            parts: &ESCAPE_EXILES,
        });
        face.casting
            .iter()
            .enumerate()
            .filter(move |&(i, _)| i < Castings::MOST && self.0 & (1 << i) != 0)
            .map(|(_, &o)| o)
            .chain(escape)
    }
}
