//! A card and its faces: the characteristics printed on each, and how a stack entry or a
//! decision names one of a face's abilities.

use crate::ability::{Ability, AbilityKind};
use crate::characteristics::{Colors, Keywords, Subtypes, Supertypes, Types};
use crate::cost::{CastingOption, Castings, CostPart};
use crate::effect::CopyException;
use crate::index::{AbilityIx, GrantIx};
use crate::mana::ManaCost;
use crate::words::SubtypeWords;

/// How a card's printed characteristics are divided. Sagas, Classes, Cases and levelers
/// have one face with abilities that depend on counters or designations. A prototype card is
/// a single face whose definition also supplies its prototyped characteristics
/// ([`Face::prototyped`]). Those differences belong in the card vocabulary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Single,
    Transforming,
    Modal,
    Split {
        fuse: bool,
    },
    /// An adventurer card. Its resolving Adventure exiles it, and that permission lets its
    /// controller cast the normal face when they could normally cast a spell of its type
    /// ([CR 716.3d]).
    ///
    /// [CR 716.3d]: https://yawgatog.com/resources/magic-rules/#R7163d
    Adventure,
    Prepared,
    Flip,
    /// One front face. The partner and the meld result are separate card definitions.
    Meld {
        with: &'static str,
        into: &'static str,
    },
}

impl Shape {
    /// The number of printed faces or sets of alternative characteristics in the definition.
    pub const fn face_count(self) -> usize {
        match self {
            Self::Single | Self::Meld { .. } => 1,
            Self::Transforming
            | Self::Modal
            | Self::Split { .. }
            | Self::Adventure
            | Self::Prepared
            | Self::Flip => 2,
        }
    }

    /// Whether the definition includes a combined spell for the fuse ability.
    pub const fn has_fuse(self) -> bool {
        matches!(self, Self::Split { fuse: true })
    }

    /// The number of sets of characteristics, including the fused spell if present.
    pub const fn characteristics_count(self) -> usize {
        self.face_count() + self.has_fuse() as usize
    }

    /// Whether the card itself is double-faced, with a different face printed on each side.
    pub const fn is_double_faced(self) -> bool {
        matches!(self, Self::Transforming | Self::Modal)
    }

    /// Whether the second face is normally available as a spell. A player chooses a modal
    /// face ([CR 712.8b]), a split half ([CR 709.3]), or an Adventure ([CR 716.3]). Only
    /// copies can use a prepare spell's inset characteristics ([CR 723.3]).
    ///
    /// [CR 712.8b]: https://yawgatog.com/resources/magic-rules/#R7128b
    /// [CR 709.3]: https://yawgatog.com/resources/magic-rules/#R7093
    /// [CR 716.3]: https://yawgatog.com/resources/magic-rules/#R7163
    /// [CR 723.3]: https://yawgatog.com/resources/magic-rules/#R7233
    pub const fn can_cast_back(self) -> bool {
        matches!(self, Self::Modal | Self::Split { .. } | Self::Adventure)
    }
}

/// One card: its name and its faces.
#[derive(Debug)]
pub struct CardDef {
    pub name: &'static str,
    pub shape: Shape,
    /// The printed faces, front first, followed by the combined spell for a card with fuse.
    pub faces: &'static [Face],
    /// The tokens and emblems this card can create, so a game's table holds them.
    pub tokens: &'static [&'static CardDef],
}

