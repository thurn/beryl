//! Battlefield row regions cropped around the stack, each player's life total and the shared
//! piles.

use beryl_protocol::protocol::{RowName, Side};
use glam::{Vec2, Vec3};
use kit_base::geometry::Rect;
use kit_base::tokens::size;
use kit_scene::hit::CELL;

use super::{Camera, Layout, RowArea, stack};

/// The area of the battlefield row `row` on `side`'s half of the table, and its height on the
/// table. As the web interface ends the permanents' rows left of the stack (`ROOM.end` in
/// `ui/src/game/tableRows.ts`), a row that the stack's band of the screen covers so far that its
/// cards keep no uncovered strip the reachable check's square high above or below it ends a
/// `layout.gap` left of the stack cascade's reserved rectangle. The reservation is
/// also present when the stack is empty, so casting a spell does not move the row. A row
/// keeps its whole area if it has room above or below the stack, or if the cut would leave
/// too little room for a tapped card. The two parts of the back row also end clear of their
/// player's life total, and on a shared-deck table every row starts clear of the shared piles.
pub fn row_area((layout, camera): (&Layout, &Camera), side: Side, row: RowName) -> (RowArea, f32) {
    // The desktop layout fitted to a window has each side's rows where the web interface draws
    // them, already clear of the stack's column and the life orbs (`ROOM` and `ROWS` in
    // `ui/src/game/tableRows.ts`).
    if let Some(rows) = &layout.fitted {
        let area = rows[super::place::side_index(side)][super::place::row_index(row)].area;
        return (area, area.y);
    }
    let (area, y) = left_of_stack((layout, camera), side, row);
    let area = clear_of_life((layout, camera), side, row, (area, y));
    (right_of_shared_piles((layout, camera), (area, y)), y)
}

/// `area`, a row at height `y` on the table, started a `layout.gap` right of the shared piles'
/// column where the row's band of the screen meets the column's. The shared library, graveyard
/// and exile stand in one column at the table's left edge, so on a shared-deck table the rows
/// beside it give up that part of the table. A row clear of the column's height, and every row
/// of a table with per-player piles, keeps its area. The row keeps room for one tapped card.
fn right_of_shared_piles(
    (layout, camera): (&Layout, &Camera),
    (area, y): (RowArea, f32),
) -> RowArea {
    if !layout.shape.is_shared() {
        return area;
    }
    let card = camera.data.card;
    let column = layout.shared_piles_bounds(camera);
    let (right, top, bottom) = (column.x + column.width, column.y, column.y + column.height);
    // The band a tapped card of the row lies across or an untapped one stands across, the
    // taller of the two.
    let reach = card.height.max(card.width) / 2.0;
    let height = |along: f32| camera.to_screen(Vec3::new(0.0, y + along, 0.0)).y;
    let (near, far) = (height(-reach), height(reach));
    if near.max(far) < top || near.min(far) > bottom {
        return area;
    }
    // Along a line of the table at one height, a point's place across the screen is linear in
    // its x. Of the two ends of a card's left edge, the one further left on screen meets the
    // column first.
    let left = [-reach, reach]
        .map(|along| {
            let point = Vec3::new(0.0, y + along, 0.0);
            (right - camera.to_screen(point).x) / camera.points_per_millimetre(point)
        })
        .into_iter()
        .fold(f32::NEG_INFINITY, f32::max);
    RowArea {
        left: area
            .left
            .max(left + layout.gap)
            .min(area.right - card.height),
        ..area
    }
}

