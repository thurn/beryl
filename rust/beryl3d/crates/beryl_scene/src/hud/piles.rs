//! The number of cards in each library, graveyard and exile pile, shown as a float of the flat
//! layer beside the pile's top card (`floats::PILE_COUNT`; docs/beryl-3d.md, "Floats"). The web
//! interface tells it on hover ("Graveyard · 5"), over the viewer's piles and under the
//! opponent's (`PileButton` in `ui/src/game/Piles.tsx`); here it stands there always, since a
//! pile cannot be hovered yet. An empty pile shows none.
//!
//! A pile's count changes as the table takes the view a script leads to. Every count the scripts
//! to come will show is laid out ahead, as each update arrives (`PileCounts::prepare`), so that
//! a frame only chooses which laid-out element each float shows and sets its look, and
//! allocates nothing.

use beryl_protocol::protocol::{PlayerView, Side, ZoneName};
use glam::Vec2;

use crate::data::space::layout::desktop::piles::COUNT_GAP;
use crate::element::{Element, floats, text};
use crate::layout::{FloatLook, Layer};
use crate::table::{PileShown, Table, pile_name};

/// The floats that show each pile's count, and the count each shows now.
#[derive(Debug, Default)]
pub struct PileCounts {
    shown: [Option<usize>; 6],
    /// Each pile's index and a count the scripts to come give it, kept between updates so that
    /// `prepare` allocates nothing once it has grown.
    coming: Vec<(usize, usize)>,
}

impl PileCounts {
    /// Lays out ahead, in `layer`, each count that `views`, the view the table shows and those
    /// the scripts to come lead to, give a pile, and forgets those laid out before that none of
    /// them gives. The client calls it as each update arrives.
    pub fn prepare<'a>(&mut self, layer: &mut Layer, views: impl Iterator<Item = &'a PlayerView>) {
        let coming = &mut self.coming;
        coming.clear();
        for view in views {
            for index in 0..6 {
                let count = count_in(view, index);
                if count > 0 && !coming.contains(&(index, count)) {
                    coming.push((index, count));
                }
            }
        }
        for index in 0..6 {
            let id = float(index);
            layer.forget_prepared(id, |key| coming.contains(&(index, count_of(key))));
            for &(_, count) in coming.iter().filter(|(pile, _)| *pile == index) {
                layer.prepare(id, key_of(count), || count_text(index, count));
            }
        }
    }

    /// Shows each pile's count beside its top card as `table` shows it at rest, or none unless
    /// `visible`. Lays nothing out and allocates nothing for what `prepare` laid out.
    pub fn write(&mut self, layer: &mut Layer, table: &Table, visible: bool) {
        for index in 0..6 {
            let (side, zone) = PileShown::of_index(index);
            let pile = table.pile(side, zone).filter(|_| visible);
            let want = pile.map(|(count, _)| count);
            let id = float(index);
            if want != self.shown[index] {
                if !layer.show_prepared(id, want.map(key_of)) {
                    debug_assert!(false, "no count {want:?} is prepared for pile {index}");
                    layer.float(id, want.map(|count| count_text(index, count)));
                }
                self.shown[index] = want;
            }
            let Some((_, corners)) = pile else {
                continue;
            };
            let left = corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min);
            let right = corners
                .iter()
                .map(|c| c.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let top = corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
            let bottom = corners
                .iter()
                .map(|c| c.y)
                .fold(f32::NEG_INFINITY, f32::max);
            let middle = f32::midpoint(left, right);
            // Over the viewer's piles, which lie at the bottom of the window, and under the
            // opponent's, which lie at its top.
            let (point, anchor) = match side {
                Side::You => (Vec2::new(middle, top - COUNT_GAP), Vec2::new(0.5, 1.0)),
                Side::Opponent => (Vec2::new(middle, bottom + COUNT_GAP), Vec2::new(0.5, 0.0)),
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

/// The float that shows the count of the pile at `index` (`PileShown::index`).
fn float(index: usize) -> crate::element::FloatId {
    floats::PILE_COUNT[index / 3][index % 3]
}

/// The key a count is prepared under.
fn key_of(count: usize) -> u64 {
    count as u64
}

/// The count a key was prepared under.
fn count_of(key: u64) -> usize {
    usize::try_from(key).unwrap_or(usize::MAX)
}

/// How many cards `view` has in the pile at `index`.
fn count_in(view: &PlayerView, index: usize) -> usize {
    let (side, zone) = PileShown::of_index(index);
    let player = match side {
        Side::You => &view.players.you,
        Side::Opponent => &view.players.opponent,
    };
    match zone {
        ZoneName::Library => player.library.len(),
        ZoneName::Graveyard => player.graveyard.len(),
        _ => view
            .objects
            .iter()
            .filter(|object| {
                object.zone == ZoneName::Exile && object.zone_side.unwrap_or(object.owner) == side
            })
            .count(),
    }
}

/// The count of the pile at `index`: text that shows the number, named for the pile and the
/// number, as "Your library 60", so that the tree's line for it says both.
fn count_text(index: usize, count: usize) -> Element {
    let (side, zone) = PileShown::of_index(index);
    let mut element = text(&count.to_string());
    element.name = format!("{} {count}", pile_name(side, zone));
    element
}
