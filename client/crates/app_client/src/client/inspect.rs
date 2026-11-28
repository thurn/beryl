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
use app_screens::screens::Screen;
use game_hud::hud::inspector;

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
        // The bug report freezes the table as it stands, hover content included, so that the
        // player can report what they were pointing at: the pointer keeps the position it had
        // when the report opened (`table_mouse`).
        let visible = matches!(self.features.screen, Screen::Game | Screen::Table)
            && self.table_shown();
        eprintln!("INSPECT hovered {:?} visible {visible} bug {}", hovered, self.bug_open());
        let decision = self.open_decision().map(|decision| decision.id);
        let view = self
            .show
            .latest()
            .map(|update| &update.view)
            .filter(|_| visible);
        let face = self.features.draw_inspector(inspector::Draw {
            layer: &mut self.layer,
            frame: &self.frame,
            faces: self.show.faces().list(),
            profile: self.show.table().profile(),
            view,
            prompt: None,
            decision,
            hovered,
            until,
        });
        if let Some(face) = face {
            self.show.faces_mut().want_full(face);
        }
    }

    /// The slot of the card the last frame found under the pointer, whether or not the
    /// inspector reads it.
    pub(super) fn hovered_slot(&self) -> Option<ObjectId> {
        if self.bug_open() {
            return self.reported_hover;
        }
        let slot = self.show.pointer().hovered()?;
        u16::try_from(slot).ok().map(ObjectId)
    }
}
