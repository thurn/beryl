//! The 3D table: its space, the camera, and where each game object rests (docs/beryl-3d.md,
//! "The table").
//!
//! Units are millimetres. The table is the plane z = 0; x runs to the viewer's right, y away
//! from the viewer toward the opponent, and z up toward the camera. A card mesh is a card's
//! printed size, 63 by 88, centred on the origin in the plane z = 0 with its face toward +z,
//! so a pose's scale of 1 is a card's real size.

mod anchors;
pub use anchors::spot_of;
pub mod badges;
pub mod far_shade;
mod hands;
pub use hands::HandFits;
pub mod lifted;
mod look;
mod own;
pub mod phone;
mod pile;
mod place;
mod revealed;
mod rows;
mod scene;
mod shadows;
mod shown;
mod stack;
mod tray;
mod tray_layout;
mod web_fit;

use beryl_protocol::protocol::{Side, ZoneName};
use glam::Vec2;

use kit_space::camera::DESKTOP_CAMERA;
pub use kit_space::camera::{Camera, CameraData};
pub use look::{Dress, Look};
pub use phone::{PhoneBands, phone_bands};
pub use pile::{PILE_IDS, PileShown, pile_name};
pub use place::{
    Group, NO_GROUP, PileRules, Zones, Zooms, arrival, blocked, blocking_rows, host_slot,
};
pub use place::{Placement, ROWS, Rest, Rows, Scroll, Scroller, place};
pub use place::{names_host, stack_slots};
pub use rows::RowWindow;
pub use rows::{NameIds, RowChange, reposed};
pub use scene::{ABILITY_IDS, Motion, Table, Turn};
pub use scene::{Objects, PileCount, ability_track};
pub use shown::{Moves, Relaid};
pub use stack::{OnStack, on_stack, row_area};
pub use tray_layout::{
    Across, Held, Second, TrayKind, TrayPoses, TraySpace, landing, tray, tray_area, tray_boxes,
};
pub use web_fit::{FittedPiles, FittedRow, FittedRows};

use kit_base::data::space;
use kit_base::tokens::type_style;
use kit_frame::frame::Place;

/// Half the line height of a life total in logical points.
pub const HALF_LIFE: f32 = type_style::TITLE.size * type_style::TITLE.line_height / 2.0;

pub use kit_ui::orb::DESKTOP_SCALE as ORB_DESKTOP_SCALE;

pub use kit_base::geometry::{CARD_HEIGHT, CARD_WIDTH, Window};
/// The height between two cards lying one on the other, in a pile or an overlapped row.
pub const LAYER: f32 = space::card::LAYER;

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

/// Where a hand stands, relative to the camera (`hand` and `opponent_hand` in `space.json`).
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
    /// coordinates. A hand that would need more rows than fit under it closes its rows up past
    /// their strips instead. On the desktop nothing limits the rows; on the phone they stop at
    /// the top safe inset.
    pub ceiling: f32,
}

/// Where the stack's items stand, relative to the camera at the right of the screen (`stack`
/// in `space.json`). Distances are in normalized device coordinates' vertical unit, half the
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
    pub stack: StackData,
    pub hand: HandData,
    pub opponent_hand: HandData,
    /// The viewer's anchors, then the opponent's.
    pub players: [PlayerAnchor; 2],
    /// On the phone layout, where the stack's items stand in the middle band, which replaces
    /// `stack`; none on the desktop.
    pub phone: Option<phone::PhoneStack>,
    /// On the phone layout, the bands of the window from the opponent's strip to the viewer's
    /// hand, which the flat layer's furniture stands in; none on the desktop.
    pub bands: Option<phone::PhoneBands>,
    /// On the desktop layout fitted to a window (`for_window`), each side's battlefield rows
    /// fitted to where the web interface draws them (`web_fit`), which replace `creatures`,
    /// `lands` and `others`; none otherwise.
    pub fitted: Option<web_fit::FittedRows>,
    /// On the desktop layout fitted to a window, the piles fitted to where the web interface
    /// draws them (`web_fit::piles`), which replace `piles` and set `pile_scale`; none
    /// otherwise.
    pub fitted_piles: Option<web_fit::FittedPiles>,
}

