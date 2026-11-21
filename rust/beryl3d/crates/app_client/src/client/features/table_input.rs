//! The manual table's pointer, on the client's side: picking up a card and dropping it
//! (`manual::drop`), Cmd-clicking a card and sweeping a rectangle to select (`manual::selection`),
//! clicking what a command acts toward (`manual::arrows`), and the marks and arrow these draw on
//! each frame.

use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use game_hud::inspect::Viewed;
use glam::Vec2;

use super::super::{
    Client, Platform,
    input::{Node, ScenePoint},
};

use app_screens::screens::Screen;
use game_manual::manual::arrows::{self, Pick};
use game_manual::manual::drop::{self, Area, End};
use game_manual::manual::selection;
use game_manual::manual::{Target, target_from_hit, viewers};
use kit_frame::frame::{Place, Role};
use kit_ui::input::{Button, PointerPhase};

impl Client {
    /// Whether the manual table is shown with nothing over it that takes the pointer: the table
    /// screen, a table's view, no sheet, menu or overlay.
    pub(in crate::client) fn at_table(&self) -> bool {
        self.features.screen == Screen::Table
            && self.features.sheet.is_none()
            && self.features.menu().is_none()
            && !self.features.overlay_open()
            && (self.show.latest()).is_some_and(|update| update.view.table.is_some())
    }

    /// Takes `pointer` on the manual table before anything else does, as the module describes.
    /// Returns whether it meant something here, and so nothing else.
    pub(in crate::client) fn manual_point(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        if self.features.screen != Screen::Table {
            return false;
        }
        if pointer.phase == PointerPhase::Down {
            // Cmd is Control off the Mac, as the web's `meta` click is.
            self.features.table_mut().modifiers.meta = if self.platform == Platform::Mac {
                pointer.modifiers.command
            } else {
                pointer.modifiers.control
            };
        }
        if self.features.table().picking.is_some() {
            return self.pick_target(point);
        }
        if self.life_clicked(point) {
            return true;
        }
        if self.sweep(point) {
            return true;
        }
        self.follow_landing(point);
        false
    }

    /// While a command waits for its target, a press on a card or a life total gives it that
    /// target, and a press anywhere else gives up. Every press then means nothing else.
    fn pick_target(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        if pointer.phase != PointerPhase::Down {
            return matches!(pointer.phase, PointerPhase::Up);
        }
        let Some(pick) = self.features.table_mut().picking.take() else {
            return false;
        };
        let clicked = target_from_hit(&self.frame, point.hit);
        if pointer.button == Button::Primary
            && let Some(to) = arrows::picked(pick, clicked)
            && let Some(view) = self.show.latest().map(|update| &update.view)
        {
            let actions = arrows::actions(pick, to, view, &self.features.table().selection);
            self.send_table(actions);
        }
        self.show.pointer_mut().cancel();
        true
    }

    /// A click on a player's life total on the table, with nothing of the flat layer over it,
    /// opens the number prompt that sets it, as the web's `registerPlayers` gives it Set life.
    /// Returns whether it did.
    fn life_clicked(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        if pointer.phase != PointerPhase::Down
            || pointer.button != Button::Primary
            || !self.at_table()
            || point
                .target
                .is_some_and(|target| matches!(target.node, Node::Control(_)))
        {
            return false;
        }
        let Target::Life(side) = target_from_hit(&self.frame, point.hit) else {
            return false;
        };
        self.run_table_command("life.set", Target::Life(side));
        true
    }

    /// A click on `pile` let go over it: on the manual table, a library draws a card for its
    /// player. Returns whether it did.
    pub(in crate::client) fn table_library_clicked(&mut self, pile: Viewed) -> bool {
        let Viewed::Pile(side, ZoneName::Library) = pile else {
            return false;
        };
        if !self.at_table() {
            return false;
        }
        self.run_table_command("library.draw", Target::Pile(side, ZoneName::Library));
        true
    }

    /// Starts a command's wait for its target, from the card it was given to.
    pub(in crate::client) fn start_pick(&mut self, command: u16, target: Target) {
        if let Target::Card(from) = target {
            self.features.table_mut().picking = Some(Pick { command, from });
        }
    }

    /// A sweep: the primary button going down on the empty table begins a rectangle, moving
    /// draws it once it has travelled the drag threshold, and letting go selects what it
    /// touches. Returns whether the pointer moved or ended a sweep, and so means nothing else.
    fn sweep(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        match pointer.phase {
            PointerPhase::Down if pointer.button == Button::Primary => {
                let empty = self.at_table()
                    && point
                        .target
                        .is_none_or(|target| !matches!(target.node, Node::Control(_)))
                    && point.hit.is_none_or(|index| {
                        let element = &self.frame.resolved[index as usize];
                        !matches!(element.role, Role::Card | Role::Pile | Role::Player)
                    });
                self.features.table_mut().sweep =
                    empty.then_some((pointer.position, pointer.position));
                false
            }
            PointerPhase::Move => {
                let Some((from, _)) = self.features.table().sweep else {
                    return false;
                };
                self.features.table_mut().sweep = Some((from, pointer.position));
                if point.drag.is_some() {
                    self.show_screen();
                }
                true
            }
            PointerPhase::Up => {
                let Some((from, _)) = self.features.table_mut().sweep.take() else {
                    return false;
                };
                if point.drag.is_some()
                    && let Some(view) = self.show.latest().map(|update| &update.view)
                {
                    let rect = selection::sweep_box(from, pointer.position);
                    self.features.table_mut().selection = selection::swept(&self.frame, rect, view);
                }
                self.show_screen();
                true
            }
            PointerPhase::Cancel | PointerPhase::Leave => {
                if self.features.table_mut().sweep.take().is_some() {
                    self.show_screen();
                }
                false
            }
            _ => false,
        }
    }

