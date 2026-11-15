//! Copiable values: a definition, the face of it that is up, and the copy exceptions that
//! made them ([`crate::additions`]). [`crate::query`] asks them of an object.

use beryl_card::{
    Ability, AbilityKind, Colors, CopyChange, Face, FaceIx, GrantIx, Supertypes, Words,
};

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
    /// The colors an exception's color words read as through the text changes of the spell or
    /// ability that made the copy, where those changed them ([`Copiable::with_addition_read`]).
    read: ReadColors,
}

/// The colors one of a copy's exceptions sets ([`CopyChange::Colors`]), read through the text
/// changes of the spell or ability that made the copy as it made it ([CR 612.1]), as Fork whose
/// "red" reads "blue" makes a blue copy. The read is made once, as the copy is made
/// ([CR 608.2h]): "If an effect requires information from the game (such as the number of
/// creatures on the battlefield), the answer is determined only once, when the effect is
/// applied." Kept only where the text changes changed the colors: every other exception's
/// color words read as printed. Only the last exception that sets colors counts (CR 707.9b),
/// so one place is enough.
///
/// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct ReadColors {
    /// One past the place among the additions of the exception they were read for; 0 for
    /// none.
    after: u8,
    colors: Colors,
}

impl ReadColors {
    /// No colors read through text changes.
    const NONE: ReadColors = ReadColors {
        after: 0,
        colors: Colors::NONE,
    };