impl CardDef {
    /// The face a card has in every zone but the battlefield and the stack, where it may have
    /// another one up.
    pub fn front(&self) -> &'static Face {
        self.face(FaceIx::FRONT)
    }

    /// The face at `face`, or for [`FaceIx::DOWN`] the characteristics every face-down
    /// permanent has, [`FACE_DOWN`].
    pub fn face(&self, face: FaceIx) -> &'static Face {
        if face == FaceIx::DOWN {
            return &FACE_DOWN;
        }
        &self.faces[usize::from(face.0)]
    }

    /// Each set of characteristics the definition provides, starting with its printed faces.
    pub fn face_ixs(&self) -> impl Iterator<Item = FaceIx> {
        (0..self.faces.len()).map(|i| FaceIx(u8::try_from(i).expect("a few faces")))
    }

    /// The indices of the printed faces.
    pub fn printed_face_ixs(&self) -> impl Iterator<Item = FaceIx> {
        self.face_ixs().take(self.shape.face_count())
    }

    /// The name of the other printed face when one is up.
    pub fn other_face_name(&self, up: FaceIx) -> Option<&'static str> {
        if self.shape.face_count() != 2 {
            return None;
        }
        match up {
            FaceIx::FRONT => Some(self.face_name(FaceIx::BACK)),
            FaceIx::BACK => Some(self.face_name(FaceIx::FRONT)),
            _ => None,
        }
    }

    /// The name of the face at `face`, the name a spell or permanent with that face up has
    /// ([CR 712.6e](https://yawgatog.com/resources/magic-rules/#R7126e)): "While a modal
    /// double-faced spell is on the stack or a modal double-faced permanent is on the
    /// battlefield, it has only the characteristics of the face that's up." The same holds of
    /// a nonmodal one ([CR 712.6d](https://yawgatog.com/resources/magic-rules/#R7126d)):
    /// "While a nonmodal double-faced permanent has its back face up, it has only the
    /// characteristics of its back face." A face-down permanent has no name (CR 708.2a): the
    /// empty string.
    pub fn face_name(&self, face: FaceIx) -> &'static str {
        if face == FaceIx::FRONT
            && (!matches!(self.shape, Shape::Split { .. } | Shape::Prepared)
                || self.front().name.is_empty())
        {
            self.name
        } else {
            self.face(face).name
        }
    }
}

/// Which of a card's faces: an index into [`CardDef::faces`], read with [`CardDef::face`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FaceIx(u8);

impl FaceIx {
    /// The front face, a card's only face unless it has two.
    pub const FRONT: FaceIx = FaceIx(0);
    /// The back face of a double-faced card.
    pub const BACK: FaceIx = FaceIx(1);
    /// The combined characteristics of a split spell cast with fuse.
    pub const FUSED: FaceIx = FaceIx(2);
    /// The prototyped characteristics of a prototype card, which has one printed face
    /// ([`Face::prototyped`]).
    pub const PROTOTYPE: FaceIx = FaceIx(1);
    /// Face down: a permanent whose card is turned face down
    /// ([CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105)): "A permanent's status
    /// is its physical state. There are four status categories, each of which has two possible
    /// values: tapped/untapped, flipped/unflipped, face up/face down, and phased in/phased
    /// out." Its characteristics are [`FACE_DOWN`]'s, whichever card it is, so every reader
    /// of [`CardDef::face`] sees what a face-down permanent has.
    pub const DOWN: FaceIx = FaceIx(u8::MAX);

    /// The index in [`CardDef::faces`], as wire formats write it.
    pub fn get(self) -> u8 {
        self.0
    }
}

/// The characteristics printed on one face.
#[derive(Debug)]
pub struct Face {
    /// The name of a split half or a back face. Other front faces take [`CardDef::name`], which
    /// is the card's name wherever it is not a permanent or spell with another face up
    /// ([CR 712.6a](https://yawgatog.com/resources/magic-rules/#R7126a)): "While a
    /// double-faced card is outside the game or in a zone other than the battlefield or
    /// stack, it has only the characteristics of its front face."
    pub name: &'static str,
    pub cost: ManaCost,
    pub supertypes: Supertypes,
    pub types: Types,
    /// Its type line's subtypes, words of its text ([`crate::words`]).
    pub subtypes: SubtypeWords,
    pub colors: Colors,
    pub power: Option<i8>,
    pub toughness: Option<i8>,
    pub loyalty: Option<u8>,
    pub defense: Option<u8>,
    pub keywords: Keywords,
    pub abilities: &'static [Ability],
    /// The alternative and additional costs it may be cast with, in the order its text lists
    /// them. A stack entry records the ones paid as a [`Castings`] set over this list.
    pub casting: &'static [CastingOption],
    /// The generated face eligibility and mana-value rule, cached for casting and queries.
    pub use_of_face: FaceUse,
    /// "As an additional cost to cast this spell, [parts]"
    /// ([CR 118.8](https://yawgatog.com/resources/magic-rules/#R1188)): "An additional cost is
    /// a cost listed in a spell's rules text, or applied to a spell or ability from another
    /// effect, that its controller must pay at the same time they pay the spell's mana cost
    /// or the ability's activation cost." One its caster may decline is a
    /// [`CastingOption`] instead.
    pub additional_cost: &'static [CostPart],
    /// Abilities this face grants to another object rather than has itself, such as the crew
    /// ability of Swift Reconfiguration's "Enchanted permanent is a Vehicle artifact with crew
    /// 5". [`Modification::Grant`] names one by its index here.
    pub grants: &'static [Ability],
    /// The exceptions the copies its abilities make are made with, such as offspring's
    /// "except it's 1/1". [`Effect::CreateCopies`](crate::Effect::CreateCopies) names one by
    /// its index here, and a copy keeps that index and this card as an addition to its
    /// copiable values, so the exception itself is copied with them
    /// ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)): "Some copy effects
    /// modify a characteristic as part of the copying process. The final set of values for
    /// that characteristic becomes part of the copiable values of the copy."
    pub excepts: &'static [CopyException],
}

