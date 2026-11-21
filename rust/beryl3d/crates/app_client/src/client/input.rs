//! How the client takes pointer and key input (docs/beryl-3d.md, "Hosts"): a press on the flat
//! layer or a card, a drag that scrolls or sets a slider, and the keys of shortcuts, focus and
//! the bug report.

use beryl_protocol::prompt::{Click, ShapeBody};
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::Client;
use super::carry::Carry;
use app_screens::screens::settings::{Message as SettingsMessage, slider_setting};
use app_screens::screens::{Screen, Sheet};
use game_hud::hud::rows::Message as RowMessage;
use game_hud::inspect::Viewed;
use game_hud::prompts;
use game_play::pointer::Hold;
use game_play::pointer::rows::scroller_of;
use game_table::table::Scroller;
use kit_frame::frame::{ElementId, Place, Role};
use kit_scene::Reach;
use kit_scene::input::{KeyResponse, Point, Router, Target, TouchPolicy};
use kit_scene::scroll::ScrollGesture;
use kit_ui::element::Action;
use kit_ui::input::{Button, Key, KeyCode, KeyPhase, Pointer, PointerKind, PointerPhase};
use kit_ui::layout::{Edit, Navigation};
use kit_ui::widgets::{STEP, level_at};

/// The part of the scene that owns a pointer action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Node {
    Control(ElementId),
    Card(usize),
    Pile(Viewed),
    Other(ElementId),
}

pub(super) type ScenePoint = Point<Target<Node, Place>>;

/// View state used when a captured interaction changes its presentation.
#[derive(Default)]
pub(super) enum Control {
    #[default]
    None,
    Slider(ElementId),
    Order(Carry),
}

#[derive(Default)]
pub(super) struct Routes {
    router: Router<Target<Node, Place>>,
    pub scrolling: ScrollGesture<super::scrolling::Target>,
    pub control: Control,
}

impl Client {
    pub(super) fn point(&mut self, pointer: Pointer) {
        let open = self.features.layer_open;
        let screen = self.features.screen;
        let no_sheet = self.features.sheet.is_none();
        let at_table = screen == Screen::Table;
        let flat = self
            .layer
            .press_at(pointer.position, open)
            .map(|(id, ..)| id);
        let manual_hold = at_table
            && no_sheet
            && self.features.menu().is_none()
            && self
                .show
                .latest()
                .is_some_and(|update| update.view.table.is_some());
        let pinned = self.features.inspector().pinned();
        // Classify each pile once by its physical zone. A press on its top card and
        // a release on its count reach the same pile.
        let game = screen == Screen::Game;
        let manual_library = self.at_table();
        let tuning = &self.show.table().data().pointer;
        let routed = self
            .routes
            .router
            .point(&self.frame, pointer, self.now, tuning, |hit| {
                let target = if let Some(id) = flat {
                    Some(Target {
                        node: Node::Control(id),
                        container: None,
                    })
                } else {
                    hit.map(|element| {
                        let pile = super::viewing::pointer_pile(
                            element.place,
                            screen,
                            no_sheet,
                            manual_library,
                        );
                        let node = match (element.role, pile) {
                            (Role::Pile, Some(pile)) => Node::Pile(pile),
                            (Role::Card, Some(pile)) if !element.interactive => Node::Pile(pile),
                            (Role::Card, _) => Node::Card(element.id.0 as usize),
                            _ => Node::Other(element.id),
                        };
                        Target {
                            node,
                            container: Some(element.place),
                        }
                    })
                };
                let game_hold = game
                    && no_sheet
                    && flat.is_none()
                    && hit.is_some_and(|element| {
                        element.role == Role::Card
                            && game_play::pointer::takes_pointer(element.place)
                    });
                let policy = TouchPolicy {
                    double_tap: (game || at_table) && no_sheet && hit.is_none(),
                    hold: !pinned && (manual_hold || game_hold),
                    delayed_pickup: at_table,
                };
                (target, policy)
            });
        if routed.cancelled {
            self.cancel_interaction();
        }
        if let Some(point) = routed.point {
            let primary_down =
                pointer.phase == PointerPhase::Down && pointer.button == Button::Primary;
            if primary_down {
                self.end_control();
                self.routes.scrolling.cancel();
                self.features.rows_mut().update(RowMessage::Focus(None));
            }
            if primary_down && pointer.kind == PointerKind::Touch && pinned {
                self.features
                    .inspector_mut()
                    .update(game_hud::hud::inspector::Message::Pin(None));
                self.routes.router.cancel();
            } else {
                self.deliver_point(&point);
            }
            if ends_gesture(&pointer) {
                self.end_control();
            }
        }
        if routed.double_tap && !self.bug_open() {
            self.set_sheet(Some(Sheet::BugReport));
        }
    }