    /// The colors read for the exception at `place` among the additions, if any were.
    #[inline]
    fn at(self, place: usize) -> Option<Colors> {
        (usize::from(self.after) == place + 1).then_some(self.colors)
    }
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
            read: self.read,
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

/// The colors the exception `addition` sets as read through the text changes `words`, where
/// they differ from its words as printed ([`ReadColors`]). Only the last change that sets
/// colors counts (CR 707.9b).
#[cold]
#[inline(never)]
fn read_colors<C: CardId>(addition: Addition<C>, t: &GameTable, words: &Words) -> Option<Colors> {
    let mut read = None;
    for change in addition.changes(t) {
        if let CopyChange::Colors(set) = *change {
            let colors = set.read(words);
            if colors != set.read(&Words::NONE) {
                read = Some(colors);
            }
        }
    }
    read
}

/// Whether an object with two instances of `ability` can do nothing that it could not do
/// with one. By [CR 113.2c], "If an object has multiple instances of the same ability, each
/// instance functions independently. This may or may not produce more effects than a single
/// instance". Two instances of a triggered ability trigger twice, and two of a static ability
/// may apply twice, so they are not interchangeable. An activated ability (mana ability
/// included) only offers its controller a choice ([CR 113.3b]): "A player may activate such an
/// ability whenever they have priority." Each instance offers the same activation, for the
/// same cost, at the same times, so a second instance adds no new choice. That stops being
/// true for an ability with a limit on how often each instance may be activated, such as
/// "Activate only once each turn": the vocabulary has no such limit, and one added must make
/// this return `false` for the abilities that have it.
///
/// [CR 113.2c]: https://yawgatog.com/resources/magic-rules/#R1132c
/// [CR 113.3b]: https://yawgatog.com/resources/magic-rules/#R1133b
fn interchangeable(ability: &Ability) -> bool {
    matches!(ability.kind(), AbilityKind::Activated | AbilityKind::Mana)
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
            read: ReadColors::NONE,
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
            read: ReadColors::NONE,
        }
    }

    /// These values with the exception `addition` after those they have, as a copy effect
    /// makes them ([CR 707.9b]): "Some copy effects modify a characteristic as part of the
    /// copying process. The final set of values for that characteristic becomes part of the
    /// copiable values of the copy." [`Full`] past what the layout holds
    /// ([`Layout::Additions`]). The exception's words read as printed: a copy made by a spell
    /// or ability whose text may be changed is made with [`Copiable::with_addition_read`].
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    #[inline]
    pub fn with_addition(mut self, addition: Addition<L::Card>) -> Result<Copiable<L>, Full> {
        self.additions.push(addition)?;
        Ok(self)
    }

    /// [`Copiable::with_addition`] for a copy made by a spell or ability whose text has the
    /// changes `words`: the color words of the exception read through them ([`ReadColors`]),
    /// as they are when the copy is made. One test while the text is as printed.
    ///
    /// An exception these values already carry, and that would change nothing made again, is
    /// not added a second time ([`Copiable::repeats`]). So Thespian's Stage, whose copy
    /// exception gives it the ability that makes the copy, can copy itself, or two of them
    /// each other, any number of times, and its values keep one exception per Stage.
    ///
    /// # Errors
    ///
    /// [`Full`] past what the layout holds ([`Layout::Additions`]).
    #[inline]
    pub fn with_addition_read(
        self,
        addition: Addition<L::Card>,
        t: &GameTable,
        words: &Words,
    ) -> Result<Copiable<L>, Full> {
        let read = if words.is_none() {
            None
        } else {
            read_colors(addition, t, words)
        };
        if self.repeats(addition, read, t) {
            return Ok(self);
        }
        let place = self.additions.len();
        let mut values = self.with_addition(addition)?;
        if let Some(colors) = read {
            // A layout holds far fewer than 255 additions.
            let after = u8::try_from(place + 1).unwrap_or(u8::MAX);
            values.read = ReadColors { after, colors };
        }
        Ok(values)
    }

    /// Whether these values already carry `addition`, with the colors `read` for its color
    /// words, such that carrying it twice would make them no different. Each kind of change
    /// in it must allow that:
    ///
    /// - [`CopyChange::Has`] gives an ability, which the values then have twice
    ///   ([CR 113.2c]): "If an object has multiple instances of the same ability, each instance
    ///   functions independently." That is only the same as one instance for an ability whose
    ///   instances can't be told apart ([`interchangeable`]),
    ///   wherever the first one is among the exceptions.
    /// - [`CopyChange::AddSubtype`] adds a subtype the values already have.
    /// - [`CopyChange::WithoutSupertypes`] removes supertypes the values already lack, and no
    ///   exception adds one back.
    /// - [`CopyChange::PowerToughness`] and [`CopyChange::Colors`] set a value, and the last
    ///   exception to set it wins ([CR 707.9b]): "The final set of values for that
    ///   characteristic becomes part of the copiable values of the copy." Setting it again to
    ///   the same value only changes nothing if the earlier exception is the last one, so no
    ///   exception between them set it to something else. Colors must also have been read the
    ///   same way.
    ///
    /// [CR 113.2c]: https://yawgatog.com/resources/magic-rules/#R1132c
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    fn repeats(&self, addition: Addition<L::Card>, read: Option<Colors>, t: &GameTable) -> bool {
        let Some(place) = self.additions().position(|a| a == addition) else {
            return false;
        };
        let last = place + 1 == self.additions.len();
        let grants = t.def(addition.card()).face(addition.face()).grants;
        addition.changes(t).iter().all(|change| match *change {
            CopyChange::Has(grant) => interchangeable(&grants[grant.index()]),
            CopyChange::AddSubtype(_) | CopyChange::WithoutSupertypes(_) => true,
            CopyChange::PowerToughness(..) => last,
            CopyChange::Colors(_) => last && self.read.at(place) == read,
        })
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
            read: copied.read,
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

    /// The printed subtypes of the face that is up, by the game's numbers. Those a copy's
    /// exceptions add are [`Copiable::excepted_subtypes`].
    #[inline]
    pub fn subtypes(self, t: &GameTable) -> GameSubtypes {
        t.face_subtypes(self.card, self.face)
    }

    /// The subtypes of the face that is up with those the exceptions add (CR 707.9b): what
    /// layer 1 gives a copy ([`crate::State::copy_subtypes`]).
    #[cold]
    #[inline(never)]
    pub fn excepted_subtypes(self, t: &GameTable) -> GameSubtypes {
        self.with_excepted_subtypes(t, self.subtypes(t))
    }

    /// `printed`, the subtypes of the face that is up as some reader takes them, with those
    /// the exceptions add (CR 707.9b). The layers pass the face's subtypes read through the
    /// permanent's text changes: those change the type line the copy took, and the subtype an
    /// exception adds stays.
    #[cold]
    #[inline(never)]
    pub fn with_excepted_subtypes(self, t: &GameTable, printed: GameSubtypes) -> GameSubtypes {
        (self.additions.iter())
            .flat_map(|addition| addition.changes(t))
            .fold(printed, |subtypes, change| match *change {
                CopyChange::AddSubtype(subtype) => subtypes.with(t.subtypes(), subtype),
                CopyChange::PowerToughness(..)
                | CopyChange::Colors(_)
                | CopyChange::WithoutSupertypes(_)
                | CopyChange::Has(_) => subtypes,
            })
    }

    /// The supertypes of the face that is up without those the exceptions take away
    /// ([CR 707.9b]): "Some copy effects modify a characteristic as part of the copying
    /// process. The final set of values for that characteristic becomes part of the copiable
    /// values of the copy." What layer 1 gives a copy ([`crate::State::copy_supertypes`]).
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    #[cold]
    #[inline(never)]
    pub fn excepted_supertypes(self, t: &GameTable) -> Supertypes {
        (self.additions.iter())
            .flat_map(|addition| addition.changes(t))
            .fold(
                self.face(t).supertypes,
                |supertypes, change| match *change {
                    CopyChange::WithoutSupertypes(removed) => supertypes.minus(removed),
                    CopyChange::PowerToughness(..)
                    | CopyChange::Colors(_)
                    | CopyChange::AddSubtype(_)
                    | CopyChange::Has(_) => supertypes,
                },
            )
    }

    /// The abilities the exceptions of these values give, each with the card and face that
    /// list it in their grants ([CR 707.9a]): "This ability becomes part of the copiable values
    /// for the copy, along with any other abilities that were copied."
    ///
    /// [CR 707.9a]: https://yawgatog.com/resources/magic-rules/#R7079a
    pub fn given_abilities(
        self,
        t: &GameTable,
    ) -> impl Iterator<Item = (L::Card, FaceIx, GrantIx)> + '_ {
        let additions = self.additions;
        let count = additions.len();
        (0..count)
            .filter_map(move |i| additions.as_ref()[i])
            .flat_map(move |addition| {
                addition
                    .changes(t)
                    .iter()
                    .filter_map(move |change| match *change {
                        CopyChange::Has(grant) => Some((addition.card(), addition.face(), grant)),
                        CopyChange::PowerToughness(..)
                        | CopyChange::Colors(_)
                        | CopyChange::WithoutSupertypes(_)
                        | CopyChange::AddSubtype(_) => None,
                    })
            })
    }

    /// The faces whose every ability these values have besides those of the face that is up:
    /// those of the cards under a merged permanent, from the top of the pile down
    /// ([`Addition::every_ability`]). None for values with no addition, after one test.
    #[inline]
    pub fn merged_faces(self, t: &GameTable) -> impl Iterator<Item = (L::Card, FaceIx)> {
        let _ = t;
        let additions = self.additions;
        let count = additions.len();
        (0..count)
            .filter_map(move |i| additions.as_ref()[i])
            .filter(|addition| addition.is_every_ability())
            .map(|addition| (addition.card(), addition.face()))
    }

    /// The card and face at `place` among [`Copiable::merged_faces`], counting from 0.
    #[inline]
    pub fn merged_face(self, place: u8) -> Option<(L::Card, FaceIx)> {
        let additions = self.additions;
        (0..additions.len())
            .filter_map(|i| additions.as_ref()[i])
            .filter(|addition| addition.is_every_ability())
            .nth(usize::from(place))
            .map(|addition| (addition.card(), addition.face()))
    }

    /// The values a merged permanent has once the card `card`, cast with its face `face` up,
    /// merges with a permanent whose values these are ([CR 731.2a]): "A merged permanent has
    /// only the characteristics of its topmost component, unless otherwise specified by the
    /// effect that caused them to merge. This is a copiable effect whose timestamp is the time
    /// the objects merged." Mutate specifies its abilities ([CR 702.140e]): "A mutated
    /// permanent has all abilities of each card and token that represents it." Under (`top` false), these values stay as they are, a copy
    /// effect's exceptions among them, and the card's face is added below the others. Over,
    /// that card's face is the values, with every ability of the face these values had up and
    /// of each face under it added. A face-down permanent's face has no abilities
    /// ([CR 708.2a]), so it adds none. [`Full`] past what the layout holds.
    ///
    /// Over a copy, the copy effect's own exceptions are not kept: the values add every ability
    /// of the face it copied, and not an ability its exception gave it.
    ///
    /// [CR 731.2a]: https://yawgatog.com/resources/magic-rules/#R7312a
    /// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
    /// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
    pub fn merged_with(self, card: L::Card, face: FaceIx, top: bool) -> Result<Copiable<L>, Full> {
        if !top {
            return self.with_addition(Addition::every_ability(card, face));
        }
        let mut values = Copiable::of_card(card).turned(face);
        if self.face != FaceIx::DOWN {
            values = values.with_addition(Addition::every_ability(self.card, self.face))?;
        }
        for addition in self.additions().filter(|a| a.is_every_ability()) {
            values = values.with_addition(addition)?;
        }
        Ok(values)
    }

    /// What the scans over every permanent read of that face, found once per game.
    #[inline]
    pub fn facts(self, t: &GameTable) -> FaceFacts {
        t.facts(self.card, self.face)
    }

    /// The rule changes the static abilities of the face that is up make, by their numbers
    /// in the game. For a card no face of which makes one, a load and a test.
    // Always inline: the compiler leaves it out of line in `beryl_rules`' check of a
    // permanent's own rule changes, which the rollout benchmark makes 70,000 times.
    #[allow(clippy::inline_always)]
    #[inline(always)]
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

    /// [`Copiable::colors`] of values with exceptions. An exception's color words read as
    /// printed, or as the copier's text changes read them when it made the copy
    /// ([`ReadColors`]).
    #[cold]
    #[inline(never)]
    fn colors_of_additions(self, t: &GameTable) -> Colors {
        let changes = (self.additions.iter().enumerate())
            .flat_map(|(place, addition)| addition.changes(t).iter().map(move |c| (place, c)));
        changes.fold(
            self.face(t).colors,
            |colors, (place, change)| match *change {
                CopyChange::Colors(set) => {
                    (self.read.at(place)).unwrap_or_else(|| set.read(&Words::NONE))
                }
                CopyChange::PowerToughness(..)
                | CopyChange::AddSubtype(_)
                | CopyChange::WithoutSupertypes(_)
                | CopyChange::Has(_) => colors,
            },
        )
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
                CopyChange::Colors(_)
                | CopyChange::AddSubtype(_)
                | CopyChange::WithoutSupertypes(_)
                | CopyChange::Has(_) => pt,
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

    /// The face these are read from, which a permanent that becomes a copy has up
    /// ([`State::become_copy`](crate::State::become_copy)).
    #[inline]
    pub(crate) fn face_up(self) -> FaceIx {
        self.face
    }

    /// Which definition these are read from, to compare and to order by.
    #[inline]
    pub const fn key(self) -> DefinitionKey<L::Card> {
        DefinitionKey(self.card)
    }
}
