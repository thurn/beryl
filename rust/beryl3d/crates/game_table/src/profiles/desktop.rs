//! The reference table fitted to the web stage's projected rows and piles.

use glam::Vec2;
use kit_base::data::profiles::{
    HeldDraw, Life, LifeChange, LifeText, Play, ProfileData, TableStyle,
};
use kit_base::data::{audit, space};
use kit_space::arrange::row::ScaleLimit;
use kit_space::camera::DESKTOP_CAMERA;

use super::{row, web_fit};
use crate::table::{
    HandData, Layout, PlayerAnchor, RowArea, RowZoom, StackData, StackPlacement, TraySpace, Window,
};

use space::layout::desktop;
pub(crate) use space::layout::desktop::tray as tray_tuning;

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

/// A `HandData` from the object of the same name in `space-constants.json`.
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
/// `space-constants.json`. The camera's pitch and field of view are solved by `just b3d camera-fit` from
/// the cards measured in Arena's recordings, `data/beryl3d/camera-fit.json`.
pub const REFERENCE: Layout = Layout {
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
    tray: TraySpace::REFERENCE,
    stack: StackPlacement::Anchored(StackData {
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
    }),
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
    target: audit::TARGET_POINTS,
    style: TableStyle {
        show_exile: true,
        show_pile_sides: true,
        show_copy_labels: true,
        full_card_faces: true,
        hand_away: space::shown::opening::HAND_AWAY,
        far_shade: true,
        tuck_rise: true,
    },
    play: Play {
        held_draw: HeldDraw::Fixed {
            ndc: Vec2::new(0.422, 0.0),
            size: 1.3,
        },
        life: Life {
            orb_scale: kit_ui::orb::DESKTOP_SCALE,
            text: LifeText::Life,
            opening_hides_viewer: false,
            change: LifeChange::BesideOrb,
        },
    },
    camera_area: None,
    life_dock: None,
    revealed: (space::revealed::AT, space::revealed::HEIGHT),
    row_zoom: None,
    bands: None,
    fitted: None,
    fitted_piles: None,
};

/// Fits the rows and piles in a window while keeping their projected stage positions.
pub(super) fn layout(data: &ProfileData, window: Window) -> Layout {
    let table = &data.table;
    let mut layout = Layout {
        camera: table.camera,
        strip: table.strip,
        creatures: row(table.creatures, true),
        lands: row(table.lands, false),
        others: row(table.others, false),
        gap: table.gap,
        pile_step: table.pile_step,
        style: data.table_style,
        play: data.play,
        target: data.checks.target_points,
        row_zoom: Some(RowZoom {
            minimum: desktop::web::ZOOM_LEAST,
            back: ScaleLimit {
                minimum: desktop::LEAST_BACK_ZOOM,
                shared_minimum: desktop::web::BACK_ROW_PULL,
            },
        }),
        ..REFERENCE
    };
    let camera = layout.fit(window);
    let piles = web_fit::piles(window, &camera);
    layout.fitted = Some(web_fit::rows(window, &camera));
    layout.fitted_piles = Some(piles);
    layout.pile_scale = piles.scale;
    layout
}
