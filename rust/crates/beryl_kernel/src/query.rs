//! Queries: what the rules ask about an object, answered once (plan K6; the crate's contract,
//! "Queries").
//!
//! A slot holds the game index of a definition and the face that is up. Neither is an answer:
//! a token's definition is no card, a face-down permanent's says nothing of what it is, a
//! copy of a back face has no mana value, and a controller is a layer result on the
//! battlefield and the owner in a library. So no other crate reads the index: it asks one of
//! these, each named for its rule.
//!
//! [`crate::Observation::card`] is the other door, for a player's view: nothing when the
//! viewer does not know the card.

use beryl_card::{CardDef, Face, FaceIx, Rule, Zone};

use crate::ids::{CardId, CardIx, Seat};
use crate::layout::{Layout, Small};
use crate::object::Object;
use crate::state::State;
use crate::table::{FaceFacts, GameTable};

/// An object's copiable values: what a copy of it would have, and what it has itself before
/// any other effect applies
/// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "The copiable values are
/// the values derived from the text printed on the object (that text being the name, mana
/// cost, color indicator, card type, subtype, supertype, rules text, power, toughness, and/or
/// loyalty), as modified by other copy effects, by its face-down status, and by 'as . . .
/// enters' and 'as . . . is turned face up' abilities that set power and toughness (and may
/// also set additional characteristics). Other effects (including type-changing and
/// text-changing effects), status, counters, and stickers are not copied."
///
/// It is a definition, the face of it that is up, and a copy effect's exception. A token's
/// are those its creation defined
/// ([CR 111.3](https://yawgatog.com/resources/magic-rules/#R1113)), a token copy's those of
/// what it copied. A face-down permanent's are the blank ones, whichever card it is
/// ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)): "Any listed
/// characteristics are the copiable values of that object's characteristics." Nothing here
/// gives its hidden card away: [`Copiable::face`] reads [`beryl_card::FACE_DOWN`], and
/// [`Copiable::record`] names [`beryl_card::FACE_DOWN_COPY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Copiable<L: Layout = Small> {
    card: L::Card,
    /// [`FaceIx::DOWN`] for a face-down permanent's.
    face: FaceIx,
    one_one: bool,
}

/// Which definition an object's values are read from, to tell objects of one from those of
/// another and to put them in an order no slot number decides. It compares and orders, and
/// names no card: read the values through [`Copiable`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionKey<C: CardId = CardIx>(C);

impl<C: CardId> DefinitionKey<C> {
    /// Its row in a table kept beside the game's definitions, one row for each
    /// ([`GameTable::len`]), such as an agent's values.
    #[inline]
    pub fn index(self) -> usize {
        self.0.index()
    }
}

impl<L: Layout> Copiable<L> {
    /// The copiable values of the card or token at a game index, its front face up: what a
    /// card has wherever it is not a permanent or a spell with another face up
    /// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a)), and what a record
    /// names that kept only the index, such as last-known information.
    #[inline]
    pub const fn of_card(card: L::Card) -> Copiable<L> {
        Copiable {
            card,
            face: FaceIx::FRONT,
            one_one: false,
        }
    }

    #[inline]
    pub(crate) fn of_object(o: &Object<L>) -> Copiable<L> {
        Copiable {
            card: o.card,
            face: o.face,
            one_one: o.is_one_one(),
        }
    }

    /// These values with power and toughness 1/1, as a copy effect's exception sets them
    /// ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)): "Some copy effects
    /// modify a characteristic as part of the copying process. The final set of values for
    /// that characteristic becomes part of the copiable values of the copy."
    #[inline]
    #[must_use]
    pub const fn except_one_one(self) -> Copiable<L> {
        Copiable {
            one_one: true,
            ..self
        }
    }

    /// These values with the face at `face` up instead: the spell a card becomes cast with
    /// that face ([CR 712.11b](https://yawgatog.com/resources/magic-rules/#R71211b)), or the
    /// permanent it becomes entering with it up or down
    /// ([CR 712.14a](https://yawgatog.com/resources/magic-rules/#R71214a), CR 708.2).
    #[inline]
    #[must_use]
    pub const fn turned(self, face: FaceIx) -> Copiable<L> {
        Copiable { face, ..self }
    }

    /// Whether power and toughness are 1/1 by a copy effect's exception (CR 707.9b).
    #[inline]
    pub(crate) const fn is_one_one(self) -> bool {
        self.one_one
    }

