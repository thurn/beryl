//! An anchored table, optionally fitted to the web stage's projected rows and piles.

use kit_base::data::profiles::{Anchored, ProfileData, TrayTuning, anchored};
use kit_space::arrange::row::ScaleLimit;

use super::{row, web_fit};
use crate::table::{
    Across, HandData, Layout, PlayerAnchor, RowZoom, StackData, StackPlacement, TraySpace, Window,
};

/// The profile's table before any window-specific fitting.
pub(super) fn reference(profile: &ProfileData, data: &Anchored) -> Layout {
    let table = &profile.table;
    let piles = [data.piles.you, data.piles.opponent];
    Layout {
        camera: table.camera,
        strip: table.strip,
        creatures: row(table.creatures, true),
        lands: row(table.lands, false),
        others: row(table.others, false),
        gap: table.gap,
        pile_step: table.pile_step,
        beneath_step: table.beneath_step,
        piles: piles.map(|pile| [pile.library, pile.graveyard, pile.exile]),
        pile_scale: piles.map(|pile| pile.scale),
        tray: tray(data.tray, table.tray_tuning),
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
        row_zoom: None,
        bands: None,
        fitted: None,
        fitted_piles: None,
    }
}

pub(super) fn tray(data: anchored::Tray, tuning: TrayTuning) -> TraySpace {
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
        heading: data.heading,
        tuning,
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
    }
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
    }
}

/// Fits the rows and piles to their projected stage positions when there is a reference stage.
pub(super) fn layout(profile: &ProfileData, window: Window, data: &Anchored) -> Layout {
    let mut layout = reference(profile, data);
    if let Some(web) = &data.web {
        layout.row_zoom = Some(RowZoom {
            minimum: web.zoom_least,
            back: ScaleLimit {
                minimum: data.least_back_zoom,
                shared_minimum: web.back_row_pull,
            },
        });
        let camera = layout.fit(window);
        let piles = web_fit::piles(window, &camera, web);
        layout.fitted = Some(web_fit::rows(window, &camera, web));
        layout.fitted_piles = Some(piles);
        layout.pile_scale = piles.scale;
    }
    layout
}
