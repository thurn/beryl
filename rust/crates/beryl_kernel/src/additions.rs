//! Additions: the copy exceptions an object's copiable values carry beside the face they are
//! read from ([`crate::query::Copiable`]).
//!
//! A copy keeps its exceptions ([CR 707.9b]): "Some copy effects modify a characteristic as
//! part of the copying process. The final set of values for that characteristic becomes part of the copiable values of the
//! copy." So an exception is copied with the values it changed, and a copy of a copy has the
//! exceptions of both. An addition names an exception by the card whose face lists it
//! ([`beryl_card::Face::excepts`]) and its place in that list, never by its values, so a
//! [`Small`](crate::Small) game keeps one in two bytes and the values stay the vocabulary's.
//!
//! [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b

use std::fmt::Debug;
use std::hash::Hash;

use beryl_card::{CopyChange, ExceptIx, FaceIx};

use crate::fixed::Full;
use crate::ids::CardId;
use crate::table::GameTable;

/// One copy exception among an object's copiable values: the [`ExceptIx`] of a face of the
/// card at a game index. Only a front or a back face lists exceptions, which is all the byte
/// beside the card has room to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Addition<C: CardId> {
    card: C,
    /// The exception's index, with [`Addition::BACK`] set for one of the back face.
    entry: u8,
}

impl<C: CardId> Addition<C> {
    /// The bit of [`Addition::entry`] that says the exception is the back face's.
    const BACK: u8 = 1 << 7;

    /// The exception at `except` of the face `face` of the card at `card`.
    ///
    /// # Panics
    ///
    /// If `face` is neither the front nor the back face, or `except` is past the 128 the byte
    /// numbers: a face lists a handful.
    #[must_use]
    pub fn new(card: C, face: FaceIx, except: ExceptIx) -> Addition<C> {
        assert!(
            except.get() < Addition::<C>::BACK,
            "an exception index past 127"
        );
        let back = match face {
            FaceIx::FRONT => 0,
            FaceIx::BACK => Addition::<C>::BACK,
            _ => panic!("a copy exception of a face neither front nor back"),
        };
        Addition {
            card,
            entry: back | except.get(),
        }
    }

    /// The card at whose face the exception is listed.
    #[inline]
    pub fn card(self) -> C {
        self.card
    }

    /// The face that lists it.
    #[inline]
    pub fn face(self) -> FaceIx {
        if self.entry & Addition::<C>::BACK == 0 {
            FaceIx::FRONT
        } else {
            FaceIx::BACK
        }
    }

    /// Its index in that face's [`beryl_card::Face::excepts`].
    #[inline]
    pub fn except(self) -> ExceptIx {
        ExceptIx::new(self.entry & !Addition::<C>::BACK)
    }

    /// The changes it makes, in the order they apply.
    #[inline]
    pub fn changes(self, t: &GameTable) -> &'static [CopyChange] {
        t.def(self.card).face(self.face()).excepts[self.except().index()].0
    }

    /// The addition with its card numbered by another layout.
    #[inline]
    pub(crate) fn relayout<D: CardId>(self) -> Addition<D> {
        Addition {
            card: crate::relayout::card(self.card),
            entry: self.entry,
        }
    }
}

/// The additions copiable values hold in a layout ([`crate::Layout::Additions`]): a fixed
/// array, filled from the start, so that [`crate::query::Copiable`] stays `Copy` in every
/// layout. [`Small`](crate::Small) holds two, which is room for a copy of a copy that each
/// made with an exception; a third is [`Full`] and stalls the game into
/// [`Large`](crate::Large).
pub trait AdditionList<C: CardId>:
    Copy
    + Debug
    + Default
    + Eq
    + Hash
    + Send
    + Sync
    + AsRef<[Option<Addition<C>>]>
    + AsMut<[Option<Addition<C>>]>
{
    /// The list with no addition.
    const NONE: Self;

    /// Whether it holds none: an exact copy, or no copy at all.
    #[inline]
    fn is_empty(&self) -> bool {
        self.as_ref().first().is_none_or(Option::is_none)
    }

    /// The additions in the order they were made.
    #[inline]
    fn iter(&self) -> impl Iterator<Item = Addition<C>> + '_ {
        self.as_ref().iter().map_while(|&addition| addition)
    }

    /// Adds `addition` after the others. [`Full`] if the list has no room for it.
    #[inline]
    fn push(&mut self, addition: Addition<C>) -> Result<(), Full> {
        let free = self.as_mut().iter_mut().find(|a| a.is_none()).ok_or(Full)?;
        *free = Some(addition);
        Ok(())
    }

    /// How many it holds.
    #[inline]
    fn len(&self) -> usize {
        self.iter().count()
    }
}

impl<C: CardId, const N: usize> AdditionList<C> for [Option<Addition<C>>; N]
where
    [Option<Addition<C>>; N]: Default,
{
    const NONE: Self = [None; N];
}
