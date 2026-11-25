//! A card and its faces: the characteristics printed on each, and how a stack entry or a
//! decision names one of a face's abilities.

mod face;
pub use face::{FACE_DOWN, Face, FaceAbility, FaceUse, ObjectAbility};

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
    /// One card of a meld pair ([CR 713.1]), with one front face: `with` names the other
    /// card of the pair and `into` the combined back face they meld into, each a separate
    /// card definition.
    ///
    /// [CR 713.1]: https://yawgatog.com/resources/magic-rules/#R7131
    Meld {
        with: &'static str,
        into: &'static str,
    },
    /// The combined back face of a meld pair, with one face: only the characteristics of the
    /// permanent two melded cards become ([CR 713.1b]): "The back faces of a meld pair are used
    /// only to determine the characteristics of the melded permanent that pair becomes on the
    /// battlefield." It is never a card in a deck, and its face can't be cast or played.
    ///
    /// [CR 713.1b]: https://yawgatog.com/resources/magic-rules/#R7131b
    Melded,
}

impl Shape {
    /// The number of printed faces or sets of alternative characteristics in the definition.
    pub const fn face_count(self) -> usize {
        match self {
            Self::Single | Self::Meld { .. } | Self::Melded => 1,
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
