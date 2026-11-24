//! Delivers released table cards to the manual table, prompt or inspector.

use beryl_protocol::prompt::{Click, ShapeBody};
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::{
    Client,
    input::{Control, Node, ScenePoint},
};
use app_screens::screens::Screen;
use game_hud::hud::inspector::Message as Inspect;
use game_hud::prompts;
use game_manual::features::table::Change;
use game_play::pointer::rows::scroller_of;
use game_table::table::Place;
use game_table::table::Scroller;
use kit_frame::frame::Place as FramePlace;
use kit_scene::input::{Direction, Target};
use kit_ui::element::Action;
use kit_ui::input::{Button, PointerKind, PointerPhase};

#[derive(Clone, Copy)]
pub(super) enum Hold {
    Never,
    Drag,
    Vertical,
    Horizontal,
}

impl Client {
    /// Gives the captured card every move after pickup, wherever the pointer goes.
    pub(super) fn held_card_point(&mut self, point: &ScenePoint) -> bool {
        let Control::Card(hold) = self.routes.control else {
            return false;
        };
        if !point.captured {
            return false;
        }
        let pointer = point.pointer;
        if let Some(drag) = point.drag
            && let Some(Target {
                node: Node::Card(slot),
                ..
            }) = drag.source
            && (self.show.pointer().held() == Some(slot)
                || (drag.pickup_ready
                    && match hold {
                        Hold::Never => false,
                        Hold::Drag => true,
                        Hold::Vertical => drag.direction == Direction::Vertical,
                        Hold::Horizontal => drag.direction == Direction::Horizontal,
                    }))
        {
            self.show.carry_card(slot, pointer.position);
            self.routes.scrolling.cancel();
            self.layer.point_at(None, None);
            if pointer.phase == PointerPhase::Move {
                if self.features.screen == Screen::Table {
                    self.follow_landing(point);
                }
                self.sync_prompt_carry();
            } else {
                self.release_card_motion(Some(pointer.position));
                if !point.on_screen() {
                    self.drop_card(slot, point);
                }
            }
            return true;
        }
        false
    }

    pub(super) fn release_card_motion(&mut self, at: Option<Vec2>) {
        self.show.release_card(at);
        self.sync_prompt_carry();
        if self.features.table().landing.is_some() {
            self.features.update_table(Change::Landing(None));
            self.show_screen();
        }
    }

    fn sync_prompt_carry(&mut self) {
        self.features
            .prompts
            .carry(self.show.pointer().held(), self.views, &self.show);
    }

    pub(super) fn card_point(&mut self, point: &ScenePoint) {
        let pointer = point.pointer;
        if pointer.phase == PointerPhase::Down
            && pointer.button == Button::Primary
            && let Some(Target {
                node: Node::Card(slot),
                container: Some(place),
            }) = point.target
        {
            let table = self.show.table();
            let in_tray = table.in_tray(slot);
            let hand_scrolls = !in_tray
                && scroller_of(place) == Some(Scroller::Hand)
                && table.row_window(Scroller::Hand).scrolls();
            let tray_scrolls = in_tray
                && pointer.kind == PointerKind::Touch
                && table
                    .tray_grid_index(slot)
                    .and_then(|index| table.tray_viewport(index))
                    .is_some();
            let hold = match (self.holdable(slot, place), hand_scrolls) {
                (false, _) => Hold::Never,
                (true, _) if tray_scrolls => Hold::Horizontal,
                (true, true) => Hold::Vertical,
                (true, false) => Hold::Drag,
            };
            self.routes.control = Control::Card(hold);
        } else if matches!(self.routes.control, Control::Card(_))
            && point.drag.is_none()
            && let Some(Target {
                node: Node::Card(slot),
                ..
            }) = point.released_by(|target| target.node)
        {
            if pointer.kind == PointerKind::Touch {
                self.card_tapped(slot);
            } else {
                self.card_clicked(slot);
            }
        }
    }

    pub(super) fn pile_point(&mut self, point: &ScenePoint) {
        if let Some(Target {
            node: Node::Pile(pile),
            ..
        }) = point.released_by(|target| target.node)
            && !self.table_library_clicked(pile)
        {
            self.dispatch(&Action::new(game_hud::hud::viewer::Message::Open(pile)));
        }
    }