/// Rules derived from a card's shape for readers that already have one of its faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FaceUse {
    Playable,
    TransformingBack,
    Alternative,
    Fused,
    /// The prototyped characteristics of a single-faced card ([`Face::prototyped`]).
    Prototype,
}

impl FaceUse {
    pub(crate) const fn of(shape: Shape, front: bool) -> Self {
        if front || shape.can_cast_back() {
            Self::Playable
        } else if matches!(shape, Shape::Transforming) {
            Self::TransformingBack
        } else {
            Self::Alternative
        }
    }
}

impl Face {
    /// The combined spell of a split card with fuse. Supply its spell ability with the
    /// left half's targets first and both halves' instructions in resolution order.
    /// The name is the whole card's display name. Rules queries read both half names.
    #[must_use]
    pub const fn fused(
        name: &'static str,
        left: &Self,
        right: &Self,
        abilities: &'static [Ability],
    ) -> Self {
        Self {
            name,
            cost: left.cost.plus(right.cost),
            supertypes: left.supertypes.union(right.supertypes),
            types: left.types.union(right.types),
            subtypes: left.subtypes.union(right.subtypes),
            colors: left.colors.union(right.colors),
            keywords: left.keywords.union(right.keywords),
            use_of_face: FaceUse::Fused,
            ..Self::EMPTY
                .abilities(abilities)
                .casting(&[CastingOption::Fuse])
        }
    }

    /// The prototyped characteristics of this face, which lists
    /// [`CastingOption::Prototype`]: the definition's next face, at [`FaceIx::PROTOTYPE`]. It
    /// has the option's mana cost, power and toughness, and the colors of that mana cost
    /// ([CR 719.3b]): "Both a prototyped spell and the permanent it becomes have only its
    /// alternative set of power, toughness, and mana cost characteristics. If that mana cost
    /// includes one or more colored mana symbols, the spell and the permanent it becomes are
    /// also that color or colors". Everything else is this face's ([CR 719.5]): "A prototype
    /// card's characteristics other than its power, toughness, and mana cost (and other than
    /// color) remain the same whether it was cast as a prototyped spell or cast normally."
    ///
    /// Copies read the face that is up, so a copy of a prototyped spell or permanent is
    /// prototyped too (see CR 719.3c and CR 719.3d).
    ///
    /// # Panics
    ///
    /// As the definition compiles, if this face lists no prototype.
    ///
    /// [CR 719.3b]: https://yawgatog.com/resources/magic-rules/#R7193b
    /// [CR 719.5]: https://yawgatog.com/resources/magic-rules/#R7195
    #[must_use]
    pub const fn prototyped(&self) -> Self {
        let mut i = 0;
        while i < self.casting.len() {
            if let CastingOption::Prototype {
                cost,
                power,
                toughness,
            } = self.casting[i]
            {
                let mut colors = Colors::NONE;
                let mut c = 0;
                while c < Colors::EACH.len() {
                    if cost.symbols[c] > 0 {
                        colors = colors.union(Colors::EACH[c]);
                    }
                    c += 1;
                }
                return Self {
                    name: self.name,
                    cost,
                    supertypes: self.supertypes,
                    types: self.types,
                    subtypes: self.subtypes,
                    colors,
                    power: Some(power),
                    toughness: Some(toughness),
                    loyalty: self.loyalty,
                    defense: self.defense,
                    keywords: self.keywords,
                    abilities: self.abilities,
                    casting: self.casting,
                    use_of_face: FaceUse::Prototype,
                    additional_cost: self.additional_cost,
                    grants: self.grants,
                    excepts: self.excepts,
                };
            }
            i += 1;
        }
        panic!("a prototyped face is made of a face that lists a prototype");
    }

