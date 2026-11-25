//! The ways a spell may be cast besides paying its mana cost: alternative and additional
//! costs, and which of them a cast paid.

use crate::cost::{Chosen, Cost, CostAction, CostPart, Measure};
use crate::definition::Face;
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::{AbilityIx, CastingIx};
use crate::mana::ManaCost;

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
    /// at index `ability`, an untargeted [`AbilityKind::Spell`](crate::AbilityKind::Spell) the face lists after its own.
    Overload { cost: ManaCost, ability: AbilityIx },
    /// [CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a): "Evoke [cost]" means
    /// "You may cast this card by paying [cost] rather than paying its mana cost" and "When
    /// this permanent enters, if its evoke cost was paid, its controller sacrifices it." The
    /// cost is `cost` and `parts`, as Subtlety's "Evoke--Exile a blue card from your hand."
    /// The face lists [`Ability::evoke`](crate::Ability::evoke) too.
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
    /// Prototype ([CR 702.160a]): "A player who casts a spell with prototype can choose to
    /// cast that card 'prototyped.' If they do, the alternative set of its power, toughness,
    /// and mana cost characteristics are used." The face lists it with that set, and the
    /// definition supplies the face it makes ([`crate::Face::prototyped`]) after the printed
    /// one, at [`crate::FaceIx::PROTOTYPE`]. A prototyped spell is cast as that face with
    /// this option chosen, so it is not an alternative cost (see [CR 719.3]) and adds none.
    ///
    /// [CR 702.160a]: https://yawgatog.com/resources/magic-rules/#R702160a
    /// [CR 719.3]: https://yawgatog.com/resources/magic-rules/#R7193
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
    /// it.
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
            | CastingOption::Mutate { cost, .. } => cost,
            CastingOption::Morph(_) => ManaCost::generic(3),
            // The prototyped face's own mana cost is the spell's: nothing is added to it.
            CastingOption::Fuse | CastingOption::Prototype { .. } => ManaCost::NONE,
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

    /// "Prototype [cost] -- [power]/[toughness]" ([`CastingOption::Prototype`]), as the card
    /// store's generated face writes it.
    pub const fn prototype(cost: ManaCost, power: i8, toughness: i8) -> CastingOption {
        CastingOption::Prototype {
            cost,
            power,
            toughness,
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
