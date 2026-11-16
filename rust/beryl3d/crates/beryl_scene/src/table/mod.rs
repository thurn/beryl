//! The 3D table: its space, the camera, and where each game object rests (docs/beryl-3d.md,
//! "The table").
//!
//! Units are millimetres. The table is the plane z = 0; x runs to the viewer's right, y away
//! from the viewer toward the opponent, and z up toward the camera. A card mesh is a card's
//! printed size, 63 by 88, centred on the origin in the plane z = 0 with its face toward +z,
//! so a pose's scale of 1 is a card's real size.

mod camera;
pub(crate) mod lifted;
mod own;
mod place;
mod revealed;
mod rows;
mod scene;
mod stack;

use glam::{Vec2, Vec3};

pub use camera::{Camera, CameraData};
pub(crate) use place::{Group, NO_GROUP, Zones};
pub use place::{Placement, ROWS, Rest, Scroll, place};
pub(crate) use rows::NameIds;
pub use scene::{ABILITY_IDS, Dress, Look, Motion, Table};
pub(crate) use scene::{Objects, ability_track};

use crate::data::space;
use crate::input::{Insets, Size};

/// A card's printed width and height (`card` in `space.json`).
pub const CARD_WIDTH: f32 = space::card::WIDTH;
pub const CARD_HEIGHT: f32 = space::card::HEIGHT;
/// The height between two cards lying one on the other, in a pile or an overlapped row.
pub const LAYER: f32 = space::card::LAYER;

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

/// Where one player's parts of the table that are not cards stand on the screen, in normalized
/// device coordinates: the middle of their life total, and the top left corner of their unspent
/// mana. A step about a player starts or ends there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerAnchor {
    pub life: Vec2,
    pub pool: Vec2,
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
    /// The viewer's anchors, then the opponent's.
    pub players: [PlayerAnchor; 2],
}

use space::layout::desktop;

/// A `RowArea` from the object of the same name in `layout.desktop`.
macro_rules! row_area {
    ($name:ident) => {
        RowArea {
            y: desktop::$name::Y,
            left: desktop::$name::LEFT,
            right: desktop::$name::RIGHT,
        }
    };
}

/// A `HandData` from the object of the same name in `layout.desktop`.
macro_rules! hand_data {
    ($name:ident) => {
        HandData {
            depth: desktop::$name::DEPTH,
            height: desktop::$name::HEIGHT,
            y: desktop::$name::Y,
            width: desktop::$name::WIDTH,
            fan: desktop::$name::FAN,
        }
    };
}

/// The desktop layout, built from `layout.desktop` in `space.json`. The camera's pitch and
/// field of view are solved by `just b3d camera-fit` from the cards measured in Arena's
/// recordings, `data/beryl3d/camera-fit.json`.
pub const DESKTOP: Layout = Layout {
    camera: CameraData {
        pitch_degrees: desktop::camera::PITCH_DEGREES,
        fov_degrees: desktop::camera::FOV_DEGREES,
        target: desktop::camera::TARGET,
        required: desktop::camera::REQUIRED,
    },
    strip: desktop::STRIP,
    creatures: row_area!(creatures),
    lands: row_area!(lands),
    others: row_area!(others),
    gap: desktop::GAP,
    pile_step: desktop::PILE_STEP,
    library: desktop::LIBRARY,
    graveyard: desktop::GRAVEYARD,
    exile: desktop::EXILE,
    stack: desktop::STACK,
    stack_step: desktop::STACK_STEP,
    hand: hand_data!(hand),
    opponent_hand: hand_data!(opponent_hand),
    // The web interface's anchors (`anchors` in `ui/src/stage/desktop.ts`): `life(side).middle`
    // and `pool(side)` on its 1600 by 900 stage, each point converted as `x / 800 - 1` and
    // `1 - y / 450`.
    players: [
        PlayerAnchor {
            life: Vec2::new(0.000_179, -0.496_672),
            pool: Vec2::new(-0.861_25, -0.186_667),
        },
        PlayerAnchor {
            life: Vec2::new(0.000_179, 0.769_995),
            pool: Vec2::new(-0.802_5, 0.503_728),
        },
    ],
};
