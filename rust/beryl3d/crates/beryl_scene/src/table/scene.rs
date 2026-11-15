//! The table's scene objects: one per game object, keyed by the object's slot in the view and
//! kept across updates, so that a card moving between zones is one object throughout.

use beryl_session::protocol::PlayerView;
use glam::{Affine3A, Vec2, Vec3};

use super::place::Scroll;
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, DESKTOP, Layout, Window, place};
use crate::frame::{Band, Draw, ElementId, FrameBuilder, Place, Resolved, Role, StateBits};
use crate::input::FaceSize;
use crate::pose::Pose;

/// How large the table cloth is, in millimetres: past every edge of any window.
const CLOTH: f32 = 4000.0;
/// The cloth lies this far below the cards lying on the table, so that the two never share a
/// height.
const CLOTH_HEIGHT: f32 = -0.5;
const CLOTH_TINT: [u8; 4] = [38, 66, 58, 255];

/// One game object's presence on the table.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SceneObject {
    pose: Pose,
    place: Place,
    visible: bool,
    tapped: bool,
    /// Its index in the view's `objects`, for its name.
    view_index: u32,
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

    /// Puts every object of `view` at its resting place, and forgets the objects it no longer
    /// holds. Until the verbs animate a script's steps, every update does this, as an update
    /// with a reset script must.
    pub fn apply(&mut self, view: &PlayerView) {
        let placement = place(view, &self.layout, &self.camera, &self.scroll);
        self.objects.clear();
        for (index, object) in view.objects.iter().enumerate() {
            let Some(rest) = placement.get(object.slot) else {
                continue;
            };
            let slot = usize::from(object.slot.0);
            if self.objects.len() <= slot {
                self.objects.resize(slot + 1, None);
            }
            self.objects[slot] = Some(SceneObject {
                pose: rest.pose,
                place: rest.place,
                visible: rest.visible,
                tapped: object.tapped,
                view_index: u32::try_from(index).expect("a view holds fewer than 2^32 objects"),
            });
        }
    }

    /// Writes a resolved element and a draw record for every visible card, and one draw record
    /// for the cloth, which comes last among the opaque records.
    pub fn write(&self, view: &PlayerView, builder: &mut FrameBuilder) {
        for (slot, object) in self.objects.iter().enumerate() {
            let Some(object) = object.filter(|object| object.visible) else {
                continue;
            };
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
