use beryl_protocol::protocol::{RowName, Side};
use glam::{Vec2, Vec3};

use game_play::hud::counts::{index, points};
use game_table::table::{ROWS, Rows, Scroller, Table, row_area};
use kit_base::data::profiles::Piece;
use kit_base::tokens::{size, type_style};
use kit_ui::element::{Element, FloatId, floats, text};

/// The number of battlefield row ends. The hand and stack follow these.
const ROW_ENDS: usize = 12;

/// Where the count of the cards off `scroller`'s `right` or left end stands, in logical points,
/// and the anchor of the float there, for the table at rest.
pub(super) fn place(scroller: Scroller, right: bool, table: &Table) -> Option<(Vec2, Vec2)> {
    let layout = table.layout();
    let camera = table.camera();
    let profile = table.profile().data();
    let gap = profile.table.row_count_gap;
    // Beside the end, on the line's middle: the left count ends a gap before the line starts,
    // and the right count starts a gap after it ends.
    let beside = |left: f32, right_end: f32, y: f32| {
        Some(if right {
            (Vec2::new(right_end + gap, y), Vec2::new(0.0, 0.5))
        } else {
            (Vec2::new(left - gap, y), Vec2::new(1.0, 0.5))
        })
    };
    match scroller {
        Scroller::Row(side, row) => {
            let (area, y) = row_area((layout, camera), side, row);
            let [left, right_end] =
                [area.left, area.right].map(|x| camera.to_screen(Vec3::new(x, y, 0.0)));
            let (mut point, anchor) = beside(left.x, right_end.x, left.y)?;
            if profile.hud.table.contains(&Piece::StepStops) {
                // A crowded lands row can end at the life orb. Keep its count outside
                // the orb and step stops, rather than printing it through that furniture.
                let life = points(layout.players[index(side)].life, camera.window.size);
                let half =
                    Vec2::new(size::ORB_WIDTH, size::ORB_HEIGHT) * layout.play.life.orb_scale / 2.0;
                let half_line = type_style::BODY.size * type_style::BODY.line_height / 2.0;
                if (point.y - life.y).abs() < half.y + half_line {
                    let half_width = size::STEPS_WIDTH / 2.0;
                    point.x = if right {
                        point.x.max(life.x + half_width + gap)
                    } else {
                        point.x.min(life.x - half_width - gap)
                    };
                }
            }
            Some((point, anchor))
        }
        Scroller::Stack => {
            let (low, high) = table.stack_bounds()?;
            if layout.centred_stack_line().is_some() {
                beside(low.x, high.x, f32::midpoint(low.y, high.y))
            } else if right {
                Some((Vec2::new(high.x, high.y + gap), Vec2::new(1.0, 0.0)))
            } else {
                Some((Vec2::new(high.x, low.y - gap), Vec2::ONE))
            }
        }
        Scroller::Hand => {
            // Over the corners of the column at the hand's top edge, inside it.
            let window = camera.window.size;
            let y = (1.0 - layout.hand.edge) / 2.0 * window.height - gap;
            let (left, right_end) = layout
                .bands
                .map_or((0.0, window.width), |bands| (bands.left, bands.right));
            Some(if right {
                (Vec2::new(right_end, y), Vec2::new(1.0, 1.0))
            } else {
                (Vec2::new(left, y), Vec2::new(0.0, 1.0))
            })
        }
    }
}

/// The line end at `index`: the line, and whether it is the right end. The viewer's rows come
/// first, each in the order of `ROWS`, then the opponent's, then the hand and the stack, the
/// left end before the right.
pub(super) fn end_of(index: usize) -> (Scroller, bool) {
    let scroller = match index / 2 {
        6 => Scroller::Hand,
        7 => Scroller::Stack,
        _ => {
            let side = if index < ROW_ENDS / 2 {
                Side::You
            } else {
                Side::Opponent
            };
            Scroller::Row(side, ROWS[index % 6 / 2])
        }
    };
    (scroller, index % 2 == 1)
}

/// How many cards `rows` has off the line end at `index`.
pub(super) fn count_in(rows: &Rows, index: usize) -> usize {
    let (scroller, right) = end_of(index);
    let window = rows.get(scroller);
    if right {
        window.after()
    } else {
        window.before()
    }
}

/// The float that shows the count of the line end at `index`. `floats::ROW_COUNT` orders the
/// rows as `RowName` does, which `ROWS` does not.
pub(super) fn float(index: usize) -> FloatId {
    let (scroller, right) = end_of(index);
    let right = usize::from(right);
    match scroller {
        Scroller::Row(side, row) => {
            let row = match row {
                RowName::Creatures => 0,
                RowName::Others => 1,
                RowName::Lands => 2,
            };
            floats::ROW_COUNT[usize::from(side == Side::Opponent)][row][right]
        }
        Scroller::Hand => floats::HAND_COUNT[right],
        Scroller::Stack => floats::STACK_COUNT[right],
    }
}

/// The key a count is prepared under.
pub(super) fn key_of(count: usize) -> u64 {
    count as u64
}

/// The count a key was prepared under.
pub(super) fn count_of(key: u64) -> usize {
    usize::try_from(key).unwrap_or(usize::MAX)
}

/// The count of the line end at `index`: text that shows "+" and the number, named for the
/// line, the end and the number, as "Your creatures right 12", "Your hand left 3" or "Stack
/// right 2", so that the tree's line says all three.
pub(super) fn count_text(index: usize, count: usize) -> Element {
    let (scroller, right) = end_of(index);
    let line = match scroller {
        Scroller::Row(side, row) => {
            let whose = match side {
                Side::You => "Your",
                Side::Opponent => "Opponent's",
            };
            let row = match row {
                RowName::Creatures => "creatures",
                RowName::Lands => "lands",
                RowName::Others => "others",
            };
            format!("{whose} {row}")
        }
        Scroller::Hand => "Your hand".to_owned(),
        Scroller::Stack => "Stack".to_owned(),
    };
    let end = if right { "right" } else { "left" };
    // It lets the pointer through to the cards under it.
    let mut element = text(&format!("+{count}")).passes(true);
    element.name = format!("{line} {end} {count}");
    element
}
