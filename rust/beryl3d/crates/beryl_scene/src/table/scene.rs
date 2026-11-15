//! The table's scene objects: one per game object, keyed by the object's slot in the view and
//! kept across updates, so that a card moving between zones is one object throughout.

use beryl_session::protocol::PlayerView;
use glam::{Affine3A, Vec2, Vec3};

use super::place::{Placement, Scroll};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, DESKTOP, Layout, Window, place};
use crate::frame::{Band, Draw, ElementId, FrameBuilder, Place, Resolved, Role, StateBits};
use crate::input::FaceSize;
use crate::motion::Track;
use crate::pose::Pose;

/// How large the table cloth is, in millimetres: past every edge of any window.
const CLOTH: f32 = 4000.0;
/// The cloth lies this far below the cards lying on the table, so that the two never share a
/// height.
const CLOTH_HEIGHT: f32 = -0.5;
const CLOTH_TINT: [u8; 4] = [38, 66, 58, 255];

/// One game object's presence on the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneObject {
    pose: Pose,
    place: Place,
    visible: bool,
    tapped: bool,
    /// Its index in the view's `objects`, for its name.
    view_index: u32,
}

impl SceneObject {
    /// Where it rests.
    pub(crate) fn pose(&self) -> Pose {
        self.pose
    }
}

/// What moves while a script plays: a track for each object that moves, by slot, the time on
/// the script's clock, and the scene objects at rest in the view the script leads to.
#[derive(Clone, Copy, Debug)]
pub struct Motion<'a> {
    pub tracks: &'a [Track],
    pub t: u64,
    pub(crate) finals: &'a [Option<SceneObject>],
    pub view: &'a PlayerView,
}

/// The 3D table: the camera and every scene object.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    layout: Layout,
    camera: Camera,
    scroll: Scroll,
    /// Indexed by the object's slot.
    objects: Vec<Option<SceneObject>>,
}

impl Default for Table {
    fn default() -> Self {
        Self::new(DESKTOP, Window::default())
    }
}

impl Table {
    #[must_use]
    pub fn new(layout: Layout, window: Window) -> Self {
        Self {
            layout,
            camera: Camera::fit(layout.camera, window),
            scroll: Scroll::default(),
            objects: Vec::new(),
        }
    }

    #[must_use]
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Fits the camera to a window of a new shape and recomputes every resting place.
    pub fn resize(&mut self, window: Window, view: Option<&PlayerView>) {
        self.camera = Camera::fit(self.layout.camera, window);
        if let Some(view) = view {
            self.apply(view);
        }
    }

    /// Where the object in `slot` rests in the view the table shows, if it shows it.
    #[must_use]
    pub fn rest(&self, slot: usize) -> Option<Pose> {
        Some(self.objects.get(slot).copied().flatten()?.pose)
    }

    /// Every object's resting place in `view`, for this window.
    #[must_use]
    pub fn placement(&self, view: &PlayerView) -> Placement {
        place(view, &self.layout, &self.camera, &self.scroll)
    }

    /// Puts every object of `view` at its resting place, and forgets the objects it no longer
    /// holds, as an update with a reset script must.
    pub fn apply(&mut self, view: &PlayerView) {
        self.objects = Self::objects_for(view, &self.placement(view));
    }

    /// The scene objects of `view`, each at its resting place in `placement`, by slot.
    pub(crate) fn objects_for(
        view: &PlayerView,
        placement: &Placement,
    ) -> Vec<Option<SceneObject>> {
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
                view_index: u32::try_from(index).expect("a view holds fewer than 2^32 objects"),
            });
        }
        objects
    }

    /// Exchanges the scene objects for `objects`, which `objects_for` made for the view the
    /// table now shows. Nothing is allocated, so a script can end inside a frame.
    pub(crate) fn swap_objects(&mut self, objects: &mut Vec<Option<SceneObject>>) {
        std::mem::swap(&mut self.objects, objects);
    }

    /// Writes a resolved element and a draw record for every visible card, and one draw record
    /// for the cloth, which comes last among the opaque records. `view` is the view the table
    /// shows. While a script plays, `motion` gives the objects that move their poses, and the
    /// records of the view the script leads to.
    pub fn write(&self, view: &PlayerView, motion: Option<Motion<'_>>, builder: &mut FrameBuilder) {
        builder.camera(self.camera.view_projection);
        let slots = motion
            .map_or(0, |motion| motion.tracks.len())
            .max(self.objects.len());
        for slot in 0..slots {
            builder.visit();
            let resting = self.objects.get(slot).copied().flatten();
            let track = motion.and_then(|motion| {
                let track = motion.tracks.get(slot)?;
                (!track.segments().is_empty()).then_some((motion, track))
            });
            let moving = track.and_then(|(motion, track)| {
                builder.segment();
                let pose = track.pose_at(motion.t);
                match motion.finals.get(slot).copied().flatten() {
                    Some(object) => Some((object, motion.view, pose)),
                    None => resting.map(|object| (object, view, pose)),
                }
            });
            let (object, view) = match (moving, resting) {
                (Some((object, view, pose)), _) => (
                    SceneObject {
                        pose,
                        visible: pose.opacity > 0.0,
                        ..object
                    },
                    view,
                ),
                (None, Some(object)) => (object, view),
                (None, None) => continue,
            };
            if !object.visible {
                continue;
            }
            let transform = object.pose.transform();
            let name = view.objects[object.view_index as usize]
                .name
                .as_deref()
                .unwrap_or("");
            let mut state = 0;
            if object.tapped {
                state |= StateBits::TAPPED;
            }
            if object.pose.turn > 0.5 {
                state |= StateBits::FACE_DOWN;
            }
            builder.resolved(
                name,
                Resolved {
                    id: ElementId(u32::try_from(slot).expect("slots are u16")),
                    parent: u32::MAX,
                    role: Role::Card,
                    name: crate::frame::NameRef::default(),
                    place: object.place,
                    corners: self.corners(transform),
                    depth: self.camera.depth(object.pose.position),
                    opacity: object.pose.opacity,
                    clip: None,
                    interactive: false,
                    state: StateBits(state),
                    // Card faces are not loaded yet, so every card is drawn from layer 0.
                    face: None,
                },
            );
            let opacity = (object.pose.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            builder.draw(
                Band::Opaque,
                Draw::card(
                    transform,
                    [255, 255, 255, opacity],
                    FaceSize::Board,
                    0,
                    0,
                    0.0,
                    0.0,
                ),
            );
        }
        let cloth = Affine3A::from_scale_rotation_translation(
            Vec3::new(CLOTH / CARD_WIDTH, CLOTH / CARD_HEIGHT, 1.0),
            glam::Quat::IDENTITY,
            Vec3::new(0.0, 0.0, CLOTH_HEIGHT),
        );
        builder.draw(Band::Opaque, Draw::table(cloth, CLOTH_TINT));
    }

    /// A card's corners on screen, in logical points: bottom left, bottom right, top right and
    /// top left of its face.
    fn corners(&self, transform: Affine3A) -> [Vec2; 4] {
        let (x, y) = (CARD_WIDTH / 2.0, CARD_HEIGHT / 2.0);
        [(-x, -y), (x, -y), (x, y), (-x, y)].map(|(x, y)| {
            self.camera
                .to_screen(transform.transform_point3(Vec3::new(x, y, 0.0)))
        })
    }
}