    fn drop_card(&mut self, slot: usize, point: &ScenePoint) {
        let at = point.pointer.position;
        let table = self.show.table();
        if self.features.screen == Screen::Table {
            self.table_drop(slot, point);
        } else if table.in_tray(slot) {
            self.drop_held(slot, at);
        } else if let Some(rest) = self.show.pose(slot, self.now) {
            // The game accepts a carried hand card on the table above the fan even when
            // it lands on bare felt.
            let (corners, _) = table.camera().card_box(&rest);
            let top = corners
                .iter()
                .map(|corner| corner.y)
                .fold(f32::INFINITY, f32::min);
            if at.y < top {
                self.card_clicked(slot);
            }
        }
    }

    /// A press that picked nothing up on the card in `slot`: with Cmd held on the
    /// manual table it adds the card to the selection or takes it out, on a card the open
    /// decision refuses it says why (`refuse_click`), and otherwise it clicks the open prompt on
    /// the card.
    pub(super) fn card_clicked(&mut self, slot: usize) {
        if let Ok(slot) = u16::try_from(slot)
            && !self.table_click(ObjectId(slot))
            && !self.refuse_click(ObjectId(slot))
        {
            let on = Subject::Object {
                slot: ObjectId(slot),
            };
            self.click(beryl_protocol::prompt::Click::On { on });
        }
    }

    /// A finger's tap on the card in `slot`: a click if the open prompt can use the card or the
    /// manual table is shown, and otherwise the card shown whole.
    pub(super) fn card_tapped(&mut self, slot: usize) {
        if self.features.screen != Screen::Game || self.usable(slot) {
            self.card_clicked(slot);
        } else if let Ok(slot) = u16::try_from(slot) {
            self.features.inspect(&Inspect::Pin(Some(ObjectId(slot))));
        }
    }

    /// Whether the open prompt can use the card in `slot`: it marks the card.
    fn usable(&self, slot: usize) -> bool {
        let Some(prompt) = self
            .features
            .prompts
            .current()
            .filter(|_| self.features.layer_open)
        else {
            return false;
        };
        let Ok(slot) = u16::try_from(slot) else {
            return false;
        };
        let on = Subject::Object {
            slot: ObjectId(slot),
        };
        prompt.marks.iter().any(|marked| marked.on == on)
    }

    /// Whether a press may pick up the card in `slot`, resting at `place`: a card the open
    /// prompt marks, once the prompt is drawn, in the viewer's hand or held up in a tray of two
    /// piles. This is the web interface's `#lifts` in `ui/src/stage/prompt.ts`.
    fn holdable(&self, slot: usize, place: FramePlace) -> bool {
        if let Some(holdable) = self.table_holdable(slot, place) {
            return holdable;
        }
        let Some(prompt) = self
            .features
            .prompts
            .current()
            .filter(|_| self.features.layer_open)
        else {
            return false;
        };
        let in_hand = matches!(
            Place::in_frame(place),
            Some(Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                ..
            })
        );
        let on = u16::try_from(slot).map(|slot| Subject::Object {
            slot: ObjectId(slot),
        });
        // A card in a tray of piles is carried from pile to pile, as the web's `#piled` has it.
        let piled = (self.show.table().held()).is_some_and(|held| held.piles.len() > 1)
            && self.show.table().in_tray(slot);
        (in_hand || piled) && (prompt.marks.iter()).any(|marked| Ok(marked.on) == on)
    }

    /// The card in `slot`, held up in the open prompt's tray of piles, was let go at `at`: if it
    /// lands in a pile that is not closed, the prompt is told it was dropped there, before the
    /// card it landed in front of (`Table::tray_landing`), as the web's `#dropped` does. Let go
    /// outside every box, or in a closed pile, it goes back where it lay and nothing is sent.
    fn drop_held(&mut self, slot: usize, at: Vec2) {
        let Some(prompt) = self
            .features
            .prompts
            .current()
            .filter(|_| self.features.layer_open)
        else {
            return;
        };
        let (ShapeBody::Split { piles }, Some(held)) = (&prompt.body, prompts::Piles::of(prompt))
        else {
            return;
        };
        let table = self.show.table();
        let held = held.held(table.held());
        let Ok(card) = u16::try_from(slot) else {
            return;
        };
        let landed = table.tray_landing(&held, at, ObjectId(card));
        let Some((pile, before)) = landed else {
            return;
        };
        if piles.get(pile).is_none_or(|pile| pile.closed) {
            return;
        }
        if let (Ok(slot), Ok(pile), Ok(before)) = (
            u16::try_from(slot),
            u8::try_from(pile),
            u16::try_from(before),
        ) {
            let on = Subject::Object {
                slot: ObjectId(slot),
            };
            self.click(Click::Drop {
                on,
                pile,
                at: before,
            });
        }
    }
}
