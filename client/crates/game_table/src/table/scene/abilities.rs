//! The abilities on the stack, which the table draws as cards: each wears its source's picture
//! with a plate that says the ability.

use crate::table::labels::ABILITY_ROLE;
use beryl_protocol::card_records::PrintingIx;
use beryl_protocol::protocol::{PlayerView, StackItem, ZoneName};
use beryl_protocol::script::{Act, Subject};

use super::{Motion, Place, SceneObject, Table, ability_track};
use crate::faces::Faces;
use crate::table::look::{Dress, Look};
use kit_frame::frame::{FrameBuilder, StateBits, ability_element};
use kit_fx::effects;

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
            let Some((object, (held, item), under_way)) =
                self.ability_at(id, view, motion, builder)
            else {
                continue;
            };
            if object.visible {
                self.write_ability(
                    id,
                    (&object, held.ability_printing(item), item, under_way),
                    faces,
                    (back, dress),
                    builder,
                );
            }
        }
    }

    /// The ability numbered `id` as this frame draws it, as `object_at` gives a card, with its
    /// item on the stack and the view that holds the item, whose objects say which printing
    /// the ability's source shows.
    ///
    /// A triggered ability that goes on the stack and resolves within one script, such as an
    /// enters trigger that resolves at once, is on neither the stack of the view before the
    /// script nor that of the view it leads to. Its track still moves it, so it takes its item
    /// from the script's own `emerge` step and its place on its controller's side of the stack.
    fn ability_at<'v>(
        &self,
        id: u32,
        view: &'v PlayerView,
        motion: Option<Motion<'v>>,
        builder: &mut FrameBuilder,
    ) -> Option<(SceneObject, (&'v PlayerView, &'v StackItem), bool)> {
        let resting = self.objects.ability(id);
        let track = motion.and_then(|motion| {
            let track = ability_track(motion.ability_tracks, id)?;
            (!track.segments().is_empty()).then_some((motion, track))
        });
        let Some((motion, track)) = track else {
            return resting
                .and_then(|object| Some((object, (view, item_of(view, &object)?), false)));
        };
        builder.segment();
        let pose = track.pose_at(motion.t);
        let (object, item) = match (motion.finals.ability(id), resting) {
            (Some(object), _) => (object, (motion.view, item_of(motion.view, &object)?)),
            (None, Some(object)) => (object, (view, item_of(view, &object)?)),
            (None, None) => {
                let item = emerging_item(motion.view, id)?;
                let StackItem::Ability { controller, .. } = item else {
                    return None;
                };
                let object = SceneObject {
                    pose,
                    place: Place::Card {
                        zone: ZoneName::Stack,
                        side: *controller,
                        row: None,
                    },
                    visible: true,
                    tapped: false,
                    copies: 1,
                    overlapped: false,
                    view_index: 0,
                };
                (object, (motion.view, item))
            }
        };
        let object = SceneObject {
            pose,
            visible: pose.opacity > 0.0,
            ..object
        };
        Some((
            object,
            item,
            track.moving(motion.t) || track.paused(motion.t),
        ))
    }

    /// Writes the resolved element and the draw record of the ability numbered `id`: `object`,
    /// the stack's `item`, moving if `under_way`, drawn as a card with the board-size
    /// plate face of the ability (its source's picture wearing a plate that says it), or its
    /// source's face while it has none, or the blank layer 0 when it has no source. The
    /// picture is that of `printing`, the printing the source object shows, when it has one.
    fn write_ability(
        &self,
        id: u32,
        (object, printing, item, under_way): (&SceneObject, Option<PrintingIx>, &StackItem, bool),
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
        } = item
        else {
            return;
        };
        // The plate face says the ability over its source's picture (`FaceList::ability_face`).
        // An ability with no source shows the blank layer 0.
        let shown = source.and_then(|_| {
            faces
                .list()
                .ability_face(name, printing, *triggered, *nth, text.as_deref())
        });
        // Until the plate face's board-size layer lands, the source's face stands in.
        let face = shown
            .and_then(|face| faces.board_layer_of(face))
            .or_else(|| {
                let source = source.and(faces.list().ability_source(name, printing))?;
                faces.board_layer_of(source)
            });
        // The phone layout draws the battlefield frame here too, as the web interface's does.
        let full = shown
            .filter(|_| self.layout.style.full_card_faces)
            .and_then(|face| faces.full_layers(Some(face)));
        let state = if under_way { StateBits::MOVING } else { 0 };
        let mut look = Look::new(object.pose, face, full, state, false);
        let on = Subject::Ability { id };
        let effects = &self.data.effects;
        let target = crate::effect::target(on);
        effects::card::look(
            builder.tier(),
            dress.effects,
            target,
            dress.now,
            face,
            &mut look,
        );
        (dress.marks)(on, &mut look);
        self.write_look(
            name,
            (ability_element(id), ABILITY_ROLE, 1),
            object.place,
            &look,
            back,
            builder,
        );
        effects::card::write(effects, dress.effects, target, dress.now, &look, builder);
        self.write_shadow((None, object.place), &look.pose, under_way, builder);
        builder.clear_owner();
    }
}

/// The item of `view`'s stack that `object`, one of its abilities, stands for.
fn item_of<'v>(view: &'v PlayerView, object: &SceneObject) -> Option<&'v StackItem> {
    view.stack_with_resolving().nth(object.view_index as usize)
}

/// The item that the `emerge` step of `view`'s script puts on the stack as the ability numbered
/// `id`.
fn emerging_item(view: &PlayerView, id: u32) -> Option<&StackItem> {
    view.script
        .as_ref()?
        .beats
        .iter()
        .flatten()
        .find_map(|step| match &step.act {
            Act::Emerge {
                id: emerged, item, ..
            } if *emerged == id => Some(item),
            _ => None,
        })
}
