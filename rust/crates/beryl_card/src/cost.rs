//! Costs: what activating an ability takes, and the alternative and additional costs a spell
//! may be cast with.

use crate::characteristics::Colors;
use crate::definition::Face;
use crate::ids::CounterKind;
use crate::index::{AbilityIx, CastingIx};
use crate::mana::ManaCost;

/// An activation cost: mana, and the other things paying it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cost {
    pub mana: ManaCost,
    /// The tap symbol ([CR 107.5](https://yawgatog.com/resources/magic-rules/#R1075)): "The tap
    /// symbol in an activation cost means 'Tap this permanent.'"
    pub tap: bool,
    /// "Sacrifice this permanent"
    /// ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)): "To sacrifice a
    /// permanent, its controller moves it from the battlefield directly to its owner's
    /// graveyard."
    pub sacrifice_self: bool,
    /// "Discard this card". Only a card in a hand can be discarded, so an ability with this
    /// cost is activated from its owner's hand, as cycling is.
    pub discard_self: bool,
    /// A loyalty ability's cost: loyalty counters to put on (positive) or remove (negative)
    /// ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)). `Some(0)` is a
    /// loyalty ability too.
    pub loyalty: Option<i8>,
    /// Crew's cost: tap other untapped creatures you control with at least this much total
    /// power, or 0 for none
    /// ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a)): "'Crew N' means
    /// 'Tap any number of other untapped creatures you control with total power N or greater:
    /// This permanent becomes an artifact creature until end of turn.'"
    pub crew: u8,
    /// "Remove a [kind] counter from this permanent", as Staff of the Storyteller's.
    pub remove: Option<CounterKind>,
    /// How much of the generic mana is a waterbend cost's
    /// ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a)): "'Waterbend
    /// [cost]' means 'Pay [cost]. For each generic mana in that cost, you may tap an untapped
    /// artifact or creature you control rather than pay that mana.'" Only that much may be
    /// paid by tapping ([CR 701.67b](https://yawgatog.com/resources/magic-rules/#R70167b)).
    pub waterbend: u8,
}

impl Cost {
    pub const NONE: Cost = Cost {
        mana: ManaCost::NONE,
        tap: false,
        sacrifice_self: false,
        discard_self: false,
        loyalty: None,
        crew: 0,
        remove: None,
        waterbend: 0,
    };
    pub const TAP: Cost = Cost {
        tap: true,
        ..Cost::NONE
    };

    /// A cost of mana only.
    pub const fn mana(mana: ManaCost) -> Cost {
        Cost { mana, ..Cost::NONE }
    }

    /// A loyalty ability's cost.
    pub const fn loyalty(n: i8) -> Cost {
        Cost {
            loyalty: Some(n),
            ..Cost::NONE
        }
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
    /// counters on it'".
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
    /// cost is `cost` and, unless `exile` is empty, a card of one of those colors exiled from
    /// its caster's hand, as Subtlety's "Evoke--Exile a blue card from your hand." The face
    /// lists [`Ability::evoke`] too.
    Evoke { cost: ManaCost, exile: Colors },
}

/// Which keyword a [`CastingOption`] is, as an effect asks whether its cost was paid
/// ([`crate::Condition::Paid`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastingKind {
    Kicker,
    Offspring,
    Impending,
    Overload,
    Evoke,
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

    /// Which keyword it is.
    pub const fn kind(self) -> CastingKind {
        match self {
            CastingOption::Kicker { .. } => CastingKind::Kicker,
            CastingOption::Offspring(_) => CastingKind::Offspring,
            CastingOption::Impending { .. } => CastingKind::Impending,
            CastingOption::Overload { .. } => CastingKind::Overload,
            CastingOption::Evoke { .. } => CastingKind::Evoke,
        }
    }

    /// Whether it is paid rather than the mana cost
    /// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)) instead of on top of
    /// it.
    pub fn is_alternative(self) -> bool {
        matches!(
            self,
            CastingOption::Impending { .. }
                | CastingOption::Overload { .. }
                | CastingOption::Evoke { .. }
        )
    }

    /// Its mana cost: all of it, or the mana part of an evoke cost.
    pub fn cost(self) -> ManaCost {
        match self {
            CastingOption::Kicker { cost, .. }
            | CastingOption::Offspring(cost)
            | CastingOption::Impending { cost, .. }
            | CastingOption::Overload { cost, .. }
            | CastingOption::Evoke { cost, .. } => cost,
        }
    }
}

/// A set of a face's casting options ([`Face::casting`]): those chosen for one cast, as its
/// decision option and stack entry record them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Castings(u8);

impl Castings {
    pub const NONE: Castings = Castings(0);

    /// The option at index `i` in [`Face::casting`] alone, counting from 0, as a scenario
    /// writes it.
    pub const fn only(i: u8) -> Castings {
        assert!(i < 8, "eight options at most");
        Castings(1 << i)
    }

    /// Every set of `face`'s options, the empty one first.
    pub fn subsets(face: &Face) -> impl Iterator<Item = Castings> {
        let n = face.casting.len();
        debug_assert!(n <= 8);
        (0..(1u16 << n)).map(|m| Castings(u8::try_from(m).expect("eight options at most")))
    }

    /// Whether it has the option at `option` in [`Face::casting`].
    pub fn contains(self, option: CastingIx) -> bool {
        option.get() < 8 && self.0 & (1 << option.get()) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The set as a bitmask over [`Face::casting`], the form scenario files are exported in.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// The indices in [`Face::casting`] of its options, in order.
    pub fn indices(self) -> impl Iterator<Item = CastingIx> {
        (0..8)
            .filter(move |&i| self.0 & (1 << i) != 0)
            .map(CastingIx::new)
    }

    /// The options of `face` it has, in the face's order.
    pub fn paid(self, face: &Face) -> impl Iterator<Item = CastingOption> + '_ {
        face.casting
            .iter()
            .enumerate()
            .filter(move |&(i, _)| i < 8 && self.0 & (1 << i) != 0)
            .map(|(_, &o)| o)
    }
}
