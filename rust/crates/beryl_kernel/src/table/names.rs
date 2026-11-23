//! The catalog of card names a player can choose in this game.
//!
//! Implements: CR 201.4

use beryl_card::definition::Shape;
use beryl_card::{CardDef, FACE_DOWN_COPY, FaceIx};

use super::GameTable;

/// The catalog numbers and additional face names for the cards in `defs` ([`GameTable::names`]).
/// The catalog skips tokens, emblems and [`FACE_DOWN_COPY`]. A split card contributes each
/// half's name (see [CR 709.4a], "Each split card has two names.").
///
/// For preparation cards, see [CR 723.5], "If an effect instructs a player to choose a card
/// name and the player wants to choose a preparation card’s alternative name, the player may
/// do so."
///
/// [CR 201.4d] says "If a player wants to choose the name of the back face of a double-faced
/// card, the player may do so."
///
/// [CR 201.4d]: https://yawgatog.com/resources/magic-rules/#R2014d
/// [CR 723.5]: https://yawgatog.com/resources/magic-rules/#R7235
/// [CR 709.4a]: https://yawgatog.com/resources/magic-rules/#R7094a
pub(super) fn card_names(defs: &[&'static CardDef]) -> (Vec<usize>, Vec<&'static str>) {
    let is_card = |def: &CardDef| {
        !std::ptr::eq(def, &raw const FACE_DOWN_COPY)
            && !defs
                .iter()
                .any(|d| d.tokens.iter().any(|&token| std::ptr::eq(token, def)))
    };
    let mut names: Vec<usize> = Vec::new();
    for (i, &def) in defs.iter().enumerate() {
        if is_card(def)
            && !names
                .iter()
                .any(|&n| defs[n].face_name(FaceIx::FRONT) == def.face_name(FaceIx::FRONT))
        {
            names.push(i);
        }
    }
    // Keep existing split and preparation choices at their catalog numbers.
    let split_and_prepared_cards = names
        .iter()
        .map(|&i| defs[i])
        .filter(|def| matches!(def.shape, Shape::Split { .. } | Shape::Prepared));
    let double_faced_cards = defs
        .iter()
        .copied()
        .filter(|def| def.shape.is_double_faced() && is_card(def));
    let mut additional_face_names = Vec::new();
    for def in split_and_prepared_cards.chain(double_faced_cards) {
        let name = def.face_name(FaceIx::BACK);
        if !name.is_empty()
            && !additional_face_names.contains(&name)
            && !names
                .iter()
                .any(|&n| defs[n].face_name(FaceIx::FRONT) == name)
        {
            additional_face_names.push(name);
        }
    }
    names.extend(defs.len()..defs.len() + additional_face_names.len());
    (names, additional_face_names)
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
            self.additional_face_names
                .get(number - self.defs.len())
                .copied()
        }
    }
}
