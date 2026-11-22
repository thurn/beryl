//! The 3D table: its space, the camera, and where each game object rests (docs/beryl-3d.md,
//! "The table").
//!
//! Units are millimetres. The table is the plane z = 0; x runs to the viewer's right, y away
//! from the viewer toward the opponent, and z up toward the camera. A card mesh is a card's
//! printed size (`card.json`, which the camera carries as `CameraData::card`), centred on the
//! origin in the plane z = 0 with its face toward +z, so a pose's scale of 1 is a card's real
//! size.

pub mod badges;
mod clearance;
mod drop_regions;
pub use drop_regions::DropZone;
mod hands;
pub use hands::HandFits;
mod hud_anchors;
pub use hud_anchors::HudAnchor;
pub mod labels;
pub mod lifted;
mod location;
mod look;
mod own;
pub(crate) mod pile;
mod place;
mod region_fit;
mod revealed;
mod rows;
mod scene;
mod shadows;
mod shown;
mod stack;
mod tray;
mod tray_layout;

use beryl_protocol::protocol::{Side, ZoneName};
use glam::Vec2;

use crate::profiles::TableBands;
pub use clearance::row_area;
use kit_base::data::profiles::Play;
pub use kit_base::data::profiles::TableStyle;
use kit_base::geometry::Rect;
pub use kit_space::camera::{Camera, CameraData};
pub use look::{Dress, Look};
pub use pile::{PILE_IDS, PileShown, pile_name};
pub use place::{
    ContainerDecision, Placement, ROWS, Rest, RowDecision, Rows, Scroll, Scroller, place,
};
pub use place::{
    Group, NO_GROUP, PileRules, Zones, Zooms, arrival, blocked, blocking_rows, host_slot,
};
pub use place::{names_host, prepare_row, row_of, stack_slots, zone_members};
pub use region_fit::{
    FittedPiles, FittedRegions, FittedRow, FittedRows, RegionFit, RegionPolicy, RowProjection,
    RowZoom, ScreenProjection, TabletopShade,
};
pub use rows::{Beneath, Groups, NameIds, RowChange, SameName, grouped, reposed};
pub use scene::{ABILITY_IDS, Motion, Table, Turn};
pub use scene::{Objects, PileCount, ability_track};
pub use shown::{Moves, Relaid};
pub(crate) use stack::{CentredStack, StackPlacement};
pub use stack::{OnStack, on_stack};
pub use tray_layout::{
    Across, Held, Second, TrayKind, TrayPoses, TraySpace, landing, tray, tray_area, tray_boxes,
};

use kit_base::tokens::type_style;
pub use location::Place;

/// Half the line height of a life total in logical points.
pub const HALF_LIFE: f32 = type_style::TITLE.size * type_style::TITLE.line_height / 2.0;

pub use kit_base::geometry::{RowWindow, Window};

/// Whether a card resting at `place` takes the pointer, once it is face up and the viewer knows
/// its name: one on the battlefield, on the stack or in the viewer's hand. A pile is one
/// element, so no card in it does. The table leaves out a face-up card with no name, such as a
/// copy of a face-down creature, because every interactive element has a name.
#[must_use]
pub fn takes_pointer(place: Place) -> bool {
    matches!(
        place,
        Place::Card {
            zone: ZoneName::Battlefield | ZoneName::Stack,
            ..
        } | Place::Card {
            zone: ZoneName::Hand,
            side: Side::You,
            ..
        }
    )
}

/// The band of the table one battlefield row lies in, on the viewer's side: the cards' centre
/// line and the left and right limits their edges stay inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowArea {
    pub y: f32,
    pub left: f32,
    pub right: f32,
    /// Where across the table a row with room to spare is centred, as near as its limits let
    /// it be; none for a row laid from its left limit. The web interface centres only the
    /// creatures' row, on the stage's middle; the lands run from the table's edge inward and the
    /// other permanents from the middle outward (`sideRows` in `ui/src/game/battlefield.ts`), so
    /// a permanent that leaves one of those rows moves only the cards to its right.
    pub centre: Option<f32>,
}

/// Where a hand stands relative to the camera, after fitting the selected profile.
/// The middle of the edge of each card nearest the screen's middle (the viewer's cards' top
/// edges, the opponent's bottom edges) lies on one circle, whatever the hand's size, so a
/// crowded hand turns and sinks less for each card than a spread one, as Arena's does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandData {
    /// How far in front of the eye the first card stands, in millimetres.
    pub depth: f32,
    /// How much nearer the eye each card stands than the one before it, in millimetres.
    pub step_depth: f32,
    /// A card's height on screen, as a fraction of the window's height.
    pub height: f32,
    /// The height of the middle card's inner edge, in normalized device coordinates.
    pub edge: f32,
    /// The radius of the circle, in card widths.
    pub fan_radius: f32,
    /// How far apart the cards' centres stand while the hand has room, in card widths.
    pub spacing: f32,
    /// The widest the outer cards' centres may stand apart before the cards close up, in card
    /// widths.
    pub room: f32,
    /// How far the whole fan leans clockwise, in degrees.
    pub bias_degrees: f32,
    /// Where across the screen the fan's middle stands, in normalized device coordinates: the
    /// window's middle on the desktop, and on the phone the opponent's fan stands at the right
    /// of their strip, as the web interface's `backSpot` puts it.
    pub across: f32,
    /// The highest the inner edge of the hand's back row may stand, in normalized device
    /// coordinates. Cards that cannot keep a press square below the ceiling scroll into view.
    pub ceiling: f32,
}

