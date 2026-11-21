//! The number of cards in each library, graveyard and exile pile, shown as a float of the flat
//! layer over the pile's top card (`floats::PILE_COUNT`; docs/beryl-3d.md, "Floats"). As the
//! web interface's pile tip does (`PileButton` in `ui/src/game/Piles.tsx`, `.tip` in
//! `ui/src/design/components/tips.css`), it shows only while the pointer is over the pile, at
//! once, as a tooltip's label naming the zone and the count ("Graveyard · 5"): centred over the
//! middle of the viewer's pile and under the foot of the opponent's. An empty pile shows none.
//!
//! A pile's count changes as the table takes the view a script leads to. Every count the scripts
//! to come will show is laid out ahead, as each update arrives (`PileCounts::prepare`), so that
//! a frame only chooses which laid-out element each float shows and sets its look, and
//! allocates nothing.

use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerState, PlayerView, Side, ZoneName};
use glam::Vec2;

use crate::play::Show;
use crate::table::{PileShown, Table, host_slot, pile_name};
use kit_base::data::space::flat::TOOLTIP_GAP;
use kit_ui::element::{Element, floats};
use kit_ui::layout::{FloatLook, Layer};
use kit_ui::widgets::tip_label;

/// The floats that show each pile's count, and the count each shows now.
#[derive(Debug, Default)]
pub struct PileCounts {
    shown: [Option<usize>; 6],
    /// The pile under the pointer as of the last `write`, whether or not its count shows.
    hovered: Option<usize>,
    /// Each pile's index and a count the scripts to come give it, kept between updates so that
    /// `prepare` allocates nothing once it has grown.
    coming: Vec<(usize, usize)>,
}

impl PileCounts {
    /// Lays out ahead, in `layer`, each count that `views`, the view the table shows and those
    /// the scripts to come lead to, give a pile, and each count `table` holds now, and forgets
    /// those laid out before that none of them gives. The client calls it as each update
    /// arrives.
    ///
    /// The table's own counts are kept because the table can still be playing an earlier
    /// script when the update arrives: a pile whose top card a box of the flat layer shows has
    /// no count, and once the box lets the card go, the pile shows the count the table holds,
    /// which no view still to come may give. The opening hand's boxes do this during a
    /// mulligan, while the table still holds the whole library.
    pub fn prepare<'a>(
        &mut self,
        layer: &mut Layer,
        table: &Table,
        views: impl Iterator<Item = &'a PlayerView>,
    ) {
        let coming = &mut self.coming;
        coming.clear();
        for index in 0..6 {
            let count = table.pile_count(index);
            if count > 0 {
                coming.push((index, count));
            }
        }
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

    /// Whether the pointer has come onto or left a pile since the last `write`, so that the
    /// next frame shows the count that follows it.
    #[must_use]
    pub fn due(&self, show: &Show) -> bool {
        show.pointer().hovered_pile() != self.hovered
    }

    /// Shows the count of the pile under `show`'s pointer, as found in the last frame, over its
    /// top card as `show`'s table shows it at rest, and no other pile's, or none unless
    /// `visible`. Lays nothing out and allocates nothing for what `prepare` laid out.
    pub fn write(&mut self, layer: &mut Layer, show: &Show, visible: bool) {
        let (table, hovered) = (show.table(), show.pointer().hovered_pile());
        self.hovered = hovered;
        for index in 0..6 {
            let (side, zone) = PileShown::of_index(index);
            let pile = (table.pile(side, zone)).filter(|_| visible && hovered == Some(index));
            let want = pile.map(|(count, _)| count);
            let id = float(index);
            if want != self.shown[index] {
                if !layer.show_prepared(id, want.map(key_of)) {
                    debug_assert!(false, "no count {want:?} is prepared for pile {index}");
                    layer.float(id, want.map(|count| count_text(index, count)));
                }
                self.shown[index] = want;
            }
            let Some((_, corners)) = &pile else {
                continue;
            };
            let (point, anchor) = match side {
                // `.tip-over`: centred on the middle of the pile, where the top card's
                // diagonals meet.
                Side::You => {
                    let middle =
                        (corners[0].midpoint(corners[2])).midpoint(corners[1].midpoint(corners[3]));
                    (middle, Vec2::new(0.5, 0.5))
                }
                // `.tip-below`: `TOOLTIP_GAP` under the pile's lowest edge, centred on the
                // middle of its foot, the bottom edge of the leaning card.
                Side::Opponent => {
                    let mut by_height = *corners;
                    by_height.sort_unstable_by(|a, b| a.y.total_cmp(&b.y));
                    let foot = f32::midpoint(by_height[2].x, by_height[3].x);
                    (
                        Vec2::new(foot, by_height[3].y + TOOLTIP_GAP),
                        Vec2::new(0.5, 0.0),
                    )
                }
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
fn float(index: usize) -> kit_ui::element::FloatId {
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
    let player = player(view, side);
    match zone {
        ZoneName::Library => player.library.len(),
        ZoneName::Graveyard => player.graveyard.len(),
        _ => exiled(view, side).count(),
    }
}

/// The slots of the cards in `side`'s pile of `zone` in `view`, the bottom one first.
#[must_use]
pub fn pile_cards(view: &PlayerView, side: Side, zone: ZoneName) -> Vec<ObjectId> {
    let player = player(view, side);
    match zone {
        ZoneName::Library => player.library.clone(),
        ZoneName::Graveyard => player.graveyard.clone(),
        _ => exiled(view, side).map(|object| object.slot).collect(),
    }
}

fn player(view: &PlayerView, side: Side) -> &PlayerState {
    match side {
        Side::You => &view.players.you,
        Side::Opponent => &view.players.opponent,
    }
}

/// The cards in `side`'s exile pile in `view`. A card that lies under the permanent holding it
/// in exile is not in the pile.
fn exiled(view: &PlayerView, side: Side) -> impl Iterator<Item = &ObjectView> {
    let find = |slot: ObjectId| view.objects.iter().find(|host| host.slot == slot);
    view.objects.iter().filter(move |object| {
        object.zone == ZoneName::Exile
            && object.zone_side.unwrap_or(object.owner) == side
            && host_slot(object, find).is_none()
    })
}

/// The count of the pile at `index`, as its tip says it: the zone and the number, "Library ·
/// 33" (`pileTip` in `ui/src/game/Piles.tsx`). It is named for the pile and the number, as
/// "Your library 60", so that the tree's line for it says both.
fn count_text(index: usize, count: usize) -> Element {
    let (side, zone) = PileShown::of_index(index);
    let words = match zone {
        ZoneName::Library => "Library",
        ZoneName::Graveyard => "Graveyard",
        _ => "Exile",
    };
    // It lets the pointer through to the pile and any card over it, as `.tip` takes no
    // pointer events.
    let mut element = tip_label(format!("{words} \u{b7} {count}")).passes(true);
    element.name = format!("{} {count}", pile_name(side, zone));
    element
}
