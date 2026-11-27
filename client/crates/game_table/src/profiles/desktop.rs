//! The profile geometry of an anchored table.

use super::row;
use crate::table::{
    Across, HandData, Layout, PlayerAnchor, RegionFit, RowZoom, StackData, StackPlacement,
    TableShape, TraySpace,
};
use kit_base::data::Card;
use kit_base::data::profiles::{Anchored, ProfileData, TrayTuning, anchored};
use kit_base::geometry::Window;
use kit_base::tokens::{size, space};
use kit_ui::layout::DOCK_RIGHT;

/// The profile's table before any window-specific fitting.
pub(super) fn reference(profile: &ProfileData, data: &Anchored) -> Layout {
    let table = &profile.table;
    let piles = [data.piles.you, data.piles.opponent];
    Layout {
        shape: TableShape::PerPlayerPiles,
        camera: table.camera,
        strip: table.strip,
        creatures: row(table.creatures, true),
        lands: row(table.lands, false),
        others: row(table.others, false),
        gap: table.gap,
        blocker_spacing: table.blocker_spacing,
        life_clearance: data.life_clearance,
        cloth: table.cloth,
        pile: table.pile,
        pile_step: table.pile_step,
        beneath_step: table.beneath_step,
        piles: piles.map(|pile| [pile.outer, pile.middle, pile.inner]),
        pile_scale: piles.map(|pile| pile.scale),
        shared_piles: [
            data.shared_piles.library,
            data.shared_piles.graveyard,
            data.shared_piles.exile,
        ],
        shared_pile_scale: data.shared_piles.scale,
        tray: tray(
            data.tray,
            table.tray_tuning,
            table.camera.card,
            profile.checks.target_points,
        ),
        stack: StackPlacement::Anchored(stack(data.stack)),
        hand: hand(data.hand),
        opponent_hand: hand(data.opponent_hand),
        players: data.players.map(|player| PlayerAnchor {
            life: player.life,
            pool: player.pool,
        }),
        target: profile.checks.target_points,
        style: profile.table_style,
        play: profile.play,
        camera_area: None,
        life_dock: None,
        revealed: None,
        region_fit: RegionFit::default(),
        row_zoom: Some(RowZoom::independent(data.least_row_scale)),
        bands: None,
        fitted: None,
        fitted_piles: None,
        tabletop_shade: None,
    }
}

pub(super) fn tray(
    data: anchored::Tray,
    tuning: TrayTuning,
    card: Card,
    grid_target: f32,
) -> TraySpace {
    TraySpace {
        stage_height: data.stage_height,
        y: data.y,
        height: data.height,
        across: Across::Middle {
            half_most: data.half_most,
            stack_side: data.stack_side,
        },
        second: None,
        most: data.most,
        least: data.least,
        gap: data.gap,
        least_step: 0.0,
        grid_target,
        heading: data.heading,
        tuning,
        card,
    }
}

fn hand(data: anchored::Hand) -> HandData {
    HandData {
        depth: data.depth,
        step_depth: data.step_depth,
        height: data.height,
        edge: data.edge,
        fan_radius: data.fan_radius,
        spacing: data.spacing,
        room: data.room,
        bias_degrees: data.bias_degrees,
        across: 0.0,
        ceiling: f32::INFINITY,
        back_left: None,
    }
}

/// Caps the desktop hand's centre span so its rightmost card clears the turn and pass-on buttons
/// at the window's bottom right, closing up the fan on narrower aspect ratios such as 1440x900
/// without changing wider windows.
pub(super) fn fitted_hand_room(profile: &ProfileData, window: Window, hand: HandData) -> f32 {
    let card = profile.table.camera.card;
    let card_width = window.size.height * hand.height * card.width / card.height;
    if card_width <= 0.0 {
        return hand.room;
    }
    // `turn_action` places the round `pass_on` button (`BUTTON_HEIGHT`) `space::M` left of the
    // turn button (`BUTTON_WIDTH`) at `DOCK_RIGHT` from the right safe area.
    let dock_left = window.size.width
        - window.safe_area.right
        - DOCK_RIGHT
        - size::BUTTON_WIDTH
        - space::M
        - size::BUTTON_HEIGHT;
    let half_room = dock_left - space::M - window.size.width * 0.5;
    // A tilted outer card's bounding box extends ~0.6 card widths beyond its centre.
    let max_room = (2.0 * (half_room / card_width - 0.6)).max(0.0);
    hand.room.min(max_room)
}

fn stack(data: anchored::Stack) -> StackData {
    StackData {
        depth: data.depth,
        step_depth: data.step_depth,
        height: data.height,
        right: data.right,
        foot: data.foot,
        top: data.top,
        step: data.step,
        reach: data.reach,
        lean: data.lean,
        spare: data.spare,
        least_scale: data.least_scale,
    }
}
