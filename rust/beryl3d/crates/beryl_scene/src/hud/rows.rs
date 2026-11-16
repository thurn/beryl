//! The number of cards scrolled off each end of a battlefield row, shown as a float of the flat
//! layer just outside that end of the row, as "+12" (`floats::ROW_COUNT`; docs/beryl-3d.md,
//! "Floats"). A row with no cards off an end shows no count there. Arena's recordings show no
//! board large enough to scroll, so this is listed among the behaviors not seen in Arena.
//!
//! A count changes when a row scrolls or when the table takes the view a script leads to. Every
//! count the table shows now or will show at the end of a script still to play is laid out
//! ahead, when a view arrives or a row scrolls (`RowCounts::prepare`), so that a frame only
//! chooses which laid-out element each float shows and sets its look, and allocates nothing.

use beryl_protocol::protocol::{RowName, Side};
use glam::{Vec2, Vec3};

use crate::data::space::layout::desktop::ROW_COUNT_GAP;
use crate::element::{Element, FloatId, floats, text};
use crate::layout::{FloatLook, Layer};
use crate::table::{ROWS, Rows, Table, row_area};

/// Each side has three rows, each with two ends.
const COUNTS: usize = 12;

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
            let (side, row, right) = end_of(index);
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
            let (area, y) = row_area((layout, camera), side, row);
            let x = if right { area.right } else { area.left };
            let end = camera.to_screen(Vec3::new(x, y, 0.0));
            // Beside the end, on the row's middle line: the left count ends a gap before the
            // row starts, and the right count starts a gap after it ends.
            let (point, anchor) = if right {
                (end + Vec2::new(ROW_COUNT_GAP, 0.0), Vec2::new(0.0, 0.5))
            } else {
                (end - Vec2::new(ROW_COUNT_GAP, 0.0), Vec2::new(1.0, 0.5))
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

/// The row end at `index`: the side, the row and whether it is the right end. The viewer's rows
/// come first, each in the order of `ROWS`, the left end before the right.
fn end_of(index: usize) -> (Side, RowName, bool) {
    let side = if index < COUNTS / 2 {
        Side::You
    } else {
        Side::Opponent
    };
    (side, ROWS[index % 6 / 2], index % 2 == 1)
}

/// How many cards `rows` has off the row end at `index`.
fn count_in(rows: &Rows, index: usize) -> usize {
    let window = rows[index / 6][index % 6 / 2];
    if index % 2 == 1 {
        window.after()
    } else {
        window.before()
    }
}

/// The float that shows the count of the row end at `index`. `floats::ROW_COUNT` orders the rows
/// as `RowName` does, which `ROWS` does not.
fn float(index: usize) -> FloatId {
    let (side, row, right) = end_of(index);
    let row = match row {
        RowName::Creatures => 0,
        RowName::Others => 1,
        RowName::Lands => 2,
    };
    floats::ROW_COUNT[usize::from(side == Side::Opponent)][row][usize::from(right)]
}

/// The key a count is prepared under.
fn key_of(count: usize) -> u64 {
    count as u64
}

/// The count a key was prepared under.
fn count_of(key: u64) -> usize {
    usize::try_from(key).unwrap_or(usize::MAX)
}

/// The count of the row end at `index`: text that shows "+" and the number, named for the row,
/// the end and the number, as "Your creatures right 12", so that the tree's line says all three.
fn count_text(index: usize, count: usize) -> Element {
    let (side, row, right) = end_of(index);
    let whose = match side {
        Side::You => "Your",
        Side::Opponent => "Opponent's",
    };
    let row = match row {
        RowName::Creatures => "creatures",
        RowName::Lands => "lands",
        RowName::Others => "others",
    };
    let end = if right { "right" } else { "left" };
    let mut element = text(&format!("+{count}"));
    element.name = format!("{whose} {row} {end} {count}");
    element
}