/// `area`, the lands' or other permanents' part of `side`'s back row at height `y` on the table,
/// ended clear of `side`'s life orb and the step stops about it where the row's band of the
/// screen meets the orb's, as the web interface ends the lands left of the stops and starts the
/// other permanents right of them (`ROWS` in `ui/src/game/tableRows.ts`). The orb and stops
/// stand at a fixed place on the screen while the table's rows scale with the window, so on a
/// window the rows' table limits alone would run the lands and a planeswalker under the orb.
/// The creatures' row and a row clear of the orb's height keep their area.
fn clear_of_life(
    (layout, camera): (&Layout, &Camera),
    side: Side,
    row: RowName,
    (area, y): (RowArea, f32),
) -> RowArea {
    if layout.life_dock.is_some() || row == RowName::Creatures {
        return area;
    }
    let window = camera.window.size;
    let anchor = layout.players[usize::from(side == Side::Opponent)].life;
    let life = Vec2::new(
        f32::midpoint(anchor.x, 1.0) * window.width,
        f32::midpoint(1.0, -anchor.y) * window.height,
    );
    let half_height = size::ORB_HEIGHT * layout.play.life.orb_scale / 2.0;
    let half_width = (size::ORB_WIDTH * layout.play.life.orb_scale).max(size::STEPS_WIDTH) / 2.0;
    // The band an untapped card of the row stands across, the taller of its two.
    let height = |along: f32| camera.to_screen(Vec3::new(0.0, y + along, 0.0)).y;
    let (near, far) = (
        height(-camera.data.card.height / 2.0),
        height(camera.data.card.height / 2.0),
    );
    if near.max(far) < life.y - half_height || near.min(far) > life.y + half_height {
        return area;
    }
    // A card's edge runs the length of an untapped card, and of its two ends the one nearer
    // the orb on screen meets it first.
    let table_x = |screen_x: f32| {
        [
            -camera.data.card.height / 2.0,
            camera.data.card.height / 2.0,
        ]
        .map(|along| {
            let point = Vec3::new(0.0, y + along, 0.0);
            (screen_x - camera.to_screen(point).x) / camera.points_per_millimetre(point)
        })
    };
    let mut area = area;
    if row == RowName::Lands {
        let [a, b] = table_x(life.x - half_width - layout.life_clearance);
        area.right = area
            .right
            .min(a.min(b))
            .max(area.left + camera.data.card.height);
    } else {
        let [a, b] = table_x(life.x + half_width + layout.life_clearance);
        area.left = area
            .left
            .max(a.max(b))
            .min(area.right - camera.data.card.height);
    }
    area
}

/// The area of `side`'s `row` and its height on the table, ended left of the stack as
/// `row_area` says.
fn left_of_stack((layout, camera): (&Layout, &Camera), side: Side, row: RowName) -> (RowArea, f32) {
    let area = match row {
        RowName::Creatures => layout.creatures,
        RowName::Lands => layout.lands,
        RowName::Others => layout.others,
    };
    let y = match side {
        Side::You => area.y,
        Side::Opponent => -area.y,
    };
    let Some((stack_min, stack_max)) = stack::reservation(layout, camera) else {
        return (area, y);
    };
    // The band a tapped card of the row lies across, the narrower of its two.
    let height = |along: f32| camera.to_screen(Vec3::new(0.0, y + along, 0.0)).y;
    let (near, far) = (
        height(-camera.data.card.width / 2.0),
        height(camera.data.card.width / 2.0),
    );
    let uncovered = (stack_min.y - near.min(far)).max(near.max(far) - stack_max.y);
    // The reachable check finds an uncovered square only in whole cells of its grid.
    if uncovered >= layout.target + 2.0 * CELL {
        return (area, y);
    }
    // Along a line of the table at one height, a point's place across the screen is linear in
    // its x. A card's right edge runs the length of an untapped card, and the end of it that
    // stands further right on screen meets the stack's reach first.
    let right = [
        -camera.data.card.height / 2.0,
        camera.data.card.height / 2.0,
    ]
    .map(|along| {
        let point = Vec3::new(0.0, y + along, 0.0);
        (stack_min.x - camera.to_screen(point).x) / camera.points_per_millimetre(point)
    })
    .into_iter()
    .fold(f32::INFINITY, f32::min);
    let end = right - layout.gap;
    // On a narrow window, such as the phone's portrait one, the stack's column stands over the
    // rows rather than beside them, and its reach lies left of where the row starts. Cutting
    // the row there would leave it no room, so the row keeps its whole area and the stack is
    // drawn over it.
    if end < area.left + camera.data.card.height {
        return (area, y);
    }
    let area = RowArea {
        right: area.right.min(end),
        ..area
    };
    (area, y)
}

impl Layout {
    /// The box on the screen, in logical points, that the bottom cards of the three shared piles
    /// cover together. A pile's cards stand a little higher than its bottom card, so a control
    /// that keeps clear of this box keeps a small gap from it as well.
    #[must_use]
    pub fn shared_piles_bounds(&self, camera: &Camera) -> Rect {
        let card = camera.data.card;
        let half = Vec2::new(card.width, card.height) * self.shared_pile_scale / 2.0;
        let corners = self.shared_piles.iter().flat_map(|&anchor| {
            let base = camera.on_table(anchor);
            [-1.0, 1.0].into_iter().flat_map(move |across: f32| {
                [-1.0, 1.0].map(|along: f32| {
                    camera.to_screen((base + half * Vec2::new(across, along)).extend(0.0))
                })
            })
        });
        let (low, high) = corners.fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(low, high), corner| (low.min(corner), high.max(corner)),
        );
        Rect {
            x: low.x,
            y: low.y,
            width: high.x - low.x,
            height: high.y - low.y,
        }
    }
}
