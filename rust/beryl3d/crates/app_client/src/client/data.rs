//! Applies tuning from the host and shows a notice when an edit cannot be loaded.

use std::sync::Arc;

use glam::Vec2;

use super::Client;
use app_screens::screens;
use kit_base::data::Data;
use kit_base::tokens::{color, size, type_style};
use kit_ui::element::{floats, row, text};
use kit_ui::layout::FloatLook;

impl Client {
    pub(super) fn set_data(&mut self, data: Arc<Data>) {
        self.layer.float(floats::DATA_ERROR, None);
        if self.data() != &data {
            self.show.set_data(data);
            self.resize(self.window);
            // Badge sizes are part of shaping: shaped badges are shaped again at the new sizes
            // now, so that no frame draws text shaped at the old ones.
            if self.show.badges_mut().shaped() {
                let (text, glyphs) = self.layer.text_and_glyphs();
                self.show.shape_badges(text, glyphs, self.window.scale);
            }
        }
    }

    /// Shows a notice over the current screen until valid tuning arrives.
    pub(super) fn show_data_error(&mut self) {
        let mut notice = row()
            .background(color::TIMING_REFUSAL)
            .border((color::STAT_DOWN, 1.0))
            .radius(size::TIMING_REFUSAL_RADIUS)
            .passes(true)
            .child(text("Data reload failed").style(type_style::TIMING_REFUSAL));
        let (across, down) = (
            size::TIMING_REFUSAL_PADDING_X,
            size::TIMING_REFUSAL_PADDING_Y,
        );
        notice.style.padding = [down, across, down, across];
        self.layer.float(floats::DATA_ERROR, Some(notice));
        self.place_data_error();
    }

    pub(super) fn place_data_error(&mut self) {
        self.layer.set_float_look(
            floats::DATA_ERROR,
            FloatLook {
                point: Vec2::new(
                    self.window.size.width * 0.5,
                    self.window.safe_area.top + screens::GEAR_INSET,
                ),
                anchor: Vec2::new(0.5, 0.0),
                ..FloatLook::default()
            },
        );
    }
}
