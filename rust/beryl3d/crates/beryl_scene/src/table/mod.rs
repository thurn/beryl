//! The 3D table: its space, the camera, and where each game object rests (docs/beryl-3d.md,
//! "The table").
//!
//! Units are millimetres. The table is the plane z = 0; x runs to the viewer's right, y away
//! from the viewer toward the opponent, and z up toward the camera. A card mesh is a card's
//! printed size, 63 by 88, centred on the origin in the plane z = 0 with its face toward +z,
//! so a pose's scale of 1 is a card's real size.

mod camera;
mod place;
mod rows;
mod scene;

use glam::{Vec2, Vec3};

pub use camera::{Camera, CameraData};
pub(crate) use place::{Group, Zones};
pub use place::{Placement, ROWS, Rest, Scroll, place};
pub(crate) use scene::SceneObject;
pub use scene::{Motion, Table};

use crate::input::{Insets, Size};

/// A card's printed width and height.
pub const CARD_WIDTH: f32 = 63.0;
pub const CARD_HEIGHT: f32 = 88.0;
/// The height between two cards lying one on the other, in a pile or an overlapped row.
pub const LAYER: f32 = 0.3;

/// The window the table is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub size: Size,
    /// Physical pixels per logical point.
    pub scale: f32,
    pub safe_area: Insets,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            size: Size {
                width: 1440.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        }
    }
}

/// The band of the table one battlefield row lies in, on the viewer's side: the cards' centre
/// line and the left and right limits their edges stay inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowArea {
    pub y: f32,
    pub left: f32,
    pub right: f32,
}

/// Where a hand stands, relative to the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandData {
    /// How far in front of the eye, in millimetres.
    pub depth: f32,
    /// A card's height on screen, as a fraction of the window's height.
    pub height: f32,
    /// The height of the cards' centres, in normalized device coordinates.
    pub y: f32,
    /// The widest the hand may spread, in normalized device coordinates.
    pub width: f32,
    /// How far each card turns about the line of sight per place from the middle, in radians.
    pub fan: f32,
}

/// Everything placement needs to know about a layout. Positions are for the viewer's side;
/// the opponent's are the same turned half a turn about the table's centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub camera: CameraData,
    /// The least strip of an overlapped card left uncovered, in logical points, both across and
    /// along it.
    pub strip: f32,
    /// The creatures' row, toward the middle of the table.
    pub creatures: RowArea,
    /// The back row's left part: lands and other mana sources.
    pub lands: RowArea,
    /// The back row's right part: every other permanent.
    pub others: RowArea,
    /// How far apart cards stand in a row with room to spare, in millimetres.
    pub gap: f32,
    /// How far each copy under the top of a pile of identical permanents peeks out, in
    /// millimetres to the right and away from the viewer.
    pub pile_step: Vec2,
    pub library: Vec2,
    pub graveyard: Vec2,
    pub exile: Vec2,
    /// The bottom item of the stack, and how far each item above it moves.
    pub stack: Vec3,
    pub stack_step: Vec3,
    pub hand: HandData,
    pub opponent_hand: HandData,
}

/// The desktop layout. The camera is a first estimate fitted by eye to Arena's recordings:
/// the far edge of Arena's table is about 0.85 times as wide on screen as the near edge.
pub const DESKTOP: Layout = Layout {
    camera: CameraData {
        pitch_degrees: 24.0,
        fov_degrees: 30.0,
        target: Vec3::new(0.0, -30.0, 0.0),
        // Both players' rows and piles, and below them the strip the viewer's hand covers.
        required: [Vec2::new(-470.0, -290.0), Vec2::new(470.0, 215.0)],
    },
    strip: 24.0,
    creatures: RowArea {
        y: -62.0,
        left: -400.0,
        right: 320.0,
    },
    lands: RowArea {
        y: -160.0,
        left: -400.0,
        right: -20.0,
    },
    others: RowArea {
        y: -160.0,
        left: 20.0,
        right: 320.0,
    },
    gap: 4.0,
    pile_step: Vec2::new(4.0, 4.0),
    library: Vec2::new(-440.0, -160.0),
    graveyard: Vec2::new(-440.0, -62.0),
    // Above the graveyard: beside it, the opponent's fell off the right edge of the window.
    exile: Vec2::new(-440.0, 36.0),
    stack: Vec3::new(370.0, 0.0, 40.0),
    stack_step: Vec3::new(-12.0, -16.0, 4.0),
    hand: HandData {
        depth: 300.0,
        height: 0.24,
        y: -0.88,
        width: 1.3,
        fan: 0.05,
    },
    opponent_hand: HandData {
        depth: 320.0,
        height: 0.16,
        y: 1.02,
        width: 0.5,
        fan: 0.04,
    },
};