    /// Whether this face lists [`CastingOption::Prototype`], so its definition supplies the
    /// prototyped face after it.
    pub const fn has_prototype(&self) -> bool {
        let mut i = 0;
        while i < self.casting.len() {
            if matches!(self.casting[i], CastingOption::Prototype { .. }) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Whether this is a prototype card's prototyped face ([`Face::prototyped`]), which a
    /// cast chooses with its [`CastingOption::Prototype`] and only so.
    #[inline]
    pub const fn is_prototyped(&self) -> bool {
        matches!(self.use_of_face, FaceUse::Prototype)
    }

    /// Applies the definition's generated shape to this face.
    #[must_use]
    pub const fn for_shape(self, shape: Shape, face: FaceIx) -> Self {
        Self {
            use_of_face: FaceUse::of(shape, face.0 == 0),
            ..self
        }
    }

    /// Whether casting or playing the card can normally choose this face. Timing, cost and
    /// whether it is a spell or a land are checked separately.
    #[inline]
    pub const fn playable(&self) -> bool {
        matches!(self.use_of_face, FaceUse::Playable | FaceUse::Prototype)
    }

    /// Whether the mana value of this face comes from the front face ([CR 712.6d]).
    ///
    /// [CR 712.6d]: https://yawgatog.com/resources/magic-rules/#R7126d
    #[inline]
    pub const fn uses_front_mana_value(&self) -> bool {
        matches!(self.use_of_face, FaceUse::TransformingBack)
    }

    /// Whether this face allows its owner to cast it while searching their own library.
    pub fn permits_cast_while_searching(&self) -> bool {
        self.abilities
            .iter()
            .any(Ability::permits_cast_while_searching)
    }

    /// Whether this face is an instant or a sorcery: its spell resolves by following its
    /// instructions, not by becoming a permanent
    /// ([CR 608.2](https://yawgatog.com/resources/magic-rules/#R6082),
    /// [CR 608.3](https://yawgatog.com/resources/magic-rules/#R6083)).
    pub fn is_instant_or_sorcery(&self) -> bool {
        self.types.intersects(Types::INSTANT.union(Types::SORCERY))
    }

    /// The spell ability of an instant or sorcery, or the targeting ability of an Aura
    /// spell: what the spell targets and does as it resolves. An ability that only a spell
    /// cast with one of its options has ([`CastingOption::ability`]) is not it.
    pub fn spell_ability(&self) -> Option<&'static Ability> {
        let optional = |index: usize| {
            self.casting
                .iter()
                .any(|o| o.ability().is_some_and(|a| a.index() == index))
        };
        (self.abilities.iter().enumerate())
            .find(|&(index, a)| a.kind() == AbilityKind::Spell && !optional(index))
            .map(|(_, a)| a)
    }

    /// The spell ability of this face cast with the options `costs`: its overloaded one when
    /// its overload cost is paid ([`CastingOption::Overload`]), its mutating one when its
    /// mutate cost is ([`CastingOption::Mutate`]), else [`Face::spell_ability`].
    pub fn spell_ability_with(&self, costs: Castings) -> Option<&'static Ability> {
        costs
            .paid(self)
            .find_map(|o| o.ability().map(|ability| &self.abilities[ability.index()]))
            .or_else(|| self.spell_ability())
    }

    /// The parts that are not mana of the cost of casting this face with the options
    /// `costs`, in the order printed: its additional cost, then those of each option paid
    /// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)): "The total cost is
    /// the mana cost or alternative cost (as determined in rule 601.2b), plus all additional
    /// costs and cost increases, and minus all cost reductions."
    pub fn cost_parts(&self, costs: Castings) -> impl Iterator<Item = &'static CostPart> + '_ {
        self.additional_cost
            .iter()
            .chain(costs.paid(self).flat_map(CastingOption::parts))
    }
}

/// What a face-down permanent is
/// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)): "A permanent that enters
/// the battlefield face down also has these characteristics unless otherwise specified by the
/// effect that put it onto the battlefield face down": "a 2/2 face-down creature with no text,
/// no name, no subtypes, and no mana cost." These are its copiable values, applied in layer 1b
/// ([CR 613.2b](https://yawgatog.com/resources/magic-rules/#R6132b)): "Face-down spells and
/// permanents have their characteristics modified as defined in rule 708.2." The only
/// face-down permanents are manifested ones (CR 701.40a), which list the same values.
pub static FACE_DOWN: Face = Face {
    types: Types::CREATURE,
    power: Some(2),
    toughness: Some(2),
    ..Face::EMPTY
};

