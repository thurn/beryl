//! Text-changing effects ([CR 612.1]): "Some continuous effects change an object’s text.
//! This can apply to any words or symbols printed on that object, but generally affects only
//! that object’s rules text (which appears in its text box) and/or the text that appears in
//! its type line."
//!
//! Each is an entry of the Effects store on one object, a spell or a permanent, whose payload
//! is one [`TextChange`]. [`State::words_of`] composes the entries on an object, in the order
//! they began ([CR 613.7]: "An effect with an earlier timestamp is applied before an effect
//! with a later timestamp"), into the [`Words`] every reader of a word of its text takes. With
//! no text change in force anywhere, as nearly every game goes, that is one AND.
//!
//! Text changes are applied in layer 3 ([CR 613.1c]): "Layer 3: Text-changing effects are
//! applied." Nothing reads a word before layer 3 does, so the words are applied wherever
//! they are read rather than as a pass of the layers: layer 1's copiable values hold words
//! as printed, and layer 4 on reads them through [`State::words_of`].
//!
//! A change on a permanent spell goes on with the permanent it becomes ([CR 400.7a]):
//! "Effects from spells, activated abilities, and triggered abilities that change the
//! characteristics or controller of a permanent spell on the stack continue to apply to the
//! permanent that spell becomes."
//!
//! An ability on the stack keeps the words its source had as it went there ([CR 113.7a]):
//! "Once activated or triggered, an ability exists on the stack independently of its
//! source." The state keeps up to four such sets for the abilities on the stack, which a
//! byte of each entry names ([`StackEntry::words`]), and empties them with the stack.
//!
//! [CR 113.7a]: https://yawgatog.com/resources/magic-rules/#R1137a
//! [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
//! [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
//! [CR 613.1c]: https://yawgatog.com/resources/magic-rules/#R6131c
//! [CR 400.7a]: https://yawgatog.com/resources/magic-rules/#R4007a

use beryl_card::Words;

use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::stack::{AS_PRINTED, READ_NOW, StackEntry};
use crate::state::State;

use super::{Affects, Continuous, ContinuousEffect, Kinds, Payload};

/// The most text changes a spell carries onto the battlefield in a [`crate::Small`] game.
/// One past it stalls the game into [`crate::Large`].
const CARRIED: usize = 4;

/// The text changes a permanent spell carries onto the battlefield, read before the move ends
/// what the stores hold about it.
pub(crate) type Carried<L> = <L as Layout>::Queue<ContinuousEffect<L>, CARRIED>;

impl<L: Layout> State<L> {
    /// The text changes on the object in `slot` ([CR 612.1]): what the color words and
    /// subtypes its printed text and type line name read as. Every reader of a word an
    /// object's text names takes these ([`beryl_card::words`]). One AND while no text change
    /// is in force.
    ///
    /// The changes compose in the order they began. A change that would need a fifth subtype
    /// entry ([`beryl_card::words::SUBTYPE_CHANGES`]) reads as nothing: the five basic land
    /// types are all the cards replace, so it would take every one of them replaced on one
    /// object.
    ///
    /// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
    #[inline]
    pub fn words_of(&self, slot: L::Slot) -> Words {
        if self.effects.kinds.0 & Kinds::TEXT == 0 {
            return Words::NONE;
        }
        self.changed_words(slot)
    }

    /// [`State::words_of`] while some text change is in force. Every entry about a slot is
    /// about its current object (the store's invariant), so the slot names the object.
    #[cold]
    #[inline(never)]
    fn changed_words(&self, slot: L::Slot) -> Words {
        self.effects
            .entries
            .iter()
            .fold(Words::NONE, |words, e| match (e.affected, e.payload) {
                (Affects::Object { slot: on, .. }, Payload::Text(change)) if on == slot => {
                    words.changing(change).unwrap_or(words)
                }
                _ => words,
            })
    }

    /// The text changes on the spell in `slot`, which it carries onto the battlefield
    /// (CR 400.7a). Empty while no text change is in force.
    ///
    /// # Errors
    ///
    /// When the spell has more than [`CARRIED`] of them.
    #[cold]
    #[inline(never)]
    pub(crate) fn carried_text(&self, slot: L::Slot) -> Result<Carried<L>, Full> {
        let mut carried = Carried::<L>::default();
        for &e in self.effects.of(Kinds::TEXT) {
            if matches!((e.affected, e.payload), (Affects::Object { slot: on, .. }, Payload::Text(_)) if on == slot)
            {
                carried.push(e)?;
            }
        }
        Ok(carried)
    }

    /// Puts the text changes a spell carried back on the permanent now in `slot`, each with
    /// the timestamp it began with.
    #[cold]
    #[inline(never)]
    pub(crate) fn carry_text(&mut self, slot: L::Slot, carried: &Carried<L>) -> Result<(), Full> {
        let object = self.object_ref(slot);
        for e in carried.iter() {
            if let Payload::Text(change) = e.payload {
                let effect = Continuous::Text(object, change);
                self.push_effect(e.source, effect, e.duration, e.timestamp)?;
            }
        }
        Ok(())
    }

