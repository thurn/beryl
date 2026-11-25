//! The definition an object has the values of, read without its copy exceptions: what the
//! readers ask that only the definition's face answers.
//!
//! Implements: none. Gives the layers the printed inputs and copy exceptions.
//!
//! [`State::copiable`] gives an object's whole copiable values, the exceptions of a copy
//! effect on it among them ([`crate::copiable::additions`]), which it reads behind a flag of the object.
//! Most readers want only what the card table keeps for each definition and face: the face's
//! printed characteristics and abilities, the facts the scans read, the masks of what it may
//! listen to or replace, its name, its key. Exceptions are read separately when the layers
//! apply them (see [CR 707.9b]): "Some copy effects modify a characteristic as
//! part of the copying process. The final set of values for that characteristic becomes part
//! of the copiable values of the copy." So those readers ask [`State::definition`], which is
//! the object's fields and nothing else, and pay nothing for copies: asking
//! [`State::copiable`] instead cost the rollout benchmark 3% more instructions.
//!
//! [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b

use beryl_card::{AbilityIx, AbilityKind, Colors, Face, FaceIx, Supertypes};

use crate::copiable::defines_pt;
use crate::event::{Listens, Replaces};
use crate::fixed::Full;
use crate::layout::{Layout, Small};
use crate::object::{AbilityMask, Derived, Flag, GrantRef};
use crate::query::{Copiable, DefinitionKey};
use crate::rule_changes::Rules;
use crate::state::State;
use crate::static_ability::Inputs;
use crate::subtypes::GameSubtypes;
use crate::table::{FaceFacts, GameTable};

/// The definition and face an object has the values of, without the exceptions of a copy
/// effect on it ([`State::definition`]). Its answers are [`Copiable`]'s of the same name,
/// which none of the exceptions changes; what an exception changes (power and toughness) it
/// does not answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Definition<L: Layout = Small>(Copiable<L>);

impl<L: Layout> Definition<L> {
    /// The card at a game index with its front face up ([`Copiable::of_card`]).
    #[inline]
    pub const fn of_card(card: L::Card) -> Definition<L> {
        Definition(Copiable::of_card(card))
    }

    /// The card at a game index with its face `face` up.
    #[inline]
    pub const fn of_face(card: L::Card, face: FaceIx) -> Definition<L> {
        Definition(Copiable::of_face(card, face))
    }

    /// [`Copiable::face`].
    #[inline]
    pub fn face(self, t: &GameTable) -> &'static Face {
        self.0.face(t)
    }

    /// [`Copiable::subtypes`].
    #[inline]
    pub fn subtypes(self, t: &GameTable) -> GameSubtypes {
        self.0.subtypes(t)
    }

    /// [`Copiable::facts`].
    #[inline]
    pub fn facts(self, t: &GameTable) -> FaceFacts {
        self.0.facts(t)
    }

    /// [`Copiable::rules`].
    #[inline]
    pub fn rules(self, t: &GameTable) -> Rules {
        self.0.rules(t)
    }

    /// [`Copiable::player_rules`].
    #[inline]
    pub fn player_rules(self, t: &GameTable) -> Rules {
        self.0.player_rules(t)
    }

    /// [`Copiable::may_listen`].
    #[inline]
    pub fn may_listen(self, t: &GameTable) -> Listens {
        self.0.may_listen(t)
    }

    /// [`Copiable::layer_inputs`].
    #[inline]
    pub fn layer_inputs(self, t: &GameTable) -> Inputs {
        self.0.layer_inputs(t)
    }

    /// [`Copiable::may_replace`].
    #[inline]
    pub fn may_replace(self, t: &GameTable) -> Replaces {
        self.0.may_replace(t)
    }

    /// [`Copiable::name`].
    #[inline]
    pub fn name(self, t: &GameTable) -> &'static str {
        self.0.name(t)
    }

    /// [`Copiable::record`]: the game index and face a stack entry or a pending trigger keeps
    /// of its source.
    #[inline]
    pub fn record(self, t: &GameTable) -> (L::Card, FaceIx) {
        self.0.record(t)
    }

    /// [`Copiable::key`].
    #[inline]
    pub const fn key(self) -> DefinitionKey<L::Card> {
        self.0.key()
    }
}

impl<L: Layout> State<L> {
    /// The definition and face the object in `slot` has the values of, read from its fields
    /// alone: for the readers of what the card table keeps for each definition, which no copy
    /// exception changes. [`State::copiable`] is the object's whole copiable values.
    #[inline]
    pub fn definition(&self, slot: L::Slot) -> Definition<L> {
        Definition(Copiable::of_object(self.object(slot)))
    }

    /// The power and toughness of the copiable values of the object in `slot`, 0 where its
    /// face has none ([`Copiable::power`], [`Copiable::toughness`]): what layer 1 gives a
    /// permanent before the other layers apply. The exceptions of a copy effect are read only
    /// behind its [`Flag::Copy`].
    #[inline]
    pub fn copiable_pt(&self, t: &GameTable, slot: L::Slot) -> (i8, i8) {
        let o = self.object(slot);
        if o.flags.has(Flag::Copy) {
            return self.excepted_pt(t, slot);
        }
        let values = Copiable::of_object(o);
        (values.power(t), values.toughness(t))
    }

