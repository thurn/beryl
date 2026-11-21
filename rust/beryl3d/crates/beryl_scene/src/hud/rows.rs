//! The number of cards scrolled off each end of a battlefield row, the viewer's hand or the
//! stack, shown as a float of the flat layer by that end, as "+12" (`floats::ROW_COUNT`,
//! `floats::HAND_COUNT` and `floats::STACK_COUNT`; docs/beryl-3d.md, "Floats"). A row's count
//! stands just outside its end, and the stack's in the room the phone keeps beside its row. The
//! hand's stand over the hand's top corners, since a hand that scrolls fills the window from
//! side to side. A line with no cards off an end shows no count there. Arena's recordings show
//! no board large enough to scroll, so this is listed among the behaviors not seen in Arena.
//!
//! A count changes when a row scrolls or when the table takes the view a script leads to. Every
//! count the table shows now or will show at the end of a script still to play is laid out
//! ahead, when a view arrives or a row scrolls (`RowCounts::prepare`), so that a frame only
//! chooses which laid-out element each float shows and sets its look, and allocates nothing.

use beryl_protocol::protocol::{RowName, Side};
use glam::{Vec2, Vec3};

use super::counts::{index, points};
use crate::art::orb;
use crate::element::{Element, FloatId, floats, text};
use crate::layout::{FloatLook, Layer};
use crate::table::{Camera, Layout, ROWS, Rows, Scroller, Table, row_area};
use kit_base::data::space::layout::desktop::ROW_COUNT_GAP;
use kit_base::tokens::{size, type_style};

/// Each side has three rows, and with the hand and the stack each has two ends.
const COUNTS: usize = 16;

/// The first of the battlefield rows' ends that are not; the hand's two come before the
/// stack's.
const ROW_ENDS: usize = 12;

/// The floats that show each row end's count, and the count each shows now.
#[derive(Debug, Default)]
pub struct RowCounts {
    shown: [Option<usize>; COUNTS],
    /// Each row end's index and a count it will show, kept between calls so that `prepare`
    /// allocates nothing once it has grown.
    coming: Vec<(usize, usize)>,
}

impl RowCounts {
    /// Lays out ahead, in `layer`, each count that `rows` give a row's end, and forgets those
    /// laid out before that none of them gives. The client calls it when a view arrives, when a
    /// row scrolls and when the window changes shape.
    pub fn prepare<'a>(&mut self, layer: &mut Layer, rows: impl Iterator<Item = &'a Rows>) {
        let coming = &mut self.coming;
        coming.clear();
        for rows in rows {
            for index in 0..COUNTS {
                let count = count_in(rows, index);
                if count > 0 && !coming.contains(&(index, count)) {
                    coming.push((index, count));
                }
            }
        }
        for index in 0..COUNTS {
            let id = float(index);
            layer.forget_prepared(id, |key| coming.contains(&(index, count_of(key))));
            for &(_, count) in coming.iter().filter(|(end, _)| *end == index) {
                layer.prepare(id, key_of(count), || count_text(index, count));
            }
        }
    }

    /// Shows each row end's count as `table` shows the view at rest, or none unless `visible`.
    /// Lays nothing out and allocates nothing for what `prepare` laid out.
    pub fn write(&mut self, layer: &mut Layer, table: &Table, visible: bool) {
        let layout = table.layout();
        let camera = table.camera();
        for index in 0..COUNTS {
            let (scroller, right) = end_of(index);
            let want = Some(count_in(table.rows(), index)).filter(|&count| visible && count > 0);
            let id = float(index);
            if want != self.shown[index] {
                if !layer.show_prepared(id, want.map(key_of)) {
                    debug_assert!(false, "no count {want:?} is prepared for row end {index}");
                    layer.float(id, want.map(|count| count_text(index, count)));
                }
                self.shown[index] = want;
            }
            if want.is_none() {
                continue;
            }
            let Some((point, anchor)) = place(scroller, right, (layout, camera), table.rows())
            else {
                continue;
            };
            layer.set_float_look(
                id,
                FloatLook {
                    point,
                    anchor,
                    ..FloatLook::default()
                },
            );
        }
    }
}

/// Where the count of the cards off `scroller`'s `right` or left end stands, in logical points,
/// and the anchor of the float there, as `layout` and `camera` show `rows`. `None` for a line
/// the layout gives no room for a count.
fn place(
    scroller: Scroller,
    right: bool,
    (layout, camera): (&Layout, &Camera),
    rows: &Rows,
) -> Option<(Vec2, Vec2)> {
    // Beside the end, on the line's middle: the left count ends a gap before the line starts,
    // and the right count starts a gap after it ends.
    let beside = |left: f32, right_end: f32, y: f32| {
        Some(if right {
            (Vec2::new(right_end + ROW_COUNT_GAP, y), Vec2::new(0.0, 0.5))
        } else {
            (Vec2::new(left - ROW_COUNT_GAP, y), Vec2::new(1.0, 0.5))
        })
    };
    match scroller {
        Scroller::Row(side, row) => {
            let (area, y) = row_area((layout, camera), side, row);
            let [left, right_end] =
                [area.left, area.right].map(|x| camera.to_screen(Vec3::new(x, y, 0.0)));
            let (mut point, anchor) = beside(left.x, right_end.x, left.y)?;
            if layout.phone.is_none() {
                // A crowded lands row can end at the life orb. Keep its count outside
                // the orb and step stops, rather than printing it through that furniture.
                let life = points(layout.players[index(side)].life, camera.window.size);
                let half = Vec2::new(size::ORB_WIDTH, size::ORB_HEIGHT) * orb::DESKTOP_SCALE / 2.0;
                let half_line = type_style::BODY.size * type_style::BODY.line_height / 2.0;
                if (point.y - life.y).abs() < half.y + half_line {
                    let half_width = size::STEPS_WIDTH / 2.0;
                    point.x = if right {
                        point.x.max(life.x + half_width + ROW_COUNT_GAP)
                    } else {
                        point.x.min(life.x - half_width - ROW_COUNT_GAP)
                    };
                }
            }
            Some((point, anchor))
        }
        Scroller::Stack => {
            let data = layout.phone?;
            let shown = rows.stack.shown().max(1);
            #[expect(
                clippy::cast_precision_loss,
                reason = "a stack holds far fewer than 2^24 items"
            )]
            let half = (shown as f32 - 1.0) / 2.0 * data.step + data.width() / 2.0;
            let middle = f32::midpoint(data.left, data.right);
            beside(middle - half, middle + half, data.middle)
        }
        Scroller::Hand => {
            // Over the corners of the column at the hand's top edge, inside it.
            let window = camera.window.size;
            let y = (1.0 - layout.hand.edge) / 2.0 * window.height - ROW_COUNT_GAP;
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
fn end_of(index: usize) -> (Scroller, bool) {
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
fn count_in(rows: &Rows, index: usize) -> usize {
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
fn float(index: usize) -> FloatId {
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
fn key_of(count: usize) -> u64 {
    count as u64
}

/// The count a key was prepared under.
fn count_of(key: u64) -> usize {
    usize::try_from(key).unwrap_or(usize::MAX)
}

/// The count of the line end at `index`: text that shows "+" and the number, named for the
/// line, the end and the number, as "Your creatures right 12", "Your hand left 3" or "Stack
/// right 2", so that the tree's line says all three.
fn count_text(index: usize, count: usize) -> Element {
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
