//! The abilities on the stack, which the table draws as cards: each wears its source's picture
//! with a plate that says the ability.

use beryl_protocol::protocol::{PlayerView, StackItem};
use beryl_protocol::script::Subject;

use super::{ABILITY_IDS, Motion, SceneObject, Table, ability_track};
use crate::effects;
use crate::faces::Faces;
use crate::frame::{ElementId, FrameBuilder, Role, StateBits};
use crate::light;
use crate::table::look::{Dress, Look};

impl Table {
    /// Writes each ability on the stack as a card: those of the view the table shows, and
    /// while a script plays, those it leads to and those it moves on their way.
    pub(super) fn write_abilities(
        &self,
        view: &PlayerView,
        motion: Option<Motion<'_>>,
        faces: &Faces,
        (back, dress): (u16, &Dress<'_>),
        builder: &mut FrameBuilder,
    ) {
        let shown = self.objects.abilities.iter().map(|(id, _)| *id);
        let finals = motion.map_or(&[][..], |motion| &motion.finals.abilities[..]);
        let moved = motion.map_or(&[][..], |motion| motion.ability_tracks);
        // Each ability once: those shown, then those only the script's view holds, then those
        // only a track holds.
        let finals = finals
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| self.objects.ability(*id).is_none());
        let moved = moved.iter().map(|(id, _)| *id).filter(|id| {
            self.objects.ability(*id).is_none()
                && motion.is_none_or(|motion| motion.finals.ability(*id).is_none())
        });
        for id in shown.chain(finals).chain(moved) {
            builder.visit();
            let Some((object, view, under_way)) = self.ability_at(id, view, motion, builder) else {
                continue;
            };
            if object.visible {
                self.write_ability(
                    id,
                    (&object, view, under_way),
                    faces,
                    (back, dress),
                    builder,
                );
            }
        }
    }

    /// The ability numbered `id` as this frame draws it, as `object_at` gives a card.
    fn ability_at<'v>(
        &self,
        id: u32,
        view: &'v PlayerView,
        motion: Option<Motion<'v>>,
        builder: &mut FrameBuilder,
    ) -> Option<(SceneObject, &'v PlayerView, bool)> {
        let resting = self.objects.ability(id);
        let track = motion.and_then(|motion| {
            let track = ability_track(motion.ability_tracks, id)?;
            (!track.segments().is_empty()).then_some((motion, track))
        });
        let Some((motion, track)) = track else {
            return resting.map(|object| (object, view, false));
        };
        builder.segment();
        let pose = track.pose_at(motion.t);
        let (object, view) = match motion.finals.ability(id) {
            Some(object) => (object, motion.view),
            None => (resting?, view),
        };
        let object = SceneObject {
            pose,
            visible: pose.opacity > 0.0,
            ..object
        };
        Some((object, view, track.moving(motion.t)))
    }

    /// Writes the resolved element and the draw record of the ability numbered `id`: `object`
    /// as `view`'s stack shows it, moving if `under_way`, drawn as a card with the board-size
    /// plate face of the ability (its source's picture wearing a plate that says it), or its
    /// source's face while it has none, or the blank layer 0 when it has no source.
    fn write_ability(
        &self,
        id: u32,
        (object, view, under_way): (&SceneObject, &PlayerView, bool),
        faces: &Faces,
        (back, dress): (u16, &Dress<'_>),
        builder: &mut FrameBuilder,
    ) {
        let StackItem::Ability {
            source,
            name,
            triggered,
            nth,
            text,
            ..
        } = &view.stack[object.view_index as usize]
        else {
            return;
        };
        // The plate face says the ability over its source's picture. An ability whose words are
        // not a paragraph of its source's printed text (`text` is given) has no plate face, and
        // shows its source's face alone.
        let plate = text
            .is_none()
            .then(|| faces.plate_layer(name, *triggered, *nth))
            .flatten();
        let face = source.and_then(|_| plate.or_else(|| faces.board_layer(name)));
        let full = source.and_then(|_| match plate {
            Some(_) => faces.plate_full_layers(name, *triggered, *nth),
            None => faces.full_layers_named(name),
        });
        let mut look = Look {
            pose: object.pose,
            tint: [255, 255, 255],
            dissolve: 0.0,
            whiten: 0.0,
            face,
            full,
            state: if under_way { StateBits::MOVING } else { 0 },
            interactive: false,
        };
        let on = Subject::Ability { id };
        effects::card::look(dress.effects, on, dress.now, face, &mut look);
        light::marks::look(dress.lit, on, dress.now, &mut look);
        effects::card::write(dress.effects, on, dress.now, &look, builder);
        self.write_shadow((None, object.place), &look.pose, under_way, builder);
        self.write_look(
            name,
            (ElementId(ABILITY_IDS + id % ABILITY_IDS), Role::Ability, 1),
            object.place,
            &look,
            back,
            builder,
        );
    }
}