/// Where the stack's items stand, relative to the camera at the right of the screen (`stack`
/// in the layout profile). Distances are in normalized device coordinates' vertical unit, half the
/// window's height, both across and up the screen. The top item stands in front, and each item
/// beneath peeks out above it and to its left. A stack too tall to fit shrinks, all its items
/// alike, and one too tall to leave each item beneath a reachable strip stands in rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackData {
    /// How far in front of the eye the bottom item stands, in millimetres.
    pub depth: f32,
    /// How much nearer the eye each item stands than the one beneath it, in millimetres.
    pub step_depth: f32,
    /// An item's height on screen at full size, as a fraction of the window's height.
    pub height: f32,
    /// The distance from the window's right edge to the right edge of the full-size column.
    pub right: f32,
    /// The height of the top item's lower edge, in normalized device coordinates.
    pub foot: f32,
    /// The highest the stack's top may reach, in normalized device coordinates.
    pub top: f32,
    /// How far each item beneath peeks out above the one over it while the stack is short.
    pub step: f32,
    /// The most all the items beneath the top one may peek out together.
    pub reach: f32,
    /// How far each item beneath lies to the left, for each unit it peeks out above.
    pub lean: f32,
    /// How many times the reachable square's side each item beneath keeps uncovered, across
    /// and up, once the stack is too tall to lean and stands in rows.
    pub spare: f32,
    /// The smallest scale of an item in the stack.
    pub least_scale: f32,
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
    pub(crate) blocker_spacing: f32,
    pub(crate) life_clearance: f32,
    pub(crate) cloth: kit_base::data::profiles::Cloth,
    pub(crate) pile: kit_base::data::profiles::PileGeometry,
    /// How far each copy under the top of a pile of identical permanents peeks out, in
    /// millimetres to the right and away from the viewer.
    pub pile_step: Vec2,
    /// How far each card under a merged permanent peeks out from behind it, to the left and
    /// away from the viewer, as a fraction of a card's width and height: the web interface's
    /// `BENEATH_STEP` (`ui/src/game/placement.ts`).
    pub beneath_step: Vec2,
    /// Where each pile lies: the point of the screen, in normalized device coordinates, over
    /// which its bottom card's centre lies on the table. By side, the viewer's first, then
    /// library, graveyard and exile.
    pub piles: [[Vec2; 3]; 2],
    /// How large a pile's cards are against a card's printed size, the viewer's then the
    /// opponent's.
    pub pile_scale: [f32; 2],
    /// The space the cards a prompt holds up are laid out in.
    pub tray: TraySpace,
    pub(crate) stack: StackPlacement,
    /// Minimum reachable square for cards and controls, in logical points.
    pub target: f32,
    pub style: TableStyle,
    pub play: Play,
    pub hand: HandData,
    pub opponent_hand: HandData,
    /// The viewer's anchors, then the opponent's.
    pub players: [PlayerAnchor; 2],
    /// The window bands used by features that follow the table.
    pub bands: Option<TableBands>,
    pub(crate) camera_area: Option<Rect>,
    pub(crate) life_dock: Option<LifeDock>,
    /// Revealed cards' centre in normalized device coordinates and height as a fraction of
    /// the window's height.
    pub(crate) revealed: Option<(Vec2, f32)>,
    pub(crate) region_fit: RegionFit,
    pub(crate) row_zoom: Option<RowZoom>,
    /// Row regions, if a projection fit supplies them.
    pub fitted: Option<FittedRows>,
    /// Piles anchored at the projected centre of the top card.
    pub fitted_piles: Option<FittedPiles>,
    pub(crate) tabletop_shade: Option<TabletopShade>,
}

/// Space for raising the life total over a hand that spills out of its band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LifeDock {
    pub gap: f32,
    pub middle: f32,
    pub top: f32,
    pub pool_offset: Vec2,
}

impl LifeDock {
    pub(crate) fn anchor(self, window: Window, hand_top: f32) -> PlayerAnchor {
        let raised = hand_top - self.gap - HALF_LIFE;
        let lowest_clear = self.top + HALF_LIFE;
        let y = self.middle.min(raised.max(lowest_clear));
        let life = Vec2::new(0.0, 1.0 - y / window.size.height * 2.0);
        let offset = self.pool_offset / Vec2::new(window.size.width, -window.size.height) * 2.0;
        PlayerAnchor {
            life,
            pool: life + offset,
        }
    }
}

impl Layout {
    /// Whether this zone has its own visible pile in the layout.
    #[must_use]
    pub fn pile_visible(&self, zone: ZoneName) -> bool {
        zone != ZoneName::Exile || self.style.show_optional_piles
    }

    /// The window line the horizontal stack scrolls along, in logical points from the top.
    #[must_use]
    pub fn centred_stack_line(&self) -> Option<f32> {
        match self.stack {
            StackPlacement::Anchored(_) => None,
            StackPlacement::Centred(stack) => Some(stack.middle),
        }
    }

    /// Fits the camera's required rectangle in its region of the window.
    #[must_use]
    pub fn fit(&self, window: Window) -> Camera {
        self.camera_area.map_or_else(
            || Camera::fit(self.camera, window),
            |area| Camera::fit_inside(self.camera, window, area),
        )
    }
}
