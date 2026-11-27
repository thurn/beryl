use beryl_protocol::protocol::{RowName, Side};
use game_table::table::{HudAnchor, ROWS, Rows, Scroller, Table};
use glam::Vec2;
use kit_base::geometry::Rect;
use kit_frame::frame::Anchor;
use kit_ui::element::{Element, FloatId, floats, text};

/// The number of battlefield row ends. The hand and stack follow these.
const ROW_ENDS: usize = 12;

/// The edge of the scroller's window that a count labels, clamped inside the window's horizontal
/// inset (the banded side inset on phones, or the safe area on desktop) so badges at the outer
/// ends of crowded rows do not clip off-screen.
pub(super) fn anchor(scroller: Scroller, right: bool, table: &Table) -> Anchor {
    let gap = table.profile().data().table.row_count_gap;
    let end = if right { 1.0 } else { 0.0 };
    let (at, align, offset) = match scroller {
        Scroller::Hand => (Vec2::new(end, 0.0), Vec2::new(end, 1.0), -Vec2::Y * gap),
        Scroller::Stack if table.layout().centred_stack_line().is_none() => (
            Vec2::new(1.0, end),
            Vec2::new(1.0, 1.0 - end),
            Vec2::Y * if right { gap } else { -gap },
        ),
        _ => (
            Vec2::new(end, 0.5),
            Vec2::new(1.0 - end, 0.5),
            Vec2::X * if right { gap } else { -gap },
        ),
    };
    let window = table.camera().window;
    let (left, right) = table.layout().bands.map_or(
        (
            window.safe_area.left,
            window.size.width - window.safe_area.right,
        ),
        |bands| (bands.left, bands.right),
    );
    let top = window.safe_area.top;
    let bottom = window.size.height - window.safe_area.bottom;
    HudAnchor::Row(scroller)
        .at(at, align, offset)
        .clamped(Rect {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        })
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
