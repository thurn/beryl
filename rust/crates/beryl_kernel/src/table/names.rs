//! The catalog of card names a player can choose in this game.
//!
//! Implements: CR 201.4

use beryl_card::definition::Shape;
use beryl_card::{CardDef, FACE_DOWN_COPY, FaceIx};

use super::GameTable;

/// The game index of each distinct name among `defs` that a card has ([`GameTable::names`]):
/// not a token's or an emblem's, which some card of `defs` lists among what it creates, and
/// not [`FACE_DOWN_COPY`]'s, which is no card's. A split card contributes each half's name
/// (see [CR 709.4a]): "Each split card has two names."
///
/// [CR 709.4a]: https://yawgatog.com/resources/magic-rules/#R7094a
pub(super) fn card_names(defs: &[&'static CardDef]) -> (Vec<usize>, Vec<&'static str>) {
    let made = |def: &CardDef| {
        defs.iter()
            .any(|d| d.tokens.iter().any(|&token| std::ptr::eq(token, def)))
    };
    let mut names: Vec<usize> = Vec::new();
    for (i, &def) in defs.iter().enumerate() {
        let card = !std::ptr::eq(def, &raw const FACE_DOWN_COPY) && !made(def);
        if card
            && !names
                .iter()
                .any(|&n| defs[n].face_name(FaceIx::FRONT) == def.face_name(FaceIx::FRONT))
        {
            names.push(i);
        }
    }
    let mut backs = Vec::new();
    for &i in &names {
        let def = defs[i];
        if matches!(def.shape, Shape::Split { .. }) {
            let name = def.face_name(FaceIx::BACK);
            if !name.is_empty()
                && !backs.contains(&name)
                && !names
                    .iter()
                    .any(|&n| defs[n].face_name(FaceIx::FRONT) == name)
            {
                backs.push(name);
            }
        }
    }
    names.extend(defs.len()..defs.len() + backs.len());
    (names, backs)
}

impl GameTable {
    /// The game index of each card name of the game a player may choose, in game order
    /// ([CR 201.4](https://yawgatog.com/resources/magic-rules/#R2014)): "the player must
    /// choose the name of a card in the Oracle card reference". A token's name is among them
    /// only where a card has it: tokens and emblems are not cards in that reference. Every
    /// other name has the same consequences in this game, as no card of it has that name, so
    /// one choice ("another card") stands for them all.
    #[inline]
    pub fn names(&self) -> &[usize] {
        &self.names
    }

    /// A card name by its catalog number ([`GameTable::names`]).
    pub(crate) fn chosen_name(&self, number: usize) -> Option<&'static str> {
        if let Some(def) = self.defs.get(number) {
            Some(def.face_name(FaceIx::FRONT))
        } else {
            self.split_back_names.get(number - self.defs.len()).copied()
        }
    }
}