    pub(super) fn mouse(&self) -> Option<Vec2> {
        self.routes.router.mouse()
    }

    pub(super) fn end_control(&mut self) {
        if let Control::Order(carry) = std::mem::take(&mut self.routes.control)
            && carry.lifted
        {
            self.put_item_down();
        }
    }

    pub(super) fn suppress_press(&mut self) {
        self.routes.router.suppress_press();
    }

    pub(super) fn cancel_gesture(&mut self) {
        self.routes.router.cancel();
        self.show.pointer_mut().cancel();
        self.end_control();
    }

    pub(super) fn cancel_touch(&mut self) {
        if self.routes.router.cancel_touch() {
            self.cancel_interaction();
        }
    }

    pub(super) fn gesture_due(&self) -> Option<u64> {
        self.routes.router.due(&self.data().pointer)
    }

    pub(super) fn advance_gesture(&mut self, now: u64) {
        let Some((source, at)) = self
            .routes
            .router
            .advance(now, &self.show.table().data().pointer)
        else {
            return;
        };
        let handled = if self.features.screen == Screen::Table {
            self.open_table_menu_at(at)
        } else if self.features.screen == Screen::Game
            && self.features.sheet.is_none()
            && let Some(Target {
                node: Node::Card(slot),
                ..
            }) = source
            && let Ok(slot) = u16::try_from(slot)
        {
            self.features
                .inspector_mut()
                .update(game_hud::hud::inspector::Message::Pin(Some(ObjectId(slot))));
            true
        } else {
            false
        };
        if handled {
            self.cancel_interaction();
        }
    }

    fn cancel_interaction(&mut self) {
        self.routes.scrolling.cancel();
        self.cancel_gesture();
        if self.features.table_mut().sweep.take().is_some() {
            self.show_screen();
        }
        self.layer.point_at(None, None);
    }

    /// Delivers the routed event to the feature using the captured node.
    fn deliver_point(&mut self, point: &ScenePoint) {
        let pointer = point.pointer;
        if self.bug_open() {
            self.bug_point(point);
            return;
        }
        let pickup = point.drag.map(|drag| (drag.direction, drag.pickup_ready));
        self.show.pointer_mut().handle(&pointer, pickup);
        if pointer.phase == PointerPhase::Down {
            self.features.table_mut().modifiers.shift = pointer.modifiers.shift;
        }
        self.features
            .prompts
            .carry(self.show.pointer().held(), self.views, &self.show);
        // A press outside an open context menu closes it and means nothing else.
        if pointer.phase == PointerPhase::Down
            && pointer.button == Button::Primary
            && self
                .layer
                .outside_menu(pointer.position, self.features.layer_open)
        {
            self.suppress_press();
            self.dismiss_menu();
            return;
        }
        // Sheets and a frozen table leave only the flat layer's controls available.
        let table = self.features.sheet.is_none() && !self.table_frozen();
        if table && self.manual_point(point) {
            if pointer.phase == PointerPhase::Down {
                self.suppress_press();
            }
            return;
        }
        if self.scroll_pointer(point) || self.open_table_menu(&pointer) {
            return;
        }
        if self.carry_item(point) {
            self.routes.scrolling.cancel();
            return;
        }
        if self.slide(&pointer) {
            self.routes.scrolling.cancel();
        } else {
            self.press_flat(point, table);
        }
        let hovered = self.hover_flat(&pointer);
        // The pointer on a payment's Auto Pay, or a touch held on it, marks what it would tap.
        let suggesting = if let (Some(prompt), Some((_, action, true))) =
            (self.features.prompts.current(), &hovered)
            && let Some(game_hud::prompts::bar::Message::Button(button)) = action.downcast_ref()
        {
            prompts::shows(prompt, *button)
        } else {
            false
        };
        self.features
            .prompts
            .suggest(suggesting, self.views, &self.show);
    }

