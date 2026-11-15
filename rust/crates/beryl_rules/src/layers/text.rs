//! A permanent's own type line and keywords read through its text changes, applied to its
//! layer 1 values before layer 4 reads them ([CR 613.1c]): "Layer 3: Text-changing effects
//! are applied." Its static abilities are read through the same words wherever the layers
//! read them ([`super::reader`]).
//!
//! [CR 613.1c]: https://yawgatog.com/resources/magic-rules/#R6131c

use crate::state::AbilityMask as _;
use beryl_kernel::derive::Rederive;

use crate::card::{AbilityIx, BasicLandTypes, Face, Types, Words};
use crate::fixed::Full;
use crate::layout::Layout;
use crate::table::{GameTable, is_simple};

use super::granted::GRANTED;
use super::{Apply, apply, position};

/// What the text changes of a permanent changed of its layer 1 values.
#[derive(Clone, Copy, Default)]
pub(super) struct Changed {
    /// A land's basic land types.
    pub(super) land_types: bool,
    /// An ability of its face is gone: a mana ability its old land type gave it.
    pub(super) removed: bool,
}

impl Changed {
    /// Both changes together.
    pub(super) fn and(self, other: Changed) -> Changed {
        Changed {
            land_types: self.land_types || other.land_types,
            removed: self.removed || other.removed,
        }
    }
}

/// Who reads the part `a` of a static ability as its layer applies: the controller of its
/// permanent as the layers so far left it, and the words of the permanent's text it is read
/// through. None for an effect of a resolved spell or ability, settled as it began. CR 612.3:
/// "any abilities that are granted to an object can't be modified by text-changing effects
/// that affect that object", so a granted ability reads as its granter prints it.
pub(super) fn reader<L: Layout>(s: &Rederive<'_, L>, a: &Apply<L>) -> Option<apply::Reader> {
    a.from.map(|(from, _)| {
        let i = position::<L>(from);
        apply::Reader {
            you: s.rows()[i].controller,
            words: if a.order & GRANTED == 0 {
                s.words_of(s.battlefield()[i])
            } else {
                Words::NONE
            },
        }
    })
}

/// Reads the type line and landwalk keywords of the permanent at battlefield index `i`, whose
/// face is `face`, through its words ([CR 612.1]): "This can apply to any words or symbols
/// printed on that object, but generally affects only that object’s rules text (which appears
/// in its text box) and/or the text that appears in its type line." Nothing while no word of
/// its type line reads otherwise, as in every game without a text change.
///
/// A land whose basic land types change has the intrinsic mana ability of each new one
/// ([CR 305.6]): "An object with the land card type and a basic land type has the intrinsic
/// ability “{T}: Add [mana symbol],” even if the text box doesn't actually contain that
/// text". A mana ability its face writes out for a basic land type it no longer has is that
/// type's intrinsic ability, which went with the type, so it is gone: a Forest whose
/// "Forest" reads as "Island" taps for {U}, not {G}.
///
/// # Errors
///
/// When the face has more abilities than a mask of lost abilities holds in the layout.
///
/// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
/// [`type_line`] for every permanent, with what they changed together and whether a mask of
/// lost abilities ran out of room. Out of line: only a game with a text change comes here.
#[cold]
#[inline(never)]
pub(super) fn type_lines<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
) -> (Changed, Result<(), Full>) {
    let mut text = Changed::default();
    let mut listed = Ok(());
    for i in 0..s.battlefield().len() {
        let face = s.definition(s.battlefield()[i]).face(t);
        match type_line(s, t, i, face) {
            Ok(changed) => text = text.and(changed),
            Err(full) => listed = Err(full),
        }
    }
    (text, listed)
}

fn type_line<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    i: usize,
    face: &Face,
) -> Result<Changed, Full> {
    let words = s.words_of(s.battlefield()[i]);
    if !words.changes_subtypes() {
        return Ok(Changed::default());
    }
    let d = &mut s.results()[i];
    d.subtypes = t.subtypes().of(face.subtypes.read(&words));
    d.keywords = words.landwalk(d.keywords);
    if !face.types.intersects(Types::LAND) {
        return Ok(Changed::default());
    }
    let printed = BasicLandTypes::of(face.subtypes.read(&Words::NONE));
    let now = BasicLandTypes::of(face.subtypes.read(&words));
    if printed == now {
        return Ok(Changed::default());
    }
    if face.abilities.len() > L::AbilityMask::WIDTH {
        return Err(Full);
    }
    let gone = printed.minus(now).mana().intrinsic_mana();
    let kept = now.mana().intrinsic_mana();
    let mut removed = false;
    for (n, ability) in face.abilities.iter().enumerate() {
        let colors = ability.produces().colors;
        if is_simple(ability) && colors != 0 && colors & gone == colors && colors & kept == 0 {
            let n = AbilityIx::new(u8::try_from(n).map_err(|_| Full)?);
            d.lost = d.lost.with(n).ok_or(Full)?;
            removed = true;
        }
    }
    d.keywords = d.keywords.union(now.minus(printed).mana());
    Ok(Changed {
        land_types: true,
        removed,
    })
}
