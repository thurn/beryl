//! What the writers of light and effects are given to ask where things are in a frame, whether
//! they are moving, and which way their controllers face.

use beryl_protocol::protocol::Side;
use beryl_protocol::script::Subject;

use super::Show;
use game_table::table::{Table, ability_track};
use glam::{Quat, Vec2};
use kit_frame::frame::{
    AnchorBox, ElementId, Endpoint, FrameBuilder, NodeRef, PaintBand, Resolved, StateBits,
    ability_element,
};
use kit_fx::effects::Reach;

use crate::art::orb;

/// The show as a frame draws it, for the writers of light and effects to ask where things are.
#[derive(Clone, Copy)]
pub struct Scene<'a> {
    show: &'a Show,
    now: u64,
}

impl<'a> Scene<'a> {
    pub(super) fn new(show: &'a Show, now: u64) -> Self {
        Self { show, now }
    }
}

impl Scene<'_> {
    /// Where `subject` was `ago` nanoseconds before the frame's time, on the playing script's
    /// track: at the script's start for a time before it, and where it rests once no script
    /// moves it.
    #[must_use]
    pub fn reach_before(&self, subject: Subject, ago: u64) -> Option<Reach> {
        self.show.reach(subject, self.now.saturating_sub(ago))
    }

    /// The table, for its camera and layout.
    #[must_use]
    pub fn table(&self) -> &Table {
        self.show.table()
    }
}

impl Scene<'_> {
    /// Registers the life totals' screen boxes at the positions shared with the HUD.
    pub(super) fn register_light_targets(&self, builder: &mut FrameBuilder<'_>) {
        let table = self.table();
        let height = table.camera().window.size.height;
        let life = table.layout().play.life;
        let mut half = table.data().light.player_half_pixels * (height / 900.0);
        if let Some(diameter) = life.mark_minimum {
            half = half.max(Vec2::splat(diameter * 0.5));
        }
        for side in [Side::You, Side::Opponent] {
            let centre =
                table.life_centre(side) + Vec2::Y * orb::face_offset(side) * life.orb_scale;
            builder.anchor_node(orb::target_id(side), AnchorBox { centre, half }.rect());
        }
    }

    /// Resolves a decoration's target after the table has drawn it, including any move
    /// into a viewer, a hand lift, or an effect that changed its pose.
    pub(crate) fn light_target(
        &self,
        subject: Subject,
        builder: &FrameBuilder<'_>,
    ) -> Option<LightTarget> {
        let id = light_target_id(subject);
        let camera = self.table().camera();
        if matches!(subject, Subject::Player { .. }) {
            let bounds = builder.endpoint(Endpoint::Node(id))?;
            let hand = &self.table().layout().hand;
            let size = camera.window.size;
            let ndc = bounds.centre / Vec2::new(size.width, size.height) * Vec2::new(2.0, -2.0)
                + Vec2::new(-1.0, 1.0);
            let pose = camera.screen_pose(ndc, hand.depth, Quat::IDENTITY, hand.height);
            let half =
                bounds.half * (pose.scale * camera.data.card.height / hand.height / size.height);
            return Some(LightTarget {
                id,
                bounds,
                reach: Reach { pose, half },
                element: None,
                band: PaintBand::Table,
                moving: false,
            });
        }
        let (index, node) = builder.anchor_element(&NodeRef::Id(id))?;
        let pose = node.pose?;
        Some(LightTarget {
            id,
            bounds: AnchorBox::around(node.corners),
            reach: Reach {
                pose,
                half: camera.data.card.half() * pose.scale,
            },
            element: Some((index, node)),
            band: builder.node_band(index),
            moving: node.state.has(StateBits::MOVING) || node.state.has(StateBits::HELD),
        })
    }

    /// A line can follow a card or ability scrolled out of view. Register its layout
    /// box as a target even when the table had no draw records for it.
    pub(crate) fn line_target(
        &self,
        subject: Subject,
        builder: &mut FrameBuilder<'_>,
    ) -> Option<LightTarget> {
        if let Some(target) = self.light_target(subject, builder) {
            return Some(target);
        }
        let id = light_target_id(subject);
        let reach = self.show.reach(subject, self.now)?;
        let target = Endpoint::Node(id);
        if builder.endpoint(target).is_none() {
            let corners = self.table().camera().card_corners(reach.pose.transform());
            builder.anchor_node(id, AnchorBox::around(corners).rect());
        }
        Some(LightTarget {
            id,
            bounds: builder.endpoint(target)?,
            reach,
            element: None,
            band: PaintBand::Table,
            moving: self.track_moving(subject),
        })
    }

    /// Whether the track is moving a target that has no draw records.
    fn track_moving(&self, subject: Subject) -> bool {
        let Some(playing) = self
            .show
            .playing
            .as_ref()
            .filter(|playing| playing.update.is_some())
        else {
            return false;
        };
        let track = match subject {
            Subject::Object { slot } => playing.compiled.tracks.get(usize::from(slot.0)),
            Subject::Ability { id } => ability_track(&playing.compiled.ability_tracks, id),
            Subject::Player { .. } => None,
        };
        track.is_some_and(|track| track.moving(playing.clock(self.now)))
    }

    /// Which way across the table `subject`'s controller faces, on the screen: -1 up it for the
    /// viewer's, 1 down it for the opponent's, as the web stage's `Reach::toward` has it. An
    /// ability is the viewer's way, as there; a player or a card in a pile (graveyard, exile,
    /// library) faces neither way, so arrows to it arch over the table instead of approaching from
    /// in front of a battlefield row.
    #[must_use]
    pub fn toward(&self, subject: Subject) -> f32 {
        match subject {
            Subject::Object { slot } => {
                let object = self.show.shown.as_ref().and_then(|update| {
                    update
                        .view
                        .objects
                        .iter()
                        .find(|object| object.slot == slot)
                });
                if object.is_some_and(|object| {
                    matches!(
                        object.zone,
                        beryl_protocol::protocol::ZoneName::Graveyard
                            | beryl_protocol::protocol::ZoneName::Exile
                            | beryl_protocol::protocol::ZoneName::Library
                    )
                }) {
                    return 0.0;
                }
                if object.map(|object| object.controller) == Some(Side::Opponent) {
                    1.0
                } else {
                    -1.0
                }
            }
            Subject::Ability { .. } => -1.0,
            Subject::Player { .. } => 0.0,
        }
    }
}

/// The scene id the table or the HUD assigns to this subject.
fn light_target_id(subject: Subject) -> ElementId {
    match subject {
        Subject::Object { slot } => ElementId(u32::from(slot.0)),
        Subject::Ability { id } => ability_element(id),
        Subject::Player { player } => orb::target_id(player),
    }
}

/// The frame geometry used to draw a decoration against its scene target.
#[derive(Clone, Copy)]
pub(crate) struct LightTarget {
    pub id: ElementId,
    pub bounds: AnchorBox,
    /// The target's plane in millimetres. Screen boxes are projected to the light plane.
    pub reach: Reach,
    pub element: Option<(u32, Resolved)>,
    pub band: PaintBand,
    pub moving: bool,
}