    /// The characteristics of the face that is up, or a face-down permanent's blank ones (CR
    /// 708.2). Its power and toughness are without the exception: read [`Copiable::power`]
    /// and [`Copiable::toughness`].
    #[inline]
    pub fn face(self, t: &GameTable) -> &'static Face {
        t.def(self.card).face(self.face)
    }

    /// What the scans over every permanent read of that face, found once per game.
    #[inline]
    pub fn facts(self, t: &GameTable) -> FaceFacts {
        t.facts(self.card, self.face)
    }

    /// Whether the face that is up has a static ability with `rule`.
    #[inline]
    pub fn has_rule(self, t: &GameTable, rule: Rule) -> bool {
        t.may_have_rule(self.card, rule) && self.facts(t).has_rule(rule)
    }

    /// The event kinds the triggered abilities of any face of the definition listen to: more
    /// than the face that is up may have, so a mask to skip an object by and nothing else. A
    /// face-down permanent has no abilities
    /// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)), so none.
    #[inline]
    pub fn may_listen(self, t: &GameTable) -> u16 {
        if self.face == FaceIx::DOWN {
            0
        } else {
            t.listens(self.card)
        }
    }

    /// The name of the face that is up
    /// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a),
    /// [CR 712.8e](https://yawgatog.com/resources/magic-rules/#R7128e)); none, the empty
    /// string, for a face-down permanent (CR 708.2a).
    #[inline]
    pub fn name(self, t: &GameTable) -> &'static str {
        t.def(self.card).face_name(self.face)
    }

    /// Power, 0 where the face has none
    /// ([CR 208.1](https://yawgatog.com/resources/magic-rules/#R2081)): "A creature card has
    /// two numbers separated by a slash printed in its lower right corner. The first number
    /// is its power".
    #[inline]
    pub fn power(self, t: &GameTable) -> i8 {
        if self.one_one {
            1
        } else {
            self.face(t).power.unwrap_or(0)
        }
    }

    /// Toughness, 0 where the face has none (CR 208.1): "the second is its toughness".
    #[inline]
    pub fn toughness(self, t: &GameTable) -> i8 {
        if self.one_one {
            1
        } else {
            self.face(t).toughness.unwrap_or(0)
        }
    }

    /// The game index and face a record keeps for these values: a stack entry's, a pending
    /// trigger's, last-known information's, and what a token copy is made of. A face-down
    /// permanent's are the one face of [`beryl_card::FACE_DOWN_COPY`], never its hidden card:
    /// being face down is a status, which a copy does not have
    /// ([CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105), CR 707.2), and the
    /// blank values are what it copies (CR 708.2).
    #[inline]
    pub fn record(self, t: &GameTable) -> (L::Card, FaceIx) {
        if self.face == FaceIx::DOWN {
            (t.face_down_copy::<L>(), FaceIx::FRONT)
        } else {
            (self.card, self.face)
        }
    }

    /// Which definition these are read from, to compare and to order by.
    #[inline]
    pub const fn key(self) -> DefinitionKey<L::Card> {
        DefinitionKey(self.card)
    }
}

impl<L: Layout> State<L> {
    /// The card that represents the object in `slot`: nothing for a token
    /// ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)): "a marker used to
    /// represent any permanent that isn't represented by a card", which stays one wherever
    /// it goes ([CR 111.7](https://yawgatog.com/resources/magic-rules/#R1117)) and is never a
    /// card ([CR 111.6](https://yawgatog.com/resources/magic-rules/#R1116)), whatever it
    /// copies. Ask this for what only a card has: the other faces it could be cast or played
    /// with, what turning a face-down permanent face up shows, a place in a decklist.
    ///
    /// It is the rules' answer, with nothing masked: a face-down permanent's is its hidden
    /// card. A player's view asks [`crate::Observation::card`].
    #[inline]
    pub fn card_behind(&self, slot: L::Slot) -> Option<L::Card> {
        let o = self.object(slot);
        (!o.is_token()).then_some(o.card)
    }

