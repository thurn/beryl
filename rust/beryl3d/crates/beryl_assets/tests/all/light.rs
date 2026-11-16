//! The shapes of light on the atlas's page 0, as `light_page` draws them.

use beryl_assets::light::{BORDER, light_page};
use beryl_assets::packer::PAGE_SIZE;
use beryl_scene::frame::{Sprite, sprites};

const ALL: [Sprite; 14] = [
    sprites::DISC,
    sprites::BURST,
    sprites::RING,
    sprites::CONE,
    sprites::COMET,
    sprites::RIBBON,
    sprites::OUTLINE,
    sprites::HEAD,
    sprites::WEDGE,
    sprites::CRESCENT,
    sprites::TAP,
    sprites::MOON,
    sprites::PLAQUE,
    sprites::COIN,
];

/// The opacity of texel `(x, y)` of `sprite`'s cell.
fn at(page: &[u8], sprite: Sprite, x: u16, y: u16) -> u8 {
    let [left, top, ..] = sprite.rect;
    page[usize::from(top + y) * usize::from(PAGE_SIZE) + usize::from(left + x)]
}

/// The texel at the middle of a cell.
const MIDDLE: u16 = sprites::CELL / 2;

#[test]
fn the_page_is_one_atlas_page_and_drawn_the_same_each_time() {
    let page = light_page();
    assert_eq!(page.len(), usize::from(PAGE_SIZE) * usize::from(PAGE_SIZE));
    assert_eq!(page, light_page());
}

#[test]
fn every_cell_has_a_shape_and_an_empty_border() {
    let page = light_page();
    let cell = sprites::CELL;
    for sprite in ALL {
        let mut inside = 0_u32;
        for y in 0..cell {
            for x in 0..cell {
                let border = x < BORDER || y < BORDER || x >= cell - BORDER || y >= cell - BORDER;
                let value = at(&page, sprite, x, y);
                if border {
                    assert_eq!(value, 0, "{sprite:?} at ({x}, {y})");
                } else {
                    inside += u32::from(value > 0);
                }
            }
        }
        assert!(inside > 100, "{sprite:?} draws {inside} texels");
    }
}

#[test]
fn the_disc_is_full_at_its_centre_and_never_brighter_farther_out() {
    let page = light_page();
    let disc = sprites::DISC;
    assert_eq!(at(&page, disc, MIDDLE, MIDDLE), 255);
    // Along each axis from the middle outward, the opacity never rises.
    for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1), (1, 1)] {
        let mut last = 255;
        // Steps that stay inside the drawn area on every side of the middle.
        for step in 0..i32::from(MIDDLE - BORDER) {
            let x = (i32::from(MIDDLE) + dx * step) as u16;
            let y = (i32::from(MIDDLE) + dy * step) as u16;
            let value = at(&page, disc, x, y);
            assert!(value <= last, "the disc rises to {value} at ({x}, {y})");
            last = value;
        }
    }
}

#[test]
fn the_ring_and_the_outline_are_empty_at_their_centres() {
    let page = light_page();
    assert_eq!(at(&page, sprites::RING, MIDDLE, MIDDLE), 0);
    assert_eq!(at(&page, sprites::OUTLINE, MIDDLE, MIDDLE), 0);
}

#[test]
fn the_outline_is_solid_on_its_edge() {
    let page = light_page();
    // Up the middle column from the centre, the outline's top edge reaches full opacity.
    let top_edge = (BORDER..MIDDLE)
        .map(|y| at(&page, sprites::OUTLINE, MIDDLE, y))
        .max();
    assert_eq!(top_edge, Some(255));
}

/// Where texel `(x, y)` of a cell lies on a unit quad drawn with the whole cell, from -0.5 to
/// 0.5 across, y up.
fn on_quad(x: u16, y: u16) -> (f32, f32) {
    let cell = f32::from(sprites::CELL);
    (
        (f32::from(x) + 0.5) / cell - 0.5,
        0.5 - (f32::from(y) + 0.5) / cell,
    )
}

#[test]
fn the_outline_is_empty_where_its_mesh_leaves_the_quad_out() {
    // A texel within two texels of a part the mesh leaves out is sampled there by bilinear
    // filtering, so it must be empty too.
    let page = light_page();
    let margin = 2.0 / f32::from(sprites::CELL);
    let [outer, hole] = [sprites::OUTLINE_OUTER, sprites::OUTLINE_HOLE];
    for y in 0..sprites::CELL {
        for x in 0..sprites::CELL {
            let (qx, qy) = on_quad(x, y);
            let outside = qx.abs() > outer[0] - margin || qy.abs() > outer[1] - margin;
            let inside = qx.abs() < hole[0] + margin && qy.abs() < hole[1] + margin;
            if outside || inside {
                assert_eq!(
                    at(&page, sprites::OUTLINE, x, y),
                    0,
                    "texel ({x}, {y}) is drawn, outside the outline's mesh"
                );
            }
        }
    }
}