    /// What layer 1 gives the permanent in `slot`, which has a copy effect on it
    /// ([`Flag::Copy`]): `printed`, the results its definition's face gives, with the power,
    /// toughness, colors and subtypes of its copiable values, which hold its exceptions
    /// ([`State::copiable_pt`], [`State::copiable_colors`], [`State::copy_subtypes`], [`State::copy_supertypes`]), and the
    /// activated abilities they give ([`State::copy_grants`]), with the keywords of the faces
    /// of a merged permanent's other cards ([`Copiable::merged_faces`]). The mask of
    /// absent abilities starts with any CDA omitted by a power and toughness exception
    /// ([`Copiable::omits_pt_definitions`]).
    ///
    /// # Errors
    ///
    /// [`Full`] if an omitted ability's index does not fit the layout's ability mask.
    #[cold]
    #[inline(never)]
    pub fn copy_layer_one(
        &self,
        t: &GameTable,
        slot: L::Slot,
        printed: &mut Derived<L>,
    ) -> Result<(), Full> {
        let values = self.copiable(slot);
        let pt = values.excepted_pt(t);
        // see CR 702.140e: "A mutated permanent has all abilities of each card and token that
        // represents it", its keywords among them.
        let keywords = values
            .merged_faces(t)
            .fold(printed.keywords, |k, (card, face)| {
                k.union(t.def(card).face(face).keywords)
            });
        printed.colors = values.colors(t);
        printed.subtypes = values.excepted_subtypes(t);
        printed.supertypes = values.excepted_supertypes(t);
        printed.granted = self.copy_grants(t, slot);
        printed.keywords = keywords;
        if let Some((power, toughness)) = pt {
            printed.power = power;
            printed.toughness = toughness;
            for (ability, index) in values.face(t).abilities.iter().zip(0u8..) {
                if defines_pt(ability) {
                    printed.lost = printed.lost.with(AbilityIx::new(index)).ok_or(Full)?;
                }
            }
        }
        Ok(())
    }

    /// The colors of the copiable values of the object in `slot` ([`Copiable::colors`]): what
    /// an object off the battlefield has before the effects that reach it. The exceptions of a
    /// copy effect are read only behind its [`Flag::Copy`].
    #[inline]
    pub fn copiable_colors(&self, t: &GameTable, slot: L::Slot) -> Colors {
        let o = self.object(slot);
        if o.flags.has(Flag::Copy) {
            return self.excepted_colors(t, slot);
        }
        Copiable::of_object(o).colors(t)
    }

    /// [`State::copiable_colors`] of an object with a copy effect on it.
    #[cold]
    #[inline(never)]
    fn excepted_colors(&self, t: &GameTable, slot: L::Slot) -> Colors {
        self.copiable(slot).colors(t)
    }

    /// The subtypes layer 1 gives the object in `slot`, a copy ([`Flag::Copy`]), with those
    /// its exceptions add, such as "it's an Illusion in addition to its other types"
    /// ([`Copiable::excepted_subtypes`]). Any other object's are its definition's.
    #[cold]
    #[inline(never)]
    pub fn copy_subtypes(&self, t: &GameTable, slot: L::Slot) -> GameSubtypes {
        self.copiable(slot).excepted_subtypes(t)
    }

    /// The supertypes layer 1 gives the object in `slot`, a copy ([`Flag::Copy`]), without those
    /// its exceptions take away, such as "except it isn't legendary"
    /// ([`Copiable::excepted_supertypes`]).
    #[cold]
    #[inline(never)]
    pub fn copy_supertypes(&self, t: &GameTable, slot: L::Slot) -> Supertypes {
        self.copiable(slot).excepted_supertypes(t)
    }

    /// The abilities layer 1 gives the object in `slot`, a copy ([`Flag::Copy`]), that its
    /// exceptions give and that are activated, such as Thespian's Stage's "except it has
    /// this ability" (see [CR 707.9a]): "This ability becomes part of the copiable values for the
    /// copy, along with any other abilities that were copied." They are kept with the
    /// abilities granted to it ([`crate::object::Derived::granted`]), which every reader of
    /// its abilities asks ([`State::abilities`]), as they would be had it been granted them
    /// first of all; an ability-removing effect removes them as it does the face's. A given
    /// triggered ability is matched apart ([`Copiable::given_abilities`]).
    ///
    /// [CR 707.9a]: https://yawgatog.com/resources/magic-rules/#R7079a
    #[cold]
    #[inline(never)]
    pub fn copy_grants(&self, t: &GameTable, slot: L::Slot) -> [Option<GrantRef<L>>; 4] {
        let mut granted = [None; 4];
        let given = self.copiable(slot).given_abilities(t);
        let activated = given.filter(|&(card, face, grant)| {
            // A granted ability is printed on its card's front face (§9).
            debug_assert_eq!(face, FaceIx::FRONT);
            matches!(
                t.def(card).front().grants[grant.index()].kind(),
                AbilityKind::Activated | AbilityKind::Mana
            )
        });
        for ((card, _, grant), place) in activated.zip(&mut granted) {
            *place = Some(GrantRef { card, grant });
        }
        granted
    }

    /// [`State::copiable_pt`] of an object with a copy effect on it.
    #[cold]
    #[inline(never)]
    fn excepted_pt(&self, t: &GameTable, slot: L::Slot) -> (i8, i8) {
        let values = self.copiable(slot);
        (values.power(t), values.toughness(t))
    }
}
