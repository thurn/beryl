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

use game_table::table::{PileShown, Table, host_slot};
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, FloatId, floats};
use kit_ui::layout::{FloatLook, Layer};
use std::convert::Infallible;
mod view;
use view::count_text;

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

/// The pile index and number its label shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Inputs {
    pub index: usize,
    pub count: usize,
}

impl Feature for PileCounts {
    type State = Self;
    type Message = Infallible;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        _: &mut Self,
        message: &Infallible,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        match *message {}
    }

    fn view_key(_: &Self, input: &Inputs) -> Inputs {
        *input
    }

    fn view(_: &Self, input: Inputs) -> Element {
        count_text(input.index, input.count)
    }
}

impl super::FloatFeature for PileCounts {
    fn floats() -> impl Iterator<Item = FloatId> {
        floats::PILE_COUNT.into_iter().flatten()
    }
}

/// Prepares the labels for the piles on the table and in the views to come.
pub struct Prepare<'a, Views> {
    pub layer: &'a mut Layer,
    pub table: &'a Table,
    pub views: Views,
}

impl<'a, Views: Iterator<Item = &'a PlayerView>> StateUpdate<PileCounts> for Prepare<'_, Views> {
    type Output = ();

    fn apply(self, state: &mut PileCounts) -> (bool, ()) {
        state.prepare(self.layer, self.table, self.views);
        (false, ())
    }
}

/// Samples the prepared label for the pile under the pointer.
pub struct Draw<'a> {
    pub layer: &'a mut Layer,
    pub table: &'a Table,
    pub hovered: Option<usize>,
    pub visible: bool,
}

impl StateUpdate<PileCounts> for Draw<'_> {
    type Output = ();

    fn apply(self, state: &mut PileCounts) -> (bool, ()) {
        state.write(self.layer, self.table, self.hovered, self.visible);
        (false, ())
    }
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
    fn prepare<'a>(
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
            layer.forget_prepared(id, |key| self.coming.contains(&(index, count_of(key))));
            for &(_, count) in self.coming.iter().filter(|(pile, _)| *pile == index) {
                layer.prepare(id, key_of(count), || {
                    Self::view(self, Inputs { index, count })
                });
            }
        }
    }

    /// Whether the pointer has come onto or left a pile since the last `write`, so that the
    /// next frame shows the count that follows it.
    #[must_use]
    pub fn due(&self, hovered: Option<usize>) -> bool {
        hovered != self.hovered
    }

    /// Shows the count of the hovered pile, as found in the last frame, over its
    /// top card on the table at rest, and no other pile's, or none unless
    /// `visible`. Lays nothing out and allocates nothing for what `prepare` laid out.
    fn write(&mut self, layer: &mut Layer, table: &Table, hovered: Option<usize>, visible: bool) {
        self.hovered = hovered;
        for index in 0..6 {
            let pile = (visible && hovered == Some(index))
                .then(|| table.pile_count_anchor(index, table.data().flat.tooltip.gap))
                .flatten();
            let want = pile.as_ref().map(|(count, _)| *count);
            let id = float(index);
            if want != self.shown[index] {
                if !layer.show_prepared(id, want.map(key_of)) {
                    debug_assert!(false, "no count {want:?} is prepared for pile {index}");
                    layer.float(
                        id,
                        want.map(|count| Self::view(self, Inputs { index, count })),
                    );
                }
                self.shown[index] = want;
            }
            if let Some((_, anchor)) = pile {
                layer.anchor_float(id, anchor, FloatLook::default());
            }
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