    /// The cards a press on `slot` carries: the selection if the card is in it, and the card
    /// alone otherwise.
    fn carried_cards(&self, slot: ObjectId) -> Vec<ObjectId> {
        let selected = &self.features.table().selection;
        if selected.contains(&slot) {
            selected.clone()
        } else {
            vec![slot]
        }
    }

    /// While a card is held on the table, outlines where it would land under `pointer`, and
    /// stops once the press ends.
    fn follow_landing(&mut self, point: &ScenePoint) {
        let landing = match (point.pointer.phase, self.show.pointer().held()) {
            (PointerPhase::Move, Some(slot)) => self.landing_at(slot, point.pointer.position),
            _ => None,
        };
        if landing != self.features.table().landing {
            self.features.table_mut().landing = landing;
            self.show_screen();
        }
    }

    /// Where the card in `slot`, and the cards it carries, would land if let go at `at`, if
    /// they may land there.
    fn landing_at(&self, slot: usize, at: Vec2) -> Option<drop::Destination> {
        let slot = ObjectId(u16::try_from(slot).ok()?);
        let view = &self.show.latest()?.view;
        let cards = self.carried_cards(slot);
        let table = self.show.table();
        let to = drop::destination_at(
            &self.frame,
            (table.layout(), table.camera()),
            at,
            &cards,
            view,
            table.data().flat.manual.drop,
        )?;
        drop::available(view, &to, &cards).then_some(to)
    }

    /// The card in `slot`, held on the table, was let go at `at`: the cards it carries land
    /// where it was let go. Several put on the top of a library open the order tray instead.
    /// On a frozen table (`manual::link`) nothing moves.
    pub(in crate::client) fn table_drop(&mut self, slot: usize, at: Vec2) {
        self.features.table_mut().landing = None;
        let to = (!self.table_frozen())
            .then(|| self.landing_at(slot, at))
            .flatten();
        let Some(to) = to else {
            self.show_screen();
            return;
        };
        let Ok(card) = u16::try_from(slot).map(ObjectId) else {
            return;
        };
        let cards = self.carried_cards(card);
        if to.area == Area::Pile(ZoneName::Library) && to.end == Some(End::Top) && cards.len() > 1 {
            self.order_library(to.side, cards);
            return;
        }
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        let actions = drop::actions(view, &self.frame, &cards, &to, at);
        self.send_table(drop::batch(actions).into_iter().collect());
        self.show_screen();
    }

    /// A Cmd-click on the card in `slot` on the table adds it to the selection or takes it out.
    pub(in crate::client) fn table_select(&mut self, slot: ObjectId) {
        let actionable =
            (self.show.latest()).is_some_and(|update| selection::actionable(&update.view, slot));
        if actionable {
            selection::toggle(&mut self.features.table_mut().selection, slot);
        }
    }

    /// Escape on the table, with no menu, overlay or viewer open for it to close, gives up a
    /// command's wait for its target, or else empties the selection, as the web's selection
    /// takes Escape. Returns whether it did either.
    pub(in crate::client) fn table_escape(&mut self) -> bool {
        if !self.at_table() || self.features.viewer().open().is_some() {
            return false;
        }
        if self.features.table_mut().picking.take().is_some() {
            return true;
        }
        if self.features.table().selection.is_empty() && self.features.table().sweep.is_none() {
            return false;
        }
        self.features.table_mut().selection.clear();
        self.features.table_mut().sweep = None;
        self.show_screen();
        true
    }

    /// Whether a press may pick up the card in `slot`, resting at `place`, on the manual table:
    /// a card on the battlefield, on the stack or in the viewer's hand that the table can act
    /// on. `None` off the table, where the open prompt decides.
    pub(in crate::client) fn table_holdable(&self, slot: usize, place: Place) -> Option<bool> {
        if !self.at_table() || self.features.table().picking.is_some() {
            return None;
        }
        let Place::Card { zone, side, .. } = place else {
            return Some(false);
        };
        let reachable = match zone {
            ZoneName::Battlefield | ZoneName::Stack => true,
            ZoneName::Hand => side == Side::You,
            _ => false,
        };
        let slot = ObjectId(u16::try_from(slot).ok()?);
        let view = &self.show.latest()?.view;
        Some(reachable && selection::actionable(view, slot))
    }

    /// The card of the open viewer under `at` on the table, as a target, if there is one.
    pub(in crate::client) fn viewer_card_at(&self, at: Vec2) -> Option<Target> {
        self.features.viewer().open()?;
        viewers::card_at(&self.frame, at, |slot| self.viewer_shows(slot))
    }

    /// On each frame at `now` on the manual table: the held card drops where it is let go, the
    /// selected cards wear the `selected` mark, the selection's summary stands over the highest
    /// selected permanent, and a command waiting for its target draws an arrow from its card to
    /// the mouse.
    pub(in crate::client) fn table_frame(&mut self, now: u64) {
        let table = self.features.screen == Screen::Table
            && (self.show.latest()).is_some_and(|update| update.view.table.is_some());
        self.show.pointer_mut().drop_anywhere(table);
        if !table {
            return;
        }
        selection::mark(self.show.lit_mut(), &self.features.table().selection, now);
        // The summary follows the highest selected permanent as the table last drew it, and
        // the screen is built again only when that permanent changes.
        let over = selection::highest(&self.frame, &self.features.table().selection);
        if over != self.features.table().summary_over {
            self.features.table_mut().summary_over = over;
            self.show_screen();
        }
        if let (Some(pick), Some(mouse)) = (self.features.table().picking, self.mouse()) {
            self.show.lit_mut().set_aims([arrows::aim(pick, mouse)]);
        }
    }
}
