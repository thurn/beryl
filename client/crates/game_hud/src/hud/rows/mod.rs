//! The number of cards scrolled off each end of a battlefield row, the viewer's hand or the
//! stack, shown as a float of the flat layer by that end, as "+12" (`floats::ROW_COUNT`,
//! `floats::HAND_COUNT` and `floats::STACK_COUNT`; docs/beryl-3d.md, "Floats"). A row's count
//! stands just outside its end. The stack's counts sit outside its visible window. The
//! hand's stand over the hand's top corners, since a hand that scrolls fills the window from
//! side to side. A line with no cards off an end shows no count there. Arena's recordings show
//! no board large enough to scroll, so this is listed among the behaviors not seen in Arena.
//!
//! A count changes when a row scrolls or when the table takes the view a script leads to. Every
//! count the table shows now or will show at the end of a script still to play is laid out
//! ahead, when a view arrives or a row scrolls (`RowCounts::prepare`), so that a frame only
//! chooses which laid-out element each float shows and sets its look, and allocates nothing.

use game_table::table::{Rows, Scroller, Table};

use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, FloatId};
use std::convert::Infallible;

mod view;
use kit_ui::layout::{FloatLook, Layer};
use view::{anchor, count_in, count_of, count_text, end_of, float, key_of};

/// Each side has three rows, and with the hand and the stack each has two ends.
const COUNTS: usize = 16;

/// The floats that show each row end's count, and the count each shows now.
#[derive(Debug, Default)]
pub struct RowCounts {
    shown: [Option<usize>; COUNTS],
    /// The row most recently given keyboard focus.
    focus: Option<Scroller>,
    /// Each row end's index and a count it will show, kept between calls so that `prepare`
    /// allocates nothing once it has grown.
    coming: Vec<(usize, usize)>,
}

/// A keyboard focus change from a row press or a change to the active screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Focus(Option<Scroller>),
}

/// The row end index and number of cards offscreen beyond it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Inputs {
    pub index: usize,
    pub count: usize,
}

impl Feature for RowCounts {
    const SOURCE: &'static str = file!();

    type State = Self;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        state: &mut Self,
        message: &Message,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        let Message::Focus(row) = *message;
        state.focus = row;
        // Focus belongs to the keyboard route and does not change a count label.
        false
    }

    fn view_key(_: &Self, input: &Inputs) -> Inputs {
        *input
    }

    fn view(_: &Self, input: Inputs) -> Element {
        count_text(input.index, input.count)
    }
}

impl super::FloatFeature for RowCounts {
    fn floats() -> impl Iterator<Item = FloatId> {
        (0..COUNTS).map(float)
    }
}

/// Prepares the labels the current and upcoming row positions can show.
pub struct Prepare<'a, Coming> {
    pub layer: &'a mut Layer,
    pub rows: Coming,
}

impl<'a, Coming: Iterator<Item = &'a Rows>> StateUpdate<RowCounts> for Prepare<'_, Coming> {
    type Output = ();

    fn apply(self, state: &mut RowCounts) -> (bool, ()) {
        state.prepare(self.layer, self.rows);
        (false, ())
    }
}

/// Samples the prepared labels for the rows at rest on the table.
pub struct Draw<'a> {
    pub layer: &'a mut Layer,
    pub table: &'a Table,
    pub visible: bool,
}

impl StateUpdate<RowCounts> for Draw<'_> {
    type Output = ();

    fn apply(self, state: &mut RowCounts) -> (bool, ()) {
        state.write(self.layer, self.table, self.visible);
        (false, ())
    }
}

impl RowCounts {
    #[must_use]
    pub fn focus(&self) -> Option<Scroller> {
        self.focus
    }

    /// Lays out ahead, in `layer`, each count that `rows` give a row's end, and forgets those
    /// laid out before that none of them gives. The client calls it when a view arrives, when a
    /// row scrolls and when the window changes shape.
    fn prepare<'a>(&mut self, layer: &mut Layer, rows: impl Iterator<Item = &'a Rows>) {
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
            layer.forget_prepared(id, |key| self.coming.contains(&(index, count_of(key))));
            for &(_, count) in self.coming.iter().filter(|(end, _)| *end == index) {
                layer.prepare(id, key_of(count), || {
                    Self::view(self, Inputs { index, count })
                });
            }
        }
    }

    /// Shows each row end's count as `table` shows the view at rest, or none unless `visible`.
    /// Lays nothing out and allocates nothing for what `prepare` laid out.
    fn write(&mut self, layer: &mut Layer, table: &Table, visible: bool) {
        for index in 0..COUNTS {
            let (scroller, right) = end_of(index);
            let want = Some(count_in(table.rows(), index)).filter(|&count| visible && count > 0);
            let id = float(index);
            if want != self.shown[index] {
                if !layer.show_prepared(id, want.map(key_of)) {
                    debug_assert!(false, "no count {want:?} is prepared for row end {index}");
                    layer.float(
                        id,
                        want.map(|count| Self::view(self, Inputs { index, count })),
                    );
                }
                self.shown[index] = want;
            }
            if want.is_none() {
                continue;
            }
            layer.anchor_float(id, anchor(scroller, right, table), FloatLook::default());
        }
    }
}
