//! The life orbs as the web interface draws them (`LifeOrb` in `ui/src/components/Player.tsx`),
//! in a desktop window at two pixels to a point, as on the Retina display the owner reported
//! them from: each orb draws the layers of the web's art, its ring toward its player's edge of
//! the table and the active player's flare, with no panel behind it or its life total, and the
//! step stops stand clear of it as the web's grid places them.

use app_client::Input;
use beryl_protocol::protocol::Side;
use game_hud::art::orb::{self, LAYERS};
use glam::{Vec2, Vec4};
use kit_frame::frame::{Frame, Material, Role, StateBits};
use kit_ui::input::{AssetReady, IconCell};

use super::flat_layer::{client, deciding, edges, find, prompt};

/// A cell for every layer of the art at every scale, each at its own column of page 1, so that
/// a draw's `uv` names the layer it draws.
fn orb_cells() -> Vec<IconCell> {
    orb::atlas_cells()
        .enumerate()
        .map(|(index, layer)| layer.cell([u16::try_from(index).unwrap(), 0, 1, 1]))
        .collect()
}

/// The middle of a flat-layer draw on the screen, in logical points.
fn screen_centre(frame: &Frame, index: usize, window: Vec2) -> Vec2 {
    let model = &frame.draws[index].model;
    let clip = frame.view_projection * Vec4::new(model[3], model[7], model[11], 1.0);
    let ndc = Vec2::new(clip.x, clip.y) / clip.w;
    Vec2::new(
        f32::midpoint(ndc.x, 1.0) * window.x,
        f32::midpoint(1.0, -ndc.y) * window.y,
    )
}

/// The layers of `side`'s desktop orb that `frame` draws, by index in `LAYERS`, in the order
/// drawn, each with its middle on the screen.
fn drawn(frame: &Frame, side: Side, window: Vec2) -> Vec<(usize, Vec2)> {
    // The desktop's cells come first, the viewer's orb's before the opponent's.
    let first = usize::from(side == Side::Opponent) * LAYERS.len();
    (frame.layer as usize..frame.draws.len())
        .filter(|&index| frame.draws[index].material == Material::Icon as u8)
        .filter(|&index| frame.draws[index].layer == 1)
        .filter_map(|index| {
            let column = usize::from(frame.draws[index].uv[0]);
            (first..first + LAYERS.len())
                .contains(&column)
                .then(|| (column - first, screen_centre(frame, index, window)))
        })
        .collect()
}

#[test]
fn each_orb_draws_the_webs_art_with_its_ring_toward_its_edge() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.handle(Input::Assets(AssetReady::Icons {
        page: 1,
        cells: orb_cells(),
    }));
    let frame = client.frame(0);
    let size = Vec2::new(1440.0, 900.0);
    let core = LAYERS
        .iter()
        .position(|layer| layer.shape == orb::Shape::Core && layer.paint.stroke.is_none())
        .expect("a core layer");
    let ring = LAYERS
        .iter()
        .rposition(|layer| matches!(layer.shape, orb::Shape::Rim { offset, .. } if offset > 0.0))
        .expect("a ring layer");
    let mut cores = Vec::new();
    for (side, name) in [(Side::You, "your orb"), (Side::Opponent, "opponent's orb")] {
        let element = find(frame, Role::Region, name);
        let active = frame.resolved[element].state.has(StateBits::SELECTED);
        let layers = drawn(frame, side, size);
        let want: Vec<usize> = (0..LAYERS.len())
            .filter(|&at| active || !LAYERS[at].active)
            .collect();
        assert_eq!(
            layers.iter().map(|&(at, _)| at).collect::<Vec<_>>(),
            want,
            "{name} draws every layer of its art in order"
        );
        let place = |at: usize| layers.iter().find(|&&(drawn, _)| drawn == at).unwrap().1;
        cores.push(place(core));
        // The ring lies toward the player's edge: below the viewer's core, above the
        // opponent's.
        let below = place(ring).y > place(core).y;
        assert_eq!(below, side == Side::You, "{name}'s ring is toward its edge");
    }
    assert!(cores[0].y > cores[1].y);

    // No panel is drawn behind either life total: the web draws none on its orb.
    let totals = (0..frame.resolved.len()).filter(|&at| frame.resolved[at].role == Role::Player);
    for total in totals {
        let name = frame.name(frame.resolved[total].name);
        let (left, top, right, bottom) = edges(frame, total);
        let middle = Vec2::new(f32::midpoint(left, right), f32::midpoint(top, bottom));
        let behind = (frame.layer as usize..frame.draws.len())
            .filter(|&index| frame.draws[index].material == Material::Panel as u8)
            .any(|index| screen_centre(frame, index, size).distance(middle) < 20.0);
        assert!(!behind, "a panel is drawn behind {name}");
    }

    // Each side's step stops end 70 points from the middle of the viewer's core, as the web's
    // grid, 308 wide, places them.
    let core_x = cores[0].x;
    let (_, _, inner_left, _) = edges(frame, find(frame, Role::Toggle, "Your draw"));
    let (inner_right, ..) = edges(frame, find(frame, Role::Toggle, "Your blockers"));
    assert!((core_x - inner_left - 70.0).abs() < 1.0, "{inner_left}");
    assert!((inner_right - core_x - 70.0).abs() < 1.0, "{inner_right}");
}
