//! An ability a save names: one to activate, or one on the stack as a target ([`AbilityRef`]).

use beryl_engine::card::{AbilityKind, FaceAbility, ObjectAbility};
use beryl_engine::state::{StackEntry, StackItem};
use beryl_engine::{GameTable, Layout, State};
use serde::{Deserialize, Serialize};

use super::is_false;

/// A permanent's ability to activate: one of its face up's own, by its place among that
/// face's activated and mana abilities (so a static or triggered ability added to the card
/// moves nothing), or the `n`th ability other objects grant it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityRef {
    #[serde(default, skip_serializing_if = "is_false")]
    pub granted: bool,
    pub n: u8,
}

impl AbilityRef {
    pub(super) fn of<L: Layout>(
        t: &GameTable,
        s: &State<L>,
        slot: L::Slot,
        ability: ObjectAbility,
    ) -> Self {
        match ability {
            ObjectAbility::Own(i) => {
                let face = s.definition(slot).face(t);
                let before = face.abilities[..i.index()]
                    .iter()
                    .filter(|a| matches!(a.kind(), AbilityKind::Activated | AbilityKind::Mana))
                    .count();
                AbilityRef {
                    granted: false,
                    n: u8::try_from(before).expect("a face's abilities number under 256"),
                }
            }
            ObjectAbility::Granted(n) => AbilityRef { granted: true, n },
        }
    }
}

impl AbilityRef {
    /// The ability of a stack entry: a triggered one by its place among its face's triggered
    /// abilities, an activated one by its place among its face's activated and mana
    /// abilities, as [`AbilityRef::of`] counts, or by its place among those its face grants
    /// to other objects.
    pub(super) fn on_stack<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> Self {
        let place = |card, face, i: usize, of: &[AbilityKind]| {
            let before = t.def(card).face(face).abilities[..i]
                .iter()
                .filter(|a| of.contains(&a.kind()))
                .count();
            u8::try_from(before).expect("a face's abilities number under 256")
        };
        match entry.item {
            StackItem::Triggered { of, .. } => AbilityRef {
                granted: false,
                n: place(
                    of.card,
                    of.face,
                    of.ability.index(),
                    &[AbilityKind::Triggered],
                ),
            },
            StackItem::Activated { of } => match of.ability {
                FaceAbility::Own(i) => AbilityRef {
                    granted: false,
                    n: place(
                        of.card,
                        of.face,
                        i.index(),
                        &[AbilityKind::Activated, AbilityKind::Mana],
                    ),
                },
                FaceAbility::Grant(i) => AbilityRef {
                    granted: true,
                    n: u8::try_from(i.index()).expect("a face's grants number under 256"),
                },
            },
            StackItem::Spell { .. } => unreachable!("an ability's entry"),
        }
    }
}