impl Layout {
    /// The layout a window of this shape takes: this one on a desktop window, with its
    /// battlefield rows fitted to where the web interface draws them in that window
    /// (`web_fit`), and the phone layout fitted to `window` when its shorter side is under 500
    /// points (`Window::phone`).
    #[must_use]
    pub fn for_window(&self, window: Window) -> Layout {
        if window.phone() {
            return phone::layout(window);
        }
        let camera = self.fit(window);
        let piles = web_fit::piles(window, &camera);
        Layout {
            fitted: Some(web_fit::rows(window, &camera)),
            fitted_piles: Some(piles),
            pile_scale: piles.scale,
            ..*self
        }
    }

    /// The space the cards a prompt holds up lie in on `window`: this layout's on a desktop
    /// window, and the phone's (`phone::tray`) on a phone, without making the rest of the
    /// phone layout.
    #[must_use]
    pub fn tray_for(&self, window: Window) -> TraySpace {
        if window.phone() {
            phone::tray(window)
        } else {
            self.tray
        }
    }

    /// This layout's camera fitted to `window`: its required rectangle inside the safe area on
    /// the desktop, and inside the battlefield's part of it on the phone (`phone::battlefield`).
    #[must_use]
    pub fn fit(&self, window: Window) -> Camera {
        if self.phone.is_some() {
            Camera::fit_inside(self.camera, window, phone::battlefield(window))
        } else {
            Camera::fit(self.camera, window)
        }
    }
}

use space::layout::desktop;

/// A `RowArea` from the object of the same name in `layout.desktop`.
macro_rules! row_area {
    ($name:ident, $centred:expr) => {
        RowArea {
            y: desktop::$name::Y,
            left: desktop::$name::LEFT,
            right: desktop::$name::RIGHT,
            centre: if $centred {
                Some((desktop::$name::LEFT + desktop::$name::RIGHT) / 2.0)
            } else {
                None
            },
        }
    };
}

/// A `HandData` from the object of the same name in `space.json`.
macro_rules! hand_data {
    ($name:ident) => {
        HandData {
            depth: space::$name::DEPTH,
            step_depth: space::$name::STEP_DEPTH,
            height: space::$name::HEIGHT,
            edge: space::$name::EDGE,
            fan_radius: space::$name::FAN_RADIUS,
            spacing: space::$name::SPACING,
            room: space::$name::ROOM,
            bias_degrees: space::$name::BIAS_DEGREES,
            across: 0.0,
            ceiling: f32::INFINITY,
        }
    };
}

/// The desktop layout, built from `layout.desktop`, `hand`, `opponent_hand` and `stack` in
/// `space.json`. The camera's pitch and field of view are solved by `just b3d camera-fit` from
/// the cards measured in Arena's recordings, `data/beryl3d/camera-fit.json`.
pub const DESKTOP: Layout = Layout {
    camera: DESKTOP_CAMERA,
    strip: desktop::STRIP,
    creatures: row_area!(creatures, true),
    lands: row_area!(lands, false),
    others: row_area!(others, false),
    gap: desktop::GAP,
    pile_step: desktop::PILE_STEP,
    beneath_step: desktop::BENEATH_STEP,
    // The web interface's `PILES` (`ui/src/game/pileLook.ts`): the middle of each top card as
    // `pileLook` draws it on the 1600 by 900 stage, converted as `x / 800 - 1` and `1 - y / 450`.
    piles: [
        [
            desktop::piles::you::LIBRARY,
            desktop::piles::you::GRAVEYARD,
            desktop::piles::you::EXILE,
        ],
        [
            desktop::piles::opponent::LIBRARY,
            desktop::piles::opponent::GRAVEYARD,
            desktop::piles::opponent::EXILE,
        ],
    ],
    // The web draws a pile's card 80 stage pixels wide on the viewer's side and 82 on the
    // opponent's (`PILES[side].width`), against 128 for a creature (`CREATURE_WIDTH` in
    // `ui/src/game/placement.ts`); a creature here is a card at its printed size.
    pile_scale: [desktop::piles::you::SCALE, desktop::piles::opponent::SCALE],
    tray: TraySpace::DESKTOP,
    stack: StackData {
        depth: space::stack::DEPTH,
        step_depth: space::stack::STEP_DEPTH,
        height: space::stack::HEIGHT,
        right: space::stack::RIGHT,
        foot: space::stack::FOOT,
        top: space::stack::TOP,
        step: space::stack::STEP,
        reach: space::stack::REACH,
        lean: space::stack::LEAN,
        spare: space::stack::SPARE,
    },
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
    phone: None,
    bands: None,
    fitted: None,
    fitted_piles: None,
};