    /// The definition of the card that represents the object in `slot`, with every face it
    /// may be cast or played with
    /// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a)); nothing for a
    /// token ([`State::card_behind`]).
    #[inline]
    pub fn card_def(&self, t: &GameTable, slot: L::Slot) -> Option<&'static CardDef> {
        self.card_behind(slot).map(|card| t.def(card))
    }

    /// The name of the card or token definition behind `slot`, whichever face is up or down:
    /// what orders a batch of objects so that no seed decides it (the contract's
    /// `in_game_order`), and what a tool, a save or a test calls an object. It is not the
    /// object's name by the rules, which is [`Copiable::name`], and it is the rules' own
    /// view: a face-down permanent's is its hidden card's.
    #[inline]
    pub fn definition_name(&self, t: &GameTable, slot: L::Slot) -> &'static str {
        t.def(self.object(slot).card).name
    }

    /// The copiable values of the object in `slot` (CR 707.2): what it is before any other
    /// effect, and what a copy of it would be.
    #[inline]
    pub fn copiable(&self, slot: L::Slot) -> Copiable<L> {
        Copiable::of_object(self.object(slot))
    }

    /// The mana value of the object in `slot`
    /// ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)): "The mana value of an
    /// object is a number equal to the total amount of mana in its mana cost, regardless of
    /// color." A spell's counts its X
    /// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as
    /// the number chosen for it while the object is on the stack."
    ///
    /// [CR 712.8e](https://yawgatog.com/resources/magic-rules/#R7128e): a transforming
    /// double-faced permanent with its back face up "has only the characteristics of its back
    /// face. However, its mana value is calculated using the mana cost of its front face. If a
    /// permanent is copying the back face of a nonmodal double-faced permanent (even if the
    /// object representing that copy is itself a double-faced permanent), the mana value of
    /// that permanent is 0." A token with a double-faced card's faces is always such a copy
    /// ([CR 707.8a](https://yawgatog.com/resources/magic-rules/#R7078a)). A face-down
    /// permanent has no mana cost (CR 708.2a), so 0.
    #[inline]
    pub fn mana_value(&self, t: &GameTable, slot: L::Slot) -> u8 {
        let o = self.object(slot);
        if o.zone == Zone::Stack
            && let Some(entry) = self
                .stack()
                .iter()
                .find(|entry| entry.spell().is_some_and(|spell| spell.slot == slot))
        {
            let cost = t.def(entry.card).face(entry.face).cost;
            return cost
                .mana_value()
                .saturating_add(cost.x.saturating_mul(entry.x));
        }
        let up = t.def(o.card).face(o.face);
        if !up.transformed {
            up.cost.mana_value()
        } else if o.is_token() {
            0
        } else {
            t.def(o.card).front().cost.mana_value()
        }
    }

    /// Who controls the object in `slot`: a permanent's controller after continuous effects
    /// ([CR 110.2](https://yawgatog.com/resources/magic-rules/#R1102)), a spell's, who is the
    /// player who cast it
    /// ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "that player
    /// becomes its controller"), and anywhere else its owner
    /// ([CR 108.4a](https://yawgatog.com/resources/magic-rules/#R1084a)): "If anything asks
    /// for the controller of a card that doesn't have one (because it's not a permanent or
    /// spell), use its owner instead."
    ///
    /// One read off the battlefield: the zone move gives an object back to its owner, and
    /// only becoming a spell or entering the battlefield names another controller
    /// ([`State::become_spell`], [`State::arrive`]).
    ///
    /// # Panics
    ///
    /// In debug builds, for a permanent while the layer results are stale.
    #[inline]
    pub fn controller(&self, slot: L::Slot) -> Seat {
        let o = self.object(slot);
        if o.zone == Zone::Battlefield {
            self.derived(slot).controller
        } else {
            o.base_controller
        }
    }

    /// The power of the object in `slot`: a permanent's after continuous effects
    /// ([CR 613.4](https://yawgatog.com/resources/magic-rules/#R6134)), and elsewhere its
    /// copiable value ([`Copiable::power`]).
    #[inline]
    pub fn power(&self, t: &GameTable, slot: L::Slot) -> i8 {
        let o = self.object(slot);
        if o.zone == Zone::Battlefield {
            self.derived(slot).power
        } else {
            Copiable::of_object(o).power(t)
        }
    }

    /// [`State::power`] for toughness.
    #[inline]
    pub fn toughness(&self, t: &GameTable, slot: L::Slot) -> i8 {
        let o = self.object(slot);
        if o.zone == Zone::Battlefield {
            self.derived(slot).toughness
        } else {
            Copiable::of_object(o).toughness(t)
        }
    }
}
