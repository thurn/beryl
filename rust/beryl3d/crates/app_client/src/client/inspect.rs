//! Which card the inspector shows, and showing it (`inspect::inspector`) as the float
//! `floats::INSPECTOR` of the flat layer.
//!
//! The pointer finds the card it is over after each frame is built (`Show::point`), so the
//! inspector follows the card the last frame found. A frame whose hovered card differs from the
//! one the inspector last looked at draws again at once (`Client::frame` sets `Wake::Now`). The float is laid out
//! only when the inspected card, its place or the game changes, so a still frame lays nothing
//! out and allocates nothing.

use beryl_protocol::protocol::ObjectId;

use super::Client;
use app_screens::screens::{Screen, ScreenIn};
use game_hud::hud::inspector::{self, Message};
use kit_ui::element::floats;
use kit_ui::layout::FloatLook;

impl Client {
    /// Whether a script plays faster while many updates wait to play (`Show::set_catch_up`).
    /// The match tools turn this off, since the web interface never catches up.
    pub fn set_catch_up(&mut self, catch_up: bool) {
        self.show.set_catch_up(catch_up);
    }
}

impl Client {
    /// Shows the inspector for the card the last frame found under the pointer, or none, and
    /// lays it out again if the card, its place, or the view or prompt (`Inspector::due`) changed
    /// since it was last laid out. It asks for the card's face at full size. It also shows the
    /// word a refused click puts under a permanent, at the table's time `now`.
    pub(super) fn inspect(&mut self, now: u64) {
        let until = self.refusal_frame(now);
        let hovered = self.hovered_slot();
        let visible =
            matches!(self.screen, Screen::Game | Screen::Table) && self.features.sheet.is_none();
        let decision = self.open_decision().map(|decision| decision.id);
        let prompt = self.prompt.as_ref().map(|(_, prompt)| prompt);
        let inspector = self.features.inspector_mut();
        inspector.update(Message::Observe { hovered, until });
        let inspected = if visible {
            inspector.inspected_now(&self.frame, self.show.faces().list(), prompt, decision)
        } else {
            None
        };
        if !inspector.refresh(inspected) {
            return;
        }
        let input = ScreenIn {
            inspected: self.features.inspector().shown(),
            ..self.screen_in()
        };
        let element = inspector::inspector(&input.hud());
        let place = inspector::place(&input.hud());
        let shown = element.is_some();
        self.layer.float(floats::INSPECTOR, element);
        if let (true, Some(place)) = (shown, place) {
            self.layer.set_float_look(
                floats::INSPECTOR,
                FloatLook {
                    point: place.corner,
                    ..FloatLook::default()
                },
            );
            if let Some(inspected) = self.features.inspector().shown() {
                self.show.faces_mut().want_full(inspected.face);
            }
        }
    }

    /// The slot of the card the last frame found under the pointer, whether or not the
    /// inspector reads it.
    pub(super) fn hovered_slot(&self) -> Option<ObjectId> {
        let slot = self.show.pointer().hovered()?;
        u16::try_from(slot).ok().map(ObjectId)
    }
}
