//! Whether a card wears the aura of a mythic rare spell on the stack, or the burst it leaves as
//! the spell goes (`own::StackAura`). MTG Arena picks both by rarity alone: its asset lookup tree
//! plays `VFX_Stack_Mythic_Tier2_loop_LUT` while a card of rarity mythic moves to the stack and
//! `VFX_Stack_Mythic_Tier2_outro_LUT` as one moves from it.

use beryl_protocol::protocol::{PlayerView, ZoneName};
use kit_base::data::effect::SubjectColors;

use super::motion::Motion;
use super::{SceneObject, Table, own};
use crate::faces::{Faces, ListedFace};

impl Table {
    /// Where the card in `slot` stands toward the stack at the moment drawn: whether it is on
    /// the stack, and if it has just left it, how long ago, in nanoseconds, the step that took
    /// it off began. `view` is the view before any script, whose record says where a card rests,
    /// and `drawn` the view that names `object`, the card as this frame draws it. While a script plays, the zones its steps
    /// have begun to put the card in decide (`Motion::zone_changes`), from the step that flies
    /// a spell to the stack until the step that carries it off, whatever record it is drawn
    /// with; a card no step has moved yet is where it rests.
    pub(super) fn toward_stack(
        &self,
        slot: usize,
        (view, drawn): (&PlayerView, &PlayerView),
        object: &SceneObject,
        motion: Option<Motion<'_>>,
    ) -> (bool, Option<u64>) {
        let shown = drawn.objects[object.view_index as usize].zone;
        let resting = || {
            let rest = self.objects.cards.get(slot).copied().flatten()?;
            Some(view.objects.get(rest.view_index as usize)?.zone)
        };
        let Some(motion) = motion else {
            return (shown == ZoneName::Stack, None);
        };
        let stack = Some(ZoneName::Stack);
        match motion.zone_changes(slot) {
            (_, None) => (resting() == Some(ZoneName::Stack), None),
            (_, Some(last)) if last.zone == stack => (true, None),
            (before, Some(last)) => {
                let was = before.map_or_else(resting, |before| before.zone);
                let left = (was == Some(ZoneName::Stack)).then(|| motion.t - last.at);
                (false, left)
            }
        }
    }

    /// What the card named `name`, listed in the card store as `listed`, wears for being a
    /// mythic rare on the stack (`toward_stack`).
    pub(super) fn mythic(
        &self,
        name: &str,
        listed: Option<&ListedFace>,
        (on_stack, left_stack): (bool, Option<u64>),
        faces: &Faces,
    ) -> own::StackAura {
        let mythic = !name.is_empty() && faces.is_mythic(name);
        let colors = (listed.filter(|_| mythic)).map_or(SubjectColors::NONE, |face| {
            SubjectColors::from_codes(face.colors.iter().map(String::as_str))
        });
        own::StackAura {
            mythic,
            on_stack,
            left_stack,
            tint: self.data.effects.palette().resolve(colors),
        }
    }
}
