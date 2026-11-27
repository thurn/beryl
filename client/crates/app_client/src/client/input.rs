//! How the client takes pointer and key input (docs/beryl-3d.md, "Hosts"): a press on the flat
//! layer or a card, a drag that scrolls or sets a slider, and the keys of shortcuts, focus and
//! the bug report.

use beryl_protocol::protocol::ObjectId;
use glam::Vec2;

use super::Client;
use super::card_input::Hold;
use super::carry::Carry;
use app_screens::screens::settings::{Message as SettingsMessage, slider_setting};
use app_screens::screens::{Screen, Sheet};
use game_hud::inspect::Viewed;
use game_table::table::{Scroller, Table};
use kit_frame::frame::{
    ElementId, Frame, Place, Resolved, Role, StateBits, TRAY_CARD_BOX_IDS, TRAY_GRID_IDS,
};
use kit_scene::Reach;
use kit_scene::hit::PointerHit;
use kit_scene::input::{Hold as TouchHold, KeyResponse, Point, Router, Target, TouchPolicy};
use kit_scene::scroll::ScrollGesture;
use kit_ui::element::Action;
use kit_ui::input::{Button, Key, KeyCode, KeyPhase, Pointer, PointerKind, PointerPhase};
use kit_ui::layout::{Edit, Layer, Navigation, PointerTarget};
use kit_ui::widgets::STEP;

/// The part of the scene that owns a pointer action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Node {
    Control(ElementId),
    Card(usize),
    Pile(Viewed),
    Tray(usize),
    Other(ElementId),
    Blocked,
}

pub(super) type SceneTarget = Target<Node, Place>;

pub(super) struct ScenePoint {
    event: Point<SceneTarget>,
    pub flat: Option<PointerTarget>,
}

impl std::ops::Deref for ScenePoint {
    type Target = Point<SceneTarget>;

    fn deref(&self) -> &Self::Target {
        &self.event
    }
}

impl ScenePoint {
    pub fn action(&self) -> Option<&Action> {
        let (action, enabled) = self.flat.as_ref()?.press.as_ref()?;
        enabled.then_some(action)
    }

    pub fn hovering(&self) -> bool {
        let pointer = self.pointer;
        !(matches!(pointer.phase, PointerPhase::Cancel | PointerPhase::Leave)
            || (pointer.kind == PointerKind::Touch && pointer.phase == PointerPhase::Up))
    }

    pub fn on_screen(&self) -> bool {
        on_screen(self.target)
    }
}

/// View state used when a captured interaction changes its presentation.
#[derive(Default)]
pub(super) enum Control {
    #[default]
    None,
    Slider(ElementId),
    Field(ElementId),
    Card(Hold),
    Order(Carry),
}

#[derive(Default)]
pub(super) struct Routes {
    router: Router<SceneTarget>,
    pub scrolling: ScrollGesture<super::scrolling::Target>,
    pub control: Control,
    action: Option<Action>,
    /// The row the keys scroll: the last one pressed, until another press or screen.
    pub row_focus: Option<Scroller>,
    /// Whether the finger down shows the card it rests on enlarged (`advance_gesture`), until
    /// it lifts or starts to drag the card.
    zoomed: bool,
}

/// The current screen over the last drawn table.
struct PointerScene<'a> {
    layer: &'a Layer,
    frame: &'a Frame,
    table: &'a Table,
    open: bool,
    screen: Screen,
    sheet: bool,
    manual_library: bool,
    selection: &'a [ObjectId],
}

