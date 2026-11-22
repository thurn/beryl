//! The scene objects of a view: each card, and each ability on the stack, at its resting place,
//! with the cards lying on the table that contact shadows fall on (`Floors`).

use beryl_protocol::protocol::{PlayerView, StackItem};
use kit_frame::frame::Place;

use super::{Objects, SceneObject, Table};
use crate::table::place::Placement;
use crate::table::shadows::Floors;

impl Objects {
    /// Ability numbers in the view's stack order, including items outside the window.
    #[must_use]
    pub fn ability_numbers(&self) -> &[u32] {
        &self.numbers
    }

    /// The table container of a card, including when a screen box shows it.
    #[must_use]
    pub fn card_place(&self, slot: usize) -> Option<Place> {
        Some(self.cards.get(slot)?.as_ref()?.place)
    }
}

impl Table {
    /// The scene objects of `view`, whose stack's abilities `numbers` numbers, each at its
    /// resting place in `placement`.
    pub fn objects_for(
        &self,
        view: &PlayerView,
        numbers: Vec<u32>,
        placement: &Placement,
    ) -> Objects {
        let mut abilities = Vec::with_capacity(placement.abilities.len());
        let stack = view
            .stack
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, StackItem::Ability { .. }));
        for ((index, _), &id) in stack.zip(&numbers) {
            let Some(rest) = placement.ability(id) else {
                continue;
            };
            abilities.push((
                id,
                SceneObject {
                    pose: rest.pose,
                    place: rest.place,
                    visible: rest.visible,
                    tapped: false,
                    copies: 1,
                    view_index: u32::try_from(index).expect("a stack holds fewer than 2^32 items"),
                },
            ));
        }
        abilities.sort_unstable_by_key(|(id, _)| *id);
        let cards = Self::cards_for(view, placement);
        Objects {
            floors: Floors::of(&cards, self.camera().data.card),
            cards,
            abilities,
            numbers,
            piles: placement.piles,
            rows: placement.rows,
        }
    }

    /// The scene objects of `view`'s cards, each at its resting place in `placement`, by slot.
    fn cards_for(view: &PlayerView, placement: &Placement) -> Vec<Option<SceneObject>> {
        let mut objects = Vec::new();
        for (index, object) in view.objects.iter().enumerate() {
            let Some(rest) = placement.get(object.slot) else {
                continue;
            };
            let slot = usize::from(object.slot.0);
            if objects.len() <= slot {
                objects.resize(slot + 1, None);
            }
            objects[slot] = Some(SceneObject {
                pose: rest.pose,
                place: rest.place,
                visible: rest.visible,
                tapped: object.tapped,
                copies: rest.copies,
                view_index: u32::try_from(index).expect("a view holds fewer than 2^32 objects"),
            });
        }
        objects
    }
}
