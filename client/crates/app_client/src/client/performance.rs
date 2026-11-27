//! The host's frame rate readout floats over the current screen.

use app_screens::performance::MonitorView;
use app_screens::screens::{Screen, settings::GEAR_INSET};
use glam::Vec2;
use kit_base::data::profiles::SlotName;
use kit_base::geometry::Window;
use kit_base::tokens::{size, space};
use kit_scene::SizeClass;
use kit_ui::element::floats;
use kit_ui::layout::FloatLook;

use super::Client;
use crate::FrameRates;

impl Client {
    /// Whether the host should measure frame delivery.
    #[must_use]
    pub fn fps_monitor_enabled(&self) -> bool {
        self.settings.fps_monitor
    }

    /// Shows a new sample from the host. Updating the labels can lay out text, so the
    /// host calls this before preparing the frame and only when the displayed values change.
    pub fn set_frame_rates(&mut self, rates: FrameRates) {
        if self.fps_monitor_enabled() && self.features.update_fps_monitor(rates) {
            self.refresh_frame_rates();
        }
    }

    /// Leaves room above the opponent's strip in compact windows, where the same corner
    /// already holds the opponent's piles and HUD.
    pub(super) fn layout_window(&self) -> Window {
        let mut window = self.window;
        if self.fps_monitor_enabled() && window.size_class == Some(SizeClass::Compact) {
            window.safe_area.top += size::FPS_MONITOR_BAND;
        }
        window
    }

    pub(super) fn apply_frame_monitor_setting(&mut self) {
        if !self.fps_monitor_enabled() {
            self.features.update_fps_monitor(FrameRates::default());
            self.layer.float(floats::FRAME_RATES, None);
        }
        if self.window.size_class == Some(SizeClass::Compact) {
            self.resize(self.window);
        } else {
            self.refresh_frame_rates();
        }
    }

    pub(super) fn refresh_frame_rates(&mut self) {
        if !self.fps_monitor_enabled() {
            return;
        }
        let compact = self.window.size_class == Some(SizeClass::Compact);
        let monitor = self.features.fps_monitor_view(MonitorView {
            compact,
            tier: self.tier,
        });
        self.layer.float(floats::FRAME_RATES, Some(monitor));
        self.place_frame_rates(compact);
    }

    fn place_frame_rates(&mut self, compact: bool) {
        let safe = self.window.safe_area;
        let top = if compact { space::S } else { GEAR_INSET };
        let mut point = Vec2::new(safe.left + GEAR_INSET, safe.top + top);
        if matches!(self.features.screen, Screen::Game | Screen::Table)
            && let Some(log) = self.show.table().profile().slot(SlotName::LogToggle)
        {
            // Keep the log control visible in the same corner.
            point.x = point.x.max(log.rect.x + size::ICON_BUTTON + space::M);
            point.y = point.y.max(log.rect.y);
        }
        self.layer.set_float_look(
            floats::FRAME_RATES,
            FloatLook {
                point,
                ..FloatLook::default()
            },
        );
    }
}