impl PointerScene<'_> {
    fn pick(
        &self,
        at: Vec2,
        wheel: Option<(Vec2, f32)>,
        carried: Option<ObjectId>,
    ) -> (Option<SceneTarget>, Option<u32>, Option<PointerTarget>) {
        let picked = kit_scene::hit::pointer(
            self.frame,
            self.layer,
            at,
            self.open,
            wheel,
            |node| self.available(node, carried),
            |node| {
                tray_grid(node.id)
                    .and_then(|index| self.table.tray_viewport(index))
                    .is_some_and(|viewport| {
                        wheel.is_none_or(|(delta, notch)| viewport.takes_wheel(delta, notch))
                    })
            },
        );
        match picked {
            PointerHit::Layer(flat) => {
                let (target, index) = self.screen_target(&flat, carried);
                (Some(target), index, Some(flat))
            }
            PointerHit::Table(index) if !self.sheet => {
                let element = &self.frame.resolved[index as usize];
                (Some(self.table_target(element)), Some(index), None)
            }
            PointerHit::Table(_) | PointerHit::Blocked => (
                Some(Target {
                    node: Node::Blocked,
                    container: None,
                }),
                None,
                None,
            ),
            PointerHit::Empty => (None, None, None),
        }
    }

    fn screen_target(
        &self,
        flat: &PointerTarget,
        carried: Option<ObjectId>,
    ) -> (SceneTarget, Option<u32>) {
        if flat.press.is_none()
            && let Some(object) = flat.shown
            && carried.is_none_or(|card| u32::from(card.0) != object.0)
            && let Some(place) = self.table.objects().card_place(object.0 as usize)
        {
            let target = Target {
                node: Node::Card(object.0 as usize),
                container: Some(place.into()),
            };
            return (target, self.index_of(object, Role::Card));
        }
        let player = flat.role == Role::Player && flat.press.is_none();
        let (node, hit) = if player {
            (Node::Other(flat.id), self.index_of(flat.id, Role::Player))
        } else {
            (Node::Control(flat.id), None)
        };
        (
            Target {
                node,
                container: Some(Place::Layer),
            },
            hit,
        )
    }

    fn index_of(&self, id: ElementId, role: Role) -> Option<u32> {
        let at = self
            .frame
            .resolved
            .iter()
            .position(|node| node.id == id && node.role == role)?;
        u32::try_from(at).ok()
    }

    fn available(&self, node: &Resolved, carried: Option<ObjectId>) -> bool {
        if node.state.has(StateBits::HELD) {
            return false;
        }
        let Some(card) = carried.filter(|_| node.role == Role::Card) else {
            return true;
        };
        let Ok(slot) = u16::try_from(node.id.0).map(ObjectId) else {
            return true;
        };
        slot != card && !(self.selection.contains(&card) && self.selection.contains(&slot))
    }

    fn table_target(&self, element: &Resolved) -> SceneTarget {
        // The top card and its pile share an input target.
        let pile = super::viewing::pointer_pile(
            element.place,
            self.screen,
            !self.sheet,
            self.manual_library,
        );
        let node = match (element.role, pile) {
            (Role::Pile, Some(pile)) => Node::Pile(pile),
            (Role::Card, Some(pile)) if !element.interactive => Node::Pile(pile),
            (Role::Card, _) => Node::Card(element.id.0 as usize),
            _ => tray_grid(element.id).map_or(Node::Other(element.id), Node::Tray),
        };
        Target {
            node,
            container: Some(element.place),
        }
    }
}