    /// Tells the flat layer which element a press under `pointer` would reach, which it draws
    /// hovered, and which one is held down, and returns the first with its press.
    pub(super) fn hover_flat(&mut self, pointer: &Pointer) -> Option<(ElementId, Action, bool)> {
        let gone = matches!(pointer.phase, PointerPhase::Cancel | PointerPhase::Leave)
            || (pointer.kind == PointerKind::Touch && pointer.phase == PointerPhase::Up);
        let hovered = (!gone)
            .then(|| {
                self.layer
                    .press_at(pointer.position, self.features.layer_open)
            })
            .flatten();
        let id = hovered.as_ref().map(|(id, ..)| *id);
        let held = if ends_gesture(pointer) {
            None
        } else {
            match self.routes.control {
                Control::Slider(id) => Some(id),
                Control::Order(carry) if carry.lifted => None,
                _ => self
                    .routes
                    .router
                    .pressed()
                    .and_then(|target| match target.node {
                        Node::Control(id) => Some(id),
                        _ => None,
                    }),
            }
        };
        (self.layer).point_at(id, held);
        let at = (!gone).then_some(pointer.position);
        self.layer.hover(at, self.features.layer_open);
        hovered
    }

    /// Sets the level of the slider the primary pointer goes down on, and of the one it drags,
    /// to the level under the pointer, until the press ends; the slider takes the focus. Returns
    /// whether the pointer is on a slider, and so means nothing else.
    fn slide(&mut self, pointer: &Pointer) -> bool {
        let primary = pointer.button == Button::Primary;
        let at = match (pointer.phase, &self.routes.control) {
            (PointerPhase::Down, _) if primary => {
                let Some((id, press, rect)) = self
                    .layer
                    .slider_at(pointer.position, self.features.layer_open)
                else {
                    return false;
                };
                self.routes.control = Control::Slider(id);
                self.layer.focus(Some(id));
                (press, rect)
            }
            (PointerPhase::Move, Control::Slider(id)) => match self.layer.slider(*id) {
                Some(at) => at,
                None => return true,
            },
            (PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave, Control::Slider(_)) => {
                return true;
            }
            _ => return false,
        };
        let (press, rect) = at;
        if let Some(setting) = slider_setting(&press)
            && let Some(level) = setting.level()
        {
            let to = level_at(pointer.position.x, rect.x, rect.width);
            if to != level {
                self.dispatch(&Action::new(SettingsMessage::Change(setting.at_level(to))));
            }
        }
        true
    }

