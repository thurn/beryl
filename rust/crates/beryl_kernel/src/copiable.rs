//! Copiable values: a definition, the face of it that is up, and the copy exceptions that
//! made them ([`crate::additions`]). [`crate::query`] asks them of an object.

use beryl_card::{Colors, CopyChange, Face, FaceIx};

use crate::additions::{Addition, AdditionList};
use crate::event::{Listens, Replaces};
use crate::fit::{Misfit, room};
use crate::fixed::Full;
use crate::ids::{CardId, CardIx};
use crate::layout::{Layout, Small};
use crate::object::Object;
use crate::rule_changes::Rules;
use crate::static_ability::Inputs;
use crate::subtypes::GameSubtypes;
use crate::table::{FaceFacts, GameTable};

/// An object's copiable values: what a copy of it would have, and what it has itself before any
/// other effect applies ([CR 707.2]): "The copiable values are the values derived from the text
/// printed on the object (that text being name, mana cost, color indicator, card type, subtype,
/// supertype, rules text, power, toughness, loyalty, and/or defense), as modified by other copy
/// effects, by its face-down status, and by 'as . . . enters' and 'as . . . is turned face up'
/// abilities that set power and toughness (and may also set additional characteristics). Other
/// effects (including type-changing and text-changing effects), status, counters, and stickers are
/// not copied."
///
/// It is a definition, the face of it that is up, and the exceptions of the copy effects that
/// made it ([`Addition`]), which a copy of it takes with the rest ([CR 707.9b]): "The final set
/// of values for that characteristic becomes part of the copiable values of the copy." A token's are
/// those its creation defined ([CR 111.3]), a token copy's those of what it copied. A face-down
/// permanent's are the blank ones, whichever card it is ([CR 708.2]): "Any listed characteristics
/// are the copiable values of that object's characteristics." Nothing here gives its hidden card
/// away: [`Copiable::face`] reads [`beryl_card::FACE_DOWN`], and [`Copiable::record`] names
/// [`beryl_card::FACE_DOWN_COPY`].
///
/// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
/// [CR 111.3]: https://yawgatog.com/resources/magic-rules/#R1113
/// [CR 708.2]: https://yawgatog.com/resources/magic-rules/#R7082
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Copiable<L: Layout = Small> {
    card: L::Card,
    /// [`FaceIx::DOWN`] for a face-down permanent's.
    face: FaceIx,
    additions: L::Additions,
}

/// Which definition an object's values are read from, to tell objects of one from those of
/// another and to put them in an order no slot number decides. It compares and orders, and
/// names no card: read the values through [`Copiable`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionKey<C: CardId = CardIx>(C);

impl<C: CardId> DefinitionKey<C> {
    /// Its row in a table kept beside the game's definitions, one row for each
    /// ([`crate::GameTable::len`]), such as an agent's values.
    #[inline]
    pub fn index(self) -> usize {
        self.0.index()
    }
}

impl<L: Layout> crate::relayout::Relayout for Copiable<L> {
    type In<M: Layout> = Copiable<M>;

    fn relayout<M: Layout>(&self) -> Copiable<M> {
        Copiable {
            card: crate::relayout::card(self.card),
            face: self.face,
            additions: relayout_additions::<L, M>(self.additions),
        }
    }
}

/// The additions in layout `M`, the first of them where `M` holds fewer: what
/// [`Copiable::fits`] refuses before a state moves to a smaller layout.
fn relayout_additions<L: Layout, M: Layout>(additions: L::Additions) -> M::Additions {
    let mut moved = M::Additions::default();
    for (to, from) in moved.as_mut().iter_mut().zip(additions.iter()) {
        *to = Some(from.relayout());
    }
    moved
}

impl<L: Layout> Copiable<L> {
    /// The copiable values of the card or token at a game index, its front face up: what a card has
    /// wherever it is not a permanent or a spell with another face up ([CR 712.6a]), and what a
    /// record names that kept only the index, such as last-known information.
    ///
    /// [CR 712.6a]: https://yawgatog.com/resources/magic-rules/#R7126a
    #[inline]
    pub const fn of_card(card: L::Card) -> Copiable<L> {
        Copiable {
            card,
            face: FaceIx::FRONT,
            additions: L::Additions::NONE,
        }
    }

    /// The values of an object with no copy effect on it: its definition and the face that is
    /// up. One with a copy effect also has the effect's additions ([`crate::State::copiable`]).
    #[inline]
    pub(crate) fn of_object(o: &Object<L>) -> Copiable<L> {
        Copiable {
            card: o.card,
            face: o.face,
            additions: L::Additions::NONE,
        }
    }