impl Client {
    fn pointer_scene(&self) -> PointerScene<'_> {
        PointerScene {
            layer: &self.layer,
            frame: &self.frame,
            table: self.show.table(),
            open: self.features.layer_open,
            screen: self.features.screen,
            sheet: self.features.sheet.is_some(),
            manual_library: self.at_table(),
            selection: &self.features.table().selection,
        }
    }

    /// The manual keyboard commands use the same scene picking as pointer input.
    pub(super) fn manual_target_at(&self, at: Vec2) -> game_manual::manual::Target {
        let (target, hit, _) = self.pointer_scene().pick(at, None, None);
        self.manual_viewer_target(target)
            .unwrap_or_else(|| game_manual::manual::target_from_hit(&self.frame, hit))
    }

    pub(super) fn point(&mut self, pointer: Pointer) {
        let screen = self.features.screen;
        let at_table = screen == Screen::Table;
        let manual_hold = at_table
            && self.features.sheet.is_none()
            && self.features.menu().is_none()
            && self
                .show
                .latest()
                .is_some_and(|update| update.view.table.is_some());
        let pinned = self.features.inspector().pinned();
        let tuning = &self.show.table().data().pointer;
        let scene = PointerScene {
            layer: &self.layer,
            frame: &self.frame,
            table: self.show.table(),
            open: self.features.layer_open,
            screen,
            sheet: self.features.sheet.is_some(),
            manual_library: self.at_table(),
            selection: &self.features.table().selection,
        };
        let mut flat = None;
        let routed = self
            .routes
            .router
            .route(pointer, self.now, tuning, |source, dragging| {
                let carried = source
                    .filter(|_| dragging)
                    .and_then(|source| match source.node {
                        Node::Card(slot) => u16::try_from(slot).ok().map(ObjectId),
                        _ => None,
                    });
                let wheel = (pointer.phase == PointerPhase::Wheel)
                    .then_some((pointer.wheel, tuning.wheel_notch_points));
                let (target, index, screen_target) = scene.pick(pointer.position, wheel, carried);
                flat = screen_target;
                let game = screen == Screen::Game;
                // Any card of the game, wherever it rests, is shown enlarged while a finger
                // rests on it.
                let game_card =
                    game && target.is_some_and(|target| matches!(target.node, Node::Card(_)));
                let policy = TouchPolicy {
                    double_tap: (game || at_table) && !scene.sheet && target.is_none(),
                    hold: if pinned {
                        TouchHold::None
                    } else if game_card {
                        TouchHold::Quick
                    } else if manual_hold && !on_screen(target) {
                        TouchHold::Long
                    } else {
                        TouchHold::None
                    },
                    delayed_pickup: at_table,
                };
                (target, index, policy)
            });
        if routed.cancelled {
            self.cancel_interaction();
        }
        // The enlarged card goes once the finger lifts, or as it starts to drag the card.
        let dragging = routed
            .point
            .as_ref()
            .is_some_and(|point| point.drag.is_some());
        if self.routes.zoomed
            && pointer.kind == PointerKind::Touch
            && (routed.cancelled || ends_gesture(&pointer) || dragging)
        {
            self.routes.zoomed = false;
            self.features
                .inspect(&game_hud::hud::inspector::Message::Pin(None));
        }
        if let Some(event) = routed.point {
            let point = ScenePoint { event, flat };
            let primary_down =
                pointer.phase == PointerPhase::Down && pointer.button == Button::Primary;
            if primary_down {
                self.end_control();
                self.routes.action = point.action().cloned();
                self.routes.scrolling.cancel();
                self.routes.row_focus = None;
            }
            if primary_down && pinned {
                self.features
                    .inspect(&game_hud::hud::inspector::Message::Pin(None));
                if pointer.kind == PointerKind::Touch {
                    self.routes.router.cancel();
                    return;
                }
            }
            self.deliver_point(&point);
            if point.captured && ends_gesture(&pointer) {
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

    /// The cursor used to aim on the table. The report still routes its own pointer events,
    /// but the table's pointer keeps the position it had when the report opened.
    pub(super) fn table_mouse(&self) -> Option<Vec2> {
        if self.bug_open() {
            self.show.pointer().position()
        } else {
            self.mouse()
        }
    }

    pub(super) fn end_control(&mut self) {
        self.routes.action = None;
        match std::mem::take(&mut self.routes.control) {
            Control::Order(carry) if carry.lifted => self.put_item_down(),
            Control::Card(_) => self.release_card_motion(None),
            Control::Field(_) => {
                self.edit(Edit::Up);
            }
            _ => {}
        }
    }

    pub(super) fn suppress_press(&mut self) {
        self.routes.router.suppress_press();
    }

    pub(super) fn cancel_gesture(&mut self) {
        self.routes.router.cancel();
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
            let target = self.manual_captured_target(source);
            self.open_table_menu_at(at, target)
        } else if self.features.screen == Screen::Game
            && self.features.sheet.is_none()
            && let Some(Target {
                node: Node::Card(slot),
                ..
            }) = source
            && let Ok(slot) = u16::try_from(slot)
        {
            // The card shows enlarged while the finger stays down. The finger keeps its press,
            // so that it can still drag the card, but lifting it does not use the card.
            self.features
                .inspect(&game_hud::hud::inspector::Message::Pin(Some(ObjectId(
                    slot,
                ))));
            self.routes.zoomed = true;
            self.routes.router.suppress_press();
            false
        } else {
            false
        };
        if handled {
            self.cancel_interaction();
        }
    }

    pub(super) fn cancel_interaction(&mut self) {
        self.routes.scrolling.cancel();
        self.cancel_gesture();
        if self.features.table().sweep.is_some() {
            self.features
                .update_table(game_manual::features::table::Change::Sweep(None));
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
        self.show.pointer_mut().observe(&pointer);
        // A press outside an open context menu dismisses it.
        if pointer.phase == PointerPhase::Down
            && pointer.button == Button::Primary
            && self.features.layer_open
            && self.layer.menu_open()
            && !point.flat.as_ref().is_some_and(|target| target.menu)
        {
            self.suppress_press();
            self.dismiss_menu();
            return;
        }
        let table = self.features.sheet.is_none() && !self.table_frozen();
        if table && self.manual_point(point) {
            if pointer.phase == PointerPhase::Down {
                self.suppress_press();
            }
            return;
        }
        if self.open_table_menu(point) {
            return;
        }
        if self.features.screen == Screen::Game
            && self.features.sheet.is_none()
            && pointer.phase == PointerPhase::Down
            && pointer.button == Button::Secondary
            && let Some(Target {
                node: Node::Card(slot),
                ..
            }) = point.target
            && let Ok(slot) = u16::try_from(slot)
        {
            let slot = ObjectId(slot);
            let next = (self.features.inspector().pinned_slot() != Some(slot)).then_some(slot);
            self.features
                .inspect(&game_hud::hud::inspector::Message::Pin(next));
            return;
        }
        // An established card drag keeps its source as it crosses other features.
        if table && self.held_card_point(point) {
            return;
        }
        if self.scroll_pointer(point) {
            return;
        }
        if self.carry_item(point) || self.slide(point) {
            self.routes.scrolling.cancel();
        } else {
            self.press_flat(point);
            if table {
                self.card_point(point);
                self.pile_point(point);
            }
        }
        self.hover_flat(point);
        self.suggest_payment(point);
    }

    /// Updates visual hover using the same screen node that receives the pointer.
    pub(super) fn hover_flat(&mut self, point: &ScenePoint) {
        let pointer = &point.pointer;
        let flat = point.flat.as_ref().filter(|_| point.hovering());
        let hovered = flat.filter(|flat| flat.press.is_some()).map(|flat| flat.id);
        let held = match self.routes.control {
            Control::Slider(id) if !ends_gesture(pointer) => Some(id),
            Control::Order(carry) if carry.lifted => None,
            _ => self
                .routes
                .router
                .pressed()
                .and_then(|target| match target.node {
                    Node::Control(id) => Some(id),
                    _ => None,
                }),
        };
        self.layer.point_at(hovered, held);
        self.layer.hover_target(
            flat.filter(|flat| flat.role == Role::MenuItem)
                .map(|flat| flat.id),
            flat.and_then(|flat| Some((flat.tooltip?, flat.id))),
        );
    }

    fn slide(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        if !point.captured {
            return false;
        }
        let (press, rect) = match (pointer.phase, &self.routes.control) {
            (PointerPhase::Down, _) if pointer.button == Button::Primary => {
                let Some(flat) = &point.flat else {
                    return false;
                };
                let (Some(press), Some(rect)) = (point.action().cloned(), flat.slider) else {
                    return false;
                };
                self.routes.control = Control::Slider(flat.id);
                self.layer.focus(Some(flat.id));
                (press, rect)
            }
            (PointerPhase::Move, Control::Slider(id)) => match self.layer.slider(*id) {
                Some(at) => at,
                None => return true,
            },
            (_, Control::Slider(_)) => return true,
            _ => return false,
        };
        self.set_slider(&press, rect, pointer.position.x);
        true
    }

    /// Applies the captured press to the screen control that owns it.
    pub(super) fn press_flat(&mut self, point: &ScenePoint) {
        let pointer = &point.pointer;
        match pointer.phase {
            PointerPhase::Down if pointer.button == Button::Primary => {
                let Some(flat) = &point.flat else {
                    return;
                };
                let focus = if flat.focus {
                    Some(flat.id)
                } else {
                    flat.scroller.map(|(id, _)| id)
                };
                if let Some(id) = focus {
                    self.layer.focus(Some(id));
                    if self.layer.edited_id() == Some(id) {
                        self.routes.control = Control::Field(id);
                        self.press_field(pointer);
                    }
                }
            }
            PointerPhase::Move if self.editing_pointer(point) => {
                self.edit(Edit::Move(pointer.position));
            }
            PointerPhase::Up if pointer.button == Button::Primary => {
                if !self.editing_pointer(point)
                    && point.released_by(|target| target.node).is_some()
                    && let Some(action) = point.action()
                    && Some(action) == self.routes.action.as_ref()
                {
                    self.dispatch(action);
                }
            }
            _ => {}
        }
    }

    fn editing_pointer(&self, point: &ScenePoint) -> bool {
        point.captured
            && matches!(self.routes.control, Control::Field(id) if self.layer.edited_id() == Some(id))
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
            || (self.features.sheet.is_none() && self.catalog_key(key))
            || (self.layer.edited_id().is_some() && self.field_key(key))
        {
            return Some(KeyResponse::Handled);
        }
        if key.code == KeyCode::Tab {
            self.routes.row_focus = None;
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

    /// Up, Down and Return while a searchable catalog prompt (card name or creature type) is
    /// open: arrows move the highlighted row among the filtered entries, and Return picks the
    /// highlighted entry (or confirms when that entry is already chosen).
    fn catalog_key(&mut self, key: Key) -> bool {
        use beryl_protocol::prompt::{ButtonId, Click, ShapeBody};
        use game_hud::prompts::catalog;

        if self.features.screen != app_screens::screens::Screen::Game
            || !self.show.settled()
            || self.features.prompts.waiting_for_reply()
        {
            return false;
        }
        let Some(prompt) = self.features.prompts.current() else {
            return false;
        };
        if !catalog::is_searchable(prompt) {
            return false;
        }
        let ShapeBody::Catalog { entries, chosen } = &prompt.body else {
            return false;
        };
        let query = self.layer.fields().get(catalog::FIELD).text;
        let matched = catalog::matching(&query, entries);
        match key.code {
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let down = key.code == KeyCode::ArrowDown;
                let count = matched.len();
                if self.update_catalog(&catalog::Message::Highlight { down, count }) {
                    self.show_screen();
                }
                true
            }
            KeyCode::Enter => {
                if key.phase != KeyPhase::Down {
                    return true;
                }
                let highlight = self
                    .catalog_highlight()
                    .min(matched.len().saturating_sub(1));
                let Some(&entry_index) = matched.get(highlight) else {
                    return true;
                };
                let index = u16::try_from(entry_index).unwrap_or(u16::MAX);
                let confirm_ready = *chosen == Some(index)
                    && prompt
                        .buttons
                        .iter()
                        .any(|button| button.id == ButtonId::Confirm && button.enabled);
                if confirm_ready {
                    self.click(Click::Button {
                        button: ButtonId::Confirm,
                    });
                } else {
                    self.click(Click::Entry { index });
                }
                true
            }
            _ => false,
        }
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
}

fn tray_grid(id: ElementId) -> Option<usize> {
    (TRAY_GRID_IDS..TRAY_CARD_BOX_IDS)
        .contains(&id.0)
        .then(|| (id.0 - TRAY_GRID_IDS) as usize)
}

fn on_screen(target: Option<SceneTarget>) -> bool {
    target.is_some_and(|target| matches!(target.node, Node::Control(_) | Node::Blocked))
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
