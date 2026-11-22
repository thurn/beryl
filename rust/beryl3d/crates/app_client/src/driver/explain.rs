//! Enables and reads the inspection probe during replay.

use super::{Duration, Page};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Capture {
    None,
    Scripts,
    Explanation,
}

impl Page {
    /// Whether later frames record their elements' looks (`Client::set_looking`), until a restart.
    pub fn set_looking(&mut self, looking: bool) {
        self.client.set_looking(looking);
    }

    /// Enables inspection before replay so that screen layout records its chosen slots.
    pub fn enable_explanation(&mut self) {
        let now = self.now;
        self.capture = Capture::Explanation;
        self.restart();
        self.step(Duration(now));
    }

    /// Reads provenance after the frame at this moment has been built.
    #[must_use]
    pub fn explain(&self, element: &kit_frame::frame::Resolved) -> serde_json::Value {
        self.client.explain(element)
    }
}