    /// These values with the exception `addition` after those they have, as a copy effect
    /// makes them ([CR 707.9b]): "Some copy effects modify a characteristic as part of the
    /// copying process. The final set of values for that characteristic becomes part of the
    /// copiable values of the copy." [`Full`] past what the layout holds
    /// ([`Layout::Additions`]).
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    #[inline]
    pub fn with_addition(mut self, addition: Addition<L::Card>) -> Result<Copiable<L>, Full> {
        self.additions.push(addition)?;
        Ok(self)
    }

    /// The exceptions these values carry, in the order the copy effects made them.
    #[inline]
    pub fn additions(&self) -> impl Iterator<Item = Addition<L::Card>> + '_ {
        self.additions.iter()
    }

    /// Whether they carry an exception: what a token made of them keeps as a copy effect
    /// ([`crate::State::make_token`]).
    #[inline]
    pub(crate) fn is_excepted(&self) -> bool {
        !self.additions.is_empty()
    }

    /// The values with the additions of `copied`, which a copy effect on an object keeps,
    /// read with the definition and face the object's fields hold.
    #[inline]
    pub(crate) fn with_additions_of(self, copied: Copiable<L>) -> Copiable<L> {
        Copiable {
            additions: copied.additions,
            ..self
        }
    }

    /// Whether a state of layout `M` holds these values: whether `M` has room for their
    /// additions.
    pub(crate) fn fits<M: Layout>(&self) -> Result<(), Misfit> {
        room(
            "a copy's exceptions",
            self.additions.len(),
            M::Additions::default().as_ref().len(),
        )
    }

    /// These values with the face at `face` up instead: the spell a card becomes cast with that
    /// face ([CR 712.8b]), or the permanent it becomes entering with it up or down ([CR 712.11a],
    /// CR 708.2).
    ///
    /// [CR 712.8b]: https://yawgatog.com/resources/magic-rules/#R7128b
    /// [CR 712.11a]: https://yawgatog.com/resources/magic-rules/#R71211a
    #[inline]
    #[must_use]
    pub const fn turned(self, face: FaceIx) -> Copiable<L> {
        Copiable { face, ..self }
    }

    /// The characteristics of the face that is up, or a face-down permanent's blank ones (CR
    /// 708.2). Its power and toughness are without the exception: read [`Copiable::power`]
    /// and [`Copiable::toughness`].
    #[inline]
    pub fn face(self, t: &GameTable) -> &'static Face {
        t.def(self.card).face(self.face)
    }

    /// The printed subtypes of the face that is up, by the game's numbers.
    #[inline]
    pub fn subtypes(self, t: &GameTable) -> GameSubtypes {
        t.face_subtypes(self.card, self.face)
    }

    /// What the scans over every permanent read of that face, found once per game.
    #[inline]
    pub fn facts(self, t: &GameTable) -> FaceFacts {
        t.facts(self.card, self.face)
    }

    /// The rule changes the static abilities of the face that is up make, by their numbers
    /// in the game. For a card no face of which makes one, a load and a test.
    #[inline]
    pub fn rules(self, t: &GameTable) -> Rules {
        if t.may_have_rules(self.card).is_empty() {
            return Rules::NONE;
        }
        t.face_rules(self.card, self.face)
    }

    /// The rule changes the static abilities of the face that is up make for players
    /// ([`crate::GameTable::player_rules`]). For a card no face of which makes one, a load and an
    /// AND. The layers ask it only of a battlefield with such a card on it
    /// ([`crate::event::Replaces::PLAYER_RULE_CHANGE`]).
    #[inline]
    pub fn player_rules(self, t: &GameTable) -> Rules {
        let of_players = t.player_rules();
        if t.may_have_rules(self.card).common(of_players).is_empty() {
            return Rules::NONE;
        }
        t.face_rules(self.card, self.face).common(of_players)
    }

    /// The event kinds the triggered abilities of any face of the definition listen to: more than
    /// the face that is up may have, so a mask to skip an object by and nothing else. A face-down
    /// permanent has no abilities ([CR 708.2a]), so none.
    ///
    /// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
    #[inline]
    pub fn may_listen(self, t: &GameTable) -> Listens {
        if self.face == FaceIx::DOWN {
            Listens::NONE
        } else {
            t.listens(self.card)
        }
    }

    /// What the static abilities of its face read as the layers apply that no layer writes
    /// ([`Inputs`]). A face-down permanent has no abilities ([CR 708.2a]), so none.
    ///
    /// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
    #[inline]
    pub fn layer_inputs(self, t: &GameTable) -> Inputs {
        if self.face == FaceIx::DOWN {
            Inputs::NONE
        } else {
            t.facts(self.card, self.face).inputs
        }
    }

    /// The kinds of event the static abilities of any face of the definition replace, and
    /// whether one changes a rule for players: like [`Copiable::may_listen`], a mask to skip
    /// an object by. A face-down permanent has no abilities ([CR 708.2a]), so none.
    ///
    /// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
    #[inline]
    pub fn may_replace(self, t: &GameTable) -> Replaces {
        if self.face == FaceIx::DOWN {
            Replaces::NONE
        } else {
            t.replaces(self.card)
        }
    }

    /// The name of the face that is up ([CR 712.6a], [CR 712.6d]); none, the empty string, for a
    /// face-down permanent (CR 708.2a).
    ///
    /// [CR 712.6a]: https://yawgatog.com/resources/magic-rules/#R7126a
    /// [CR 712.6d]: https://yawgatog.com/resources/magic-rules/#R7126d
    #[inline]
    pub fn name(self, t: &GameTable) -> &'static str {
        t.def(self.card).face_name(self.face)
    }

    /// Power, 0 where the face has none ([CR 208.1]): "A creature card has two numbers separated by
    /// a slash printed in its lower right corner. The first number is its power".
    ///
    /// [CR 208.1]: https://yawgatog.com/resources/magic-rules/#R2081
    #[inline]
    pub fn power(self, t: &GameTable) -> i8 {
        match self.excepted_pt(t) {
            Some((power, _)) => power,
            None => self.face(t).power.unwrap_or(0),
        }
    }

    /// Colors ([CR 105.2]): "An object can be one or more of the five colors, or it can be no
    /// color at all." The face's, or those the last exception that sets colors sets (CR
    /// 707.9b), as Fork's copy is red whatever its mana cost says.
    ///
    /// [CR 105.2]: https://yawgatog.com/resources/magic-rules/#R1052
    #[inline]
    pub fn colors(self, t: &GameTable) -> Colors {
        if self.additions.is_empty() {
            self.face(t).colors
        } else {
            self.colors_of_additions(t)
        }
    }

    /// [`Copiable::colors`] of values with exceptions.
    #[cold]
    #[inline(never)]
    fn colors_of_additions(self, t: &GameTable) -> Colors {
        let changes = self
            .additions
            .iter()
            .flat_map(|addition| addition.changes(t));
        changes.fold(self.face(t).colors, |colors, change| match *change {
            CopyChange::Colors(set) => set,
            CopyChange::PowerToughness(..) => colors,
        })
    }

    /// Toughness, 0 where the face has none (CR 208.1): "the second is its toughness".
    #[inline]
    pub fn toughness(self, t: &GameTable) -> i8 {
        match self.excepted_pt(t) {
            Some((_, toughness)) => toughness,
            None => self.face(t).toughness.unwrap_or(0),
        }
    }

    /// The power and toughness the last exception that sets them sets, if one does (CR
    /// 707.9b): "The final set of values for that characteristic becomes part of the copiable
    /// values of the copy." For values with no exception, one test.
    #[inline]
    fn excepted_pt(self, t: &GameTable) -> Option<(i8, i8)> {
        if self.additions.is_empty() {
            None
        } else {
            self.pt_of_additions(t)
        }
    }

    /// [`Copiable::excepted_pt`] of values with exceptions.
    #[cold]
    #[inline(never)]
    fn pt_of_additions(self, t: &GameTable) -> Option<(i8, i8)> {
        self.additions
            .iter()
            .flat_map(|addition| addition.changes(t))
            .fold(None, |pt, change| match *change {
                CopyChange::PowerToughness(power, toughness) => Some((power, toughness)),
                CopyChange::Colors(_) => pt,
            })
    }

    /// The game index and face a record keeps for these values: a stack entry's, a pending
    /// trigger's, last-known information's, and what a token copy is made of. A face-down
    /// permanent's are the one face of [`beryl_card::FACE_DOWN_COPY`], never its hidden card: being
    /// face down is a status, which a copy does not have ([CR 110.5], CR 707.2), and the blank
    /// values are what it copies (CR 708.2).
    ///
    /// [CR 110.5]: https://yawgatog.com/resources/magic-rules/#R1105
    #[inline]
    pub fn record(self, t: &GameTable) -> (L::Card, FaceIx) {
        if self.face == FaceIx::DOWN {
            (t.face_down_copy::<L>(), FaceIx::FRONT)
        } else {
            (self.card, self.face)
        }
    }

    /// These values as a record keeps them ([`Copiable::record`]), with their additions: what
    /// a token copy is made of and its copy effect keeps.
    #[inline]
    #[must_use]
    pub fn recorded(self, t: &GameTable) -> Copiable<L> {
        let (card, face) = self.record(t);
        Copiable { card, face, ..self }
    }

    /// The game index of the definition these are read from: the kernel's own, for the copy
    /// effect that keeps an object's definition equal to these ([`crate::effects::copy`]).
    #[inline]
    pub(crate) fn card(self) -> L::Card {
        self.card
    }

    /// Which definition these are read from, to compare and to order by.
    #[inline]
    pub const fn key(self) -> DefinitionKey<L::Card> {
        DefinitionKey(self.card)
    }
}