    /// Applies the captured press to a control, card or pile.
    pub(super) fn press_flat(&mut self, point: &ScenePoint, table: bool) {
        let pointer = &point.pointer;
        let primary = pointer.button == Button::Primary;
        let open = self.features.layer_open;
        match pointer.phase {
            PointerPhase::Down if primary => {
                let focus = self
                    .layer
                    .focusable_at(pointer.position, open)
                    .or_else(|| self.layer.scroller_at(pointer.position, open));
                if let Some(id) = focus {
                    self.layer.focus(Some(id));
                    if self.layer.edited_id() == Some(id) {
                        self.press_field(pointer);
                    }
                }
                if table
                    && let Some(Target {
                        node: Node::Card(slot),
                        container: Some(place),
                    }) = point.target
                {
                    let scene = self.show.table();
                    let in_tray = scene.in_tray(slot);
                    let hand_scrolls = !in_tray
                        && scroller_of(place) == Some(Scroller::Hand)
                        && scene.row_window(Scroller::Hand).scrolls();
                    let tray_scrolls = in_tray
                        && pointer.kind == PointerKind::Touch
                        && scene.tray_viewport_at(pointer.position).is_some();
                    let hold = match (self.holdable(slot, place), hand_scrolls) {
                        (false, _) => Hold::Never,
                        (true, _) if tray_scrolls => Hold::Sideways,
                        (true, false) => Hold::Drag,
                        (true, true) => Hold::UpOrDown,
                    };
                    self.show
                        .pointer_mut()
                        .press(slot, pointer.position, hold, pointer.kind);
                }
            }
            PointerPhase::Move if self.layer.edited_id().is_some() => {
                self.edit(Edit::Move(pointer.position));
            }
            PointerPhase::Up if primary => {
                if self.layer.edited_id().is_some() {
                    self.edit(Edit::Up);
                }
                match point
                    .released_by(|target| target.node)
                    .map(|target| target.node)
                {
                    Some(Node::Pile(pile)) if table => {
                        if !self.table_library_clicked(pile) {
                            self.dispatch(&Action::new(game_hud::hud::viewer::Message::Open(pile)));
                        }
                    }
                    Some(Node::Control(pressed))
                        if let Some((id, action, true)) =
                            self.layer.press_at(pointer.position, open)
                            && pressed == id =>
                    {
                        self.dispatch(&action);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Gives the open overlay and focused control the key before looking for shortcuts.
    pub(super) fn key(&mut self, key: Key) {
        if key.phase == KeyPhase::Up {
            return;
        }
        let action = kit_scene::input::key(
            self,
            |client| client.focused_key(key),
            |client| client.shortcut(key),
        );
        if let Some(KeyResponse::Action(action)) = action {
            self.dispatch(&action);
        }
    }

    /// Navigation, scrolling and editing can consume a key without returning an action.
    fn focused_key(&mut self, key: Key) -> Option<KeyResponse<Action>> {
        if self.bug_key(key)
            || (self.features.sheet.is_none() && self.table_overlay_key(key))
            || (self.layer.edited_id().is_some() && self.field_key(key))
        {
            return Some(KeyResponse::Handled);
        }
        if key.code == KeyCode::Tab {
            self.features.rows_mut().update(RowMessage::Focus(None));
            self.layer
                .move_focus(!key.modifiers.shift, self.features.layer_open);
            return Some(KeyResponse::Handled);
        }
        match self.layer.navigate(&key, self.features.layer_open) {
            Navigation::Press(action) => return Some(KeyResponse::Action(action)),
            Navigation::Moved => return Some(KeyResponse::Handled),
            Navigation::Ignored => {}
        }
        let arrow = match key.code {
            KeyCode::ArrowLeft => Some(false),
            KeyCode::ArrowRight => Some(true),
            _ => None,
        };
        if let Some(up) = arrow
            && self
                .layer
                .focused_slider(self.features.layer_open)
                .is_some()
        {
            return Some(
                self.nudge(up)
                    .map_or(KeyResponse::Handled, KeyResponse::Action),
            );
        }
        if self.scroll_key(key) {
            return Some(KeyResponse::Handled);
        }
        if key.phase == KeyPhase::Down && key.code == KeyCode::Enter {
            return self
                .layer
                .focused_press(self.features.layer_open)
                .map(KeyResponse::Action);
        }
        None
    }

    /// A key no focused element took: the declared shortcuts of the features shown, then
    /// Escape closing the sheet or menu drawn over the rest, then the shortcuts every screen
    /// answers (`features::keys`).
    fn shortcut(&mut self, key: Key) -> Option<KeyResponse<Action>> {
        if let Some(response) = self.declared_shortcut(key, Reach::Shown) {
            return Some(response);
        }
        if let Some(action) = self.layer.dismiss_press(&key, self.features.layer_open) {
            return Some(KeyResponse::Action(action));
        }
        self.declared_shortcut(key, Reach::App)
    }

    /// The press that moves the focused slider's level up a step, or down one if not `up`,
    /// as the left and right keys move a focused range input; `None` with no slider focused
    /// or at the end it would move past.
    fn nudge(&self, up: bool) -> Option<Action> {
        let action = self.layer.focused_slider(self.features.layer_open)?;
        let setting = slider_setting(&action)?;
        let level = setting.level()?;
        let to = if up {
            level.saturating_add(STEP).min(100)
        } else {
            level.saturating_sub(STEP)
        };
        (to != level).then(|| Action::new(SettingsMessage::Change(setting.at_level(to))))
    }

    /// Whether a press may pick up the card in `slot`, resting at `place`: a card the open
    /// prompt marks, once the prompt is drawn, in the viewer's hand or held up in a tray of two
    /// piles. This is the web interface's `#lifts` in `ui/src/stage/prompt.ts`.
    pub(super) fn holdable(&self, slot: usize, place: Place) -> bool {
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
            place,
            Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                ..
            }
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
    pub(super) fn drop_held(&mut self, slot: usize, at: Vec2) {
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

/// Whether `pointer` ends what the primary pointer was doing: the primary button or a finger
/// coming up, Cancel or Leave.
fn ends_gesture(pointer: &Pointer) -> bool {
    match pointer.phase {
        PointerPhase::Up => pointer.button == Button::Primary,
        PointerPhase::Cancel | PointerPhase::Leave => true,
        PointerPhase::Down | PointerPhase::Move | PointerPhase::Wheel => false,
    }
}
