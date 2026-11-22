//! Dispatches messages received from the flat layer and the client's own controls.

use super::Client;
use app_screens::screens::GalleryMessage;
use kit_ui::element::Action;
use kit_ui::layout::DismissMenu;

impl Client {
    /// Carries a flat-layer message to the feature that handles it.
    pub fn dispatch(&mut self, action: &Action) {
        if let Some(changed) = self.route_feature(action) {
            if changed {
                self.show_screen();
            }
        } else if let Some(&press) = action.downcast_ref::<GalleryMessage>() {
            match press {
                GalleryMessage::GalleryMenu(kind) => {
                    let at = self.mouse().unwrap_or_else(|| {
                        glam::Vec2::new(self.window.size.width, self.window.size.height) / 2.0
                    });
                    self.features.gallery.menu = Some((kind, at));
                    self.show_screen();
                }
                GalleryMessage::GalleryQuery(query) => {
                    self.features.gallery.query = query;
                    self.show_screen();
                }
                GalleryMessage::DismissMenu => self.dismiss_menu(),
                GalleryMessage::GalleryMenuChoice => {}
            }
        } else if action.downcast_ref::<DismissMenu>().is_some() {
            self.dismiss_menu();
        }
    }

    pub(super) fn set_slider(&mut self, press: &Action, rect: kit_ui::input::Rect, x: f32) {
        use app_screens::screens::settings::{Message, slider_setting};
        if let Some(setting) = slider_setting(press)
            && let Some(level) = setting.level()
        {
            let to = kit_ui::widgets::level_at(x, rect.x, rect.width);
            if to != level {
                self.dispatch(&Action::new(Message::Change(setting.at_level(to))));
            }
        }
    }

    pub(super) fn dismiss_menu(&mut self) {
        if self.features.gallery.menu.take().is_some() {
            self.show_screen();
        }
        self.close_table_menu();
    }
}