/// The card a token copying a face-down permanent is a copy of: one face, [`FACE_DOWN`], and no
/// name. Face up or face down is a status
/// ([CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105)), which a copy doesn't copy
/// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "Other effects ...,
/// status, counters, and stickers are not copied." What it does copy are the face-down
/// values ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)): "Any listed
/// characteristics are the copiable values of that object's characteristics." So the copy is
/// a face-up token of this card, which every game table holds: public, it betrays nothing of
/// the card behind the original.
pub static FACE_DOWN_COPY: CardDef = CardDef {
    name: "",
    shape: Shape::Single,
    faces: std::slice::from_ref(&FACE_DOWN),
    tokens: &[],
};

/// A card with one face. Each face starts from [`Face::EMPTY`]:
///
/// ```
/// use beryl_card::prelude::*;
///
/// // "Counter target spell."
/// pub static CARD: CardDef = card(
///     "Counterspell",
///     &[Face::EMPTY
///         .cost(mana("{U}{U}"))
///         .types(Types::INSTANT)
///         .colors(Colors::BLUE)
///         .abilities(&[Ability::spell()
///             .target(&TargetSpec::spell())
///             .effect(&counter(target()))])],
/// );
/// ```
///
/// # Panics
///
/// As the definition compiles, if it says what no game could carry out
/// ([`CardDef::check`]): a build failure in the card's own file. This one adds mana as a
/// spell resolves, which only a mana ability does:
///
/// ```compile_fail
/// use beryl_card::prelude::*;
///
/// pub static CARD: CardDef = card(
///     "Dark Ritual",
///     &[Face::EMPTY
///         .types(Types::INSTANT)
///         .abilities(&[Ability::spell().effect(&add_mana(you(), ManaSet::one(ManaColor::Black)))])],
/// );
/// ```
pub const fn card(name: &'static str, faces: &'static [Face]) -> CardDef {
    card_with_shape(name, Shape::Single, faces)
}

/// A card with the given printed shape and faces.
///
/// # Panics
///
/// As the definition compiles, if the face count does not match the shape or a face says
/// something that no game could carry out ([`CardDef::check`]).
pub const fn card_with_shape(name: &'static str, shape: Shape, faces: &'static [Face]) -> CardDef {
    let card = CardDef {
        name,
        shape,
        faces,
        tokens: &[],
    };
    match card.check() {
        Ok(()) => card,
        Err(malformed) => panic!("{}", malformed.says()),
    }
}

impl CardDef {
    /// The tokens and emblems the card can create ([`CardDef::tokens`]).
    #[must_use]
    pub const fn creating(self, tokens: &'static [&'static CardDef]) -> CardDef {
        CardDef { tokens, ..self }
    }
}

/// What a definition says of a face, starting from [`Face::EMPTY`]. A card file's face starts
/// instead from its `printed::` constant, which its crate's build script generates from the
/// card store with the builders that set printed characteristics, and adds its abilities.
impl Face {
    /// The name of a back face ([`Face::name`]).
    #[must_use]
    pub const fn named(self, name: &'static str) -> Face {
        Face { name, ..self }
    }

    #[must_use]
    pub const fn cost(self, cost: ManaCost) -> Face {
        Face { cost, ..self }
    }

    #[must_use]
    pub const fn supertypes(self, supertypes: Supertypes) -> Face {
        Face { supertypes, ..self }
    }

    #[must_use]
    pub const fn types(self, types: Types) -> Face {
        Face { types, ..self }
    }

    #[must_use]
    pub const fn subtypes(self, subtypes: Subtypes) -> Face {
        Face {
            subtypes: SubtypeWords::new(subtypes),
            ..self
        }
    }

    #[must_use]
    pub const fn colors(self, colors: Colors) -> Face {
        Face { colors, ..self }
    }

    /// A creature's printed power and toughness.
    #[must_use]
    pub const fn power_toughness(self, power: i8, toughness: i8) -> Face {
        Face {
            power: Some(power),
            toughness: Some(toughness),
            ..self
        }
    }

    /// A planeswalker's printed loyalty.
    #[must_use]
    pub const fn loyalty(self, loyalty: u8) -> Face {
        Face {
            loyalty: Some(loyalty),
            ..self
        }
    }

    #[must_use]
    pub const fn keywords(self, keywords: Keywords) -> Face {
        Face { keywords, ..self }
    }

    /// The defense printed on a battle ([CR 210.1]).
    ///
    /// [CR 210.1]: https://yawgatog.com/resources/magic-rules/#R2101
    #[must_use]
    pub const fn defense(self, defense: u8) -> Face {
        Face {
            defense: Some(defense),
            ..self
        }
    }