    /// The words a spell or ability on the stack reads its text through: a spell's own, as
    /// they are now, and an ability's source's as the ability went on the stack (CR 113.7a).
    #[inline]
    pub fn entry_words(&self, entry: &StackEntry<L>) -> Words {
        match entry.spell() {
            Some(spell) => self.words_of(spell.slot),
            None => self.words_by(entry.source().map(|r| r.slot), entry.words),
        }
    }

    /// The words named by `words`, a [`StackEntry::words`], of the ability whose source is in
    /// `source`: as printed, a captured set, or for [`READ_NOW`] its source's now. One AND while
    /// no text change is in force, whatever `words` is.
    #[inline]
    pub fn words_by(&self, source: Option<L::Slot>, words: u8) -> Words {
        if self.effects.kinds.0 & Kinds::TEXT == 0 && self.captured_words.is_empty() {
            return Words::NONE;
        }
        match words {
            AS_PRINTED => Words::NONE,
            READ_NOW => source.map_or(Words::NONE, |slot| self.changed_words(slot)),
            n => self.captured_words.as_slice()[usize::from(n - 1)],
        }
    }

    /// Whether a text change is in force on any object: what the layers test once before
    /// reading any permanent's type line through its words.
    #[inline]
    pub fn changes_text(&self) -> bool {
        self.effects.kinds.0 & Kinds::TEXT != 0
    }

    /// Notes in `entry`, an ability about to go on the stack, the words of its source now.
    /// Nothing while no text change is in force: the entry's words are as printed already
    /// ([`AS_PRINTED`]), and a change that begins after it is on the stack changes nothing of
    /// it.
    ///
    /// # Errors
    ///
    /// When the abilities on the stack already keep [`crate::state::CAPTURED_WORDS`] other sets.
    #[inline]
    pub(crate) fn capture_words(&mut self, entry: &mut StackEntry<L>) -> Result<(), Full> {
        if self.effects.kinds.0 & Kinds::TEXT == 0 {
            return Ok(());
        }
        let Some(source) = entry.source() else {
            return Ok(());
        };
        entry.words = self.captured(self.words_of(source.slot))?;
        Ok(())
    }

    /// The number by which a stack entry names `words`, kept from here.
    #[cold]
    #[inline(never)]
    fn captured(&mut self, words: Words) -> Result<u8, Full> {
        if words.is_none() {
            return Ok(AS_PRINTED);
        }
        let kept = &mut self.captured_words;
        let at = if let Some(at) = kept.as_slice().iter().position(|&w| w == words) {
            at
        } else {
            kept.push(words)?;
            kept.len() - 1
        };
        // A layout that keeps more than 254 sets would name one as AS_PRINTED; none does.
        u8::try_from(at + 1).map_err(|_| Full)
    }

    /// Whether a text change is in force or the stack's abilities keep a set of them: the
    /// one test [`State::put_on_stack`] makes in a game without a text change.
    #[inline]
    pub(crate) fn keeps_words(&self) -> bool {
        self.effects.kinds.0 & Kinds::TEXT != 0 || !self.captured_words.is_empty()
    }

    /// Settles the kept sets of text changes and notes in `entry` its source's words, as
    /// `entry` goes on the stack ([`State::keeps_words`]). Out of line: only a game with a
    /// text change comes here.
    ///
    /// # Errors
    ///
    /// When the abilities on the stack already keep [`crate::state::CAPTURED_WORDS`] other sets.
    #[cold]
    #[inline(never)]
    pub(crate) fn words_on_stack(&mut self, entry: &mut StackEntry<L>) -> Result<(), Full> {
        self.settle_captured_words();
        self.capture_words(entry)
    }

    /// Empties the sets of text changes the stack's abilities kept, once the stack is
    /// empty: no entry names one.
    #[inline]
    pub fn settle_captured_words(&mut self) {
        if self.stack.is_empty() && !self.captured_words.is_empty() {
            self.captured_words.clear();
        }
    }
}

/// [`crate::table::land_mana`] when `words` changes a subtype word. Out of line: only a game with a
/// text change comes here.
#[cold]
#[inline(never)]
pub(crate) fn changed_land_mana(face: &beryl_card::Face, words: &Words, written: u8) -> u8 {
    let printed = beryl_card::BasicLandTypes::of(face.subtypes.read(&Words::NONE));
    let types = beryl_card::BasicLandTypes::of(face.subtypes.read(words));
    // A written ability of a land type the face no longer has went with the type.
    types.mana().intrinsic_mana() & !(written & !printed.minus(types).mana().intrinsic_mana())
}
