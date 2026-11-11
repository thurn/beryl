//! A card and its faces: the characteristics printed on each, and how a stack entry or a
//! decision names one of a face's abilities.

use crate::ability::{Ability, AbilityKind};
use crate::characteristics::{Colors, Keywords, Subtypes, Supertypes, Types};
use crate::cost::{CastingOption, Castings};
use crate::mana::ManaCost;

/// One card: its name and its faces.
#[derive(Debug)]
pub struct CardDef {
    pub name: &'static str,
    /// One face, or two for double-faced, modal double-faced, split and adventure cards.
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

    /// Each of its faces' indices, front first.
    pub fn face_ixs(&self) -> impl Iterator<Item = FaceIx> {
        (0..self.faces.len()).map(|i| FaceIx(u8::try_from(i).expect("a few faces")))
    }

    /// The name of the face at `face`, the name a spell or permanent with that face up has
    /// ([CR 712.8f](https://yawgatog.com/resources/magic-rules/#R7128f)): "While a modal
    /// double-faced spell is on the stack or a modal double-faced permanent is on the
    /// battlefield, it has only the characteristics of the face that's up." The same holds of
    /// a nonmodal one ([CR 712.8e](https://yawgatog.com/resources/magic-rules/#R7128e)):
    /// "While a nonmodal double-faced permanent has its back face up, it has only the
    /// characteristics of its back face." A face-down permanent has no name (CR 708.2a): the
    /// empty string.
    pub fn face_name(&self, face: FaceIx) -> &'static str {
        if face == FaceIx::FRONT {
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
    /// The name of a back face. A front face's name is its card's, [`CardDef::name`], which
    /// is the card's name wherever it is not a permanent or spell with another face up
    /// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a)): "While a
    /// double-faced card is outside the game or in a zone other than the battlefield or
    /// stack, it has only the characteristics of its front face."
    pub name: &'static str,
    pub cost: ManaCost,
    pub supertypes: Supertypes,
    pub types: Types,
    pub subtypes: Subtypes,
    pub colors: Colors,
    pub power: Option<i8>,
    pub toughness: Option<i8>,
    pub loyalty: Option<u8>,
    pub keywords: Keywords,
    pub abilities: &'static [Ability],
    /// The alternative and additional costs it may be cast with, in the order its text lists
    /// them. A stack entry records the ones paid as a [`Castings`] set over this list.
    pub casting: &'static [CastingOption],
    /// The back face of a transforming double-faced card, which is never cast: "A
    /// double-faced spell is cast with its front face up by default"
    /// ([CR 712.11](https://yawgatog.com/resources/magic-rules/#R71211)). Only a modal
    /// double-faced card lets its caster choose the face
    /// ([CR 712.11b](https://yawgatog.com/resources/magic-rules/#R71211b)).
    pub transformed: bool,
    /// "As an additional cost to cast this spell, exile N cards from your graveyard"
    /// (Abhorrent Oculus): N, or 0 for no such cost. It is paid with the rest of the total
    /// cost ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)).
    pub exile_from_graveyard: u8,
    /// Abilities this face grants to another object rather than has itself, such as the crew
    /// ability of Swift Reconfiguration's "Enchanted permanent is a Vehicle artifact with crew
    /// 5". [`Modification::Grant`] names one by its index here.
    pub grants: &'static [Ability],
}

impl Face {
    /// Whether this face is an instant or a sorcery: its spell resolves by following its
    /// instructions, not by becoming a permanent
    /// ([CR 608.2](https://yawgatog.com/resources/magic-rules/#R6082),
    /// [CR 608.3](https://yawgatog.com/resources/magic-rules/#R6083)).
    pub fn is_instant_or_sorcery(&self) -> bool {
        self.types.intersects(Types::INSTANT.union(Types::SORCERY))
    }

    /// The spell ability of an instant or sorcery, or the targeting ability of an Aura
    /// spell: what the spell targets and does as it resolves.
    pub fn spell_ability(&self) -> Option<&'static Ability> {
        self.abilities.iter().find(|a| a.kind == AbilityKind::Spell)
    }

    /// The spell ability of this face cast with the options `costs`: its overloaded one when
    /// its overload cost is paid ([`CastingOption::Overload`]), else
    /// [`Face::spell_ability`].
    pub fn spell_ability_with(&self, costs: Castings) -> Option<&'static Ability> {
        costs
            .paid(self)
            .find_map(|o| match o {
                CastingOption::Overload { ability, .. } => {
                    Some(&self.abilities[usize::from(ability)])
                }
                _ => None,
            })
            .or_else(|| self.spell_ability())
    }

    /// The colors of the card that paying `costs` exiles from its caster's hand, such as
    /// Subtlety's evoke cost, "Exile a blue card from your hand"; none for no such card.
    pub fn exiles_with(&self, costs: Castings) -> Colors {
        costs.paid(self).fold(Colors::NONE, |colors, o| match o {
            CastingOption::Evoke { exile, .. } => colors.union(exile),
            _ => colors,
        })
    }
}

/// What a face-down permanent is
/// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)): "A permanent that enters
/// the battlefield face down also has these characteristics unless otherwise specified by the
/// effect that put it onto the battlefield face down": "a 2/2 face-down creature with no text,
/// no name, no subtypes, and no mana cost." These are its copiable values, applied in layer 1b
/// ([CR 613.2b](https://yawgatog.com/resources/magic-rules/#R6132b)): "Face-down spells and
/// permanents have their characteristics modified as defined in rule 708.2." The cube's only
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
    faces: std::slice::from_ref(&FACE_DOWN),
    tokens: &[],
};

impl Face {
    /// A face with no cost, types or abilities, for `..Face::EMPTY` in card files.
    pub const EMPTY: Face = Face {
        name: "",
        cost: ManaCost::NONE,
        supertypes: Supertypes::NONE,
        types: Types::NONE,
        subtypes: Subtypes::NONE,
        colors: Colors::NONE,
        power: None,
        toughness: None,
        loyalty: None,
        keywords: Keywords::NONE,
        abilities: &[],
        casting: &[],
        transformed: false,
        exile_from_graveyard: 0,
        grants: &[],
    };

    /// One of this face's own abilities or of those it grants.
    pub fn ability(&'static self, ability: FaceAbility) -> &'static Ability {
        match ability {
            FaceAbility::Own(i) => &self.abilities[usize::from(i)],
            FaceAbility::Grant(i) => &self.grants[usize::from(i)],
        }
    }
}

/// An ability printed on a face, as a stack entry names it: one the face has itself, by its
/// index in [`Face::abilities`], or one the face grants to another object, by its index in
/// [`Face::grants`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceAbility {
    Own(u8),
    Grant(u8),
}

/// An ability of a permanent, as a decision to activate it names it: one of its face
/// up's own, by its index in [`Face::abilities`], or the `k`th ability other objects grant
/// it, by its index among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectAbility {
    Own(u8),
    Granted(u8),
}