    /// The card of this one face, named as the face is: `Printed::CardName.abilities(..)
    /// .card()`, where the face starts from a card of one face, which its generated
    /// characteristics name. A card with a different shape is built with [`card_with_shape`].
    ///
    /// # Panics
    ///
    /// As the definition compiles, when the face has no name, or [`card`] refuses it.
    pub const fn card(&'static self) -> CardDef {
        assert!(
            !self.name.is_empty(),
            "a face with no name makes no card: use `card_with_shape` for a multi-face card"
        );
        card(self.name, std::slice::from_ref(self))
    }

    #[must_use]
    pub const fn abilities(self, abilities: &'static [Ability]) -> Face {
        Face { abilities, ..self }
    }

    /// The alternative and additional costs it may be cast with ([`Face::casting`]).
    #[must_use]
    pub const fn casting(self, casting: &'static [CastingOption]) -> Face {
        Face { casting, ..self }
    }

    /// "As an additional cost to cast this spell, [parts]" ([`Face::additional_cost`]).
    #[must_use]
    pub const fn additional_cost(self, additional_cost: &'static [CostPart]) -> Face {
        Face {
            additional_cost,
            ..self
        }
    }

    /// The abilities it grants to another object ([`Face::grants`]).
    #[must_use]
    pub const fn grants(self, grants: &'static [Ability]) -> Face {
        Face { grants, ..self }
    }

    /// The exceptions its copies are made with ([`Face::excepts`]).
    #[must_use]
    pub const fn excepts(self, excepts: &'static [CopyException]) -> Face {
        Face { excepts, ..self }
    }
}

impl Face {
    /// A face with no cost, types or abilities: where a card file's face starts.
    pub const EMPTY: Face = Face {
        name: "",
        cost: ManaCost::NONE,
        supertypes: Supertypes::NONE,
        types: Types::NONE,
        subtypes: SubtypeWords::NONE,
        colors: Colors::NONE,
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        keywords: Keywords::NONE,
        abilities: &[],
        casting: &[],
        use_of_face: FaceUse::Playable,
        additional_cost: &[],
        grants: &[],
        excepts: &[],
    };

    /// One of this face's own abilities or of those it grants.
    pub fn ability(&'static self, ability: FaceAbility) -> &'static Ability {
        match ability {
            FaceAbility::Own(i) => &self.abilities[i.index()],
            FaceAbility::Grant(i) => &self.grants[i.index()],
        }
    }
}

/// An ability printed on a face, as a stack entry names it: one the face has itself, by its
/// index in [`Face::abilities`], or one the face grants to another object, by its index in
/// [`Face::grants`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceAbility {
    Own(AbilityIx),
    Grant(GrantIx),
}

impl FaceAbility {
    /// The face's own ability at `index`, counting from 0.
    pub const fn own(index: u8) -> FaceAbility {
        FaceAbility::Own(AbilityIx::new(index))
    }
}

impl FaceAbility {
    /// A byte that tells the abilities of one face apart, for a record that names an ability
    /// by a number: an own ability's index, or 128 more than a grant's.
    pub const fn number(self) -> u8 {
        match self {
            FaceAbility::Own(i) => i.get(),
            FaceAbility::Grant(i) => 128 | i.get(),
        }
    }
}

/// The face's first own ability: the value a fixed list's unused places hold.
impl Default for FaceAbility {
    fn default() -> FaceAbility {
        FaceAbility::own(0)
    }
}

/// An ability of an object, as a decision to activate it names it: one of its face up's
/// own, by its index in [`Face::abilities`], the `k`th ability other objects grant it,
/// counting those in the order they were granted, or, of a merged permanent, an ability of
/// a card under its top card ([CR 702.140e]): "A mutated permanent has all abilities of each
/// card and token that represents it." That one is named by the card's place among the
/// faces the permanent has every ability of, counting from the top of the pile down, and
/// its index on that card's face. Both counts are the state's, indexes into no list of a
/// definition, so they are plain numbers.
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectAbility {
    Own(AbilityIx),
    Granted(u8),
    Merged(u8, AbilityIx),
}

impl ObjectAbility {
    /// The object's own ability at `index` of its face up, counting from 0.
    pub const fn own(index: u8) -> ObjectAbility {
        ObjectAbility::Own(AbilityIx::new(index))
    }
}

impl Default for ObjectAbility {
    fn default() -> ObjectAbility {
        ObjectAbility::own(0)
    }
}
