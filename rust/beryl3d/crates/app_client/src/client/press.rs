//! Dispatches messages received from the flat layer and the client's own controls.

use super::Client;
use app_screens::screens::ScreenPress;
use kit_ui::element::Action;
use kit_ui::layout::DismissMenu;

impl Client {
    /// Carries a flat-layer message to the feature that handles it.
    pub fn press(&mut self, action: &Action) {
        if let Some(changed) = self.route_feature(action) {
            if changed {
                self.show_screen();
            }
        } else if let Some(&press) = action.downcast_ref::<ScreenPress>() {
            match press {
                ScreenPress::GalleryMenu(kind) => {
                    let at = self.mouse.unwrap_or_else(|| {
                        glam::Vec2::new(self.window.size.width, self.window.size.height) / 2.0
                    });
                    self.gallery.menu = Some((kind, at));
                    self.show_screen();
                }
                ScreenPress::GalleryQuery(query) => {
                    self.gallery.query = query;
                    self.show_screen();
                }
                ScreenPress::DismissMenu => self.dismiss_menu(),
                ScreenPress::GalleryMenuChoice => {}
            }
        } else if action.downcast_ref::<DismissMenu>().is_some() {
            self.dismiss_menu();
        }
    }

    pub(super) fn dismiss_menu(&mut self) {
        if self.gallery.menu.take().is_some() {
            self.show_screen();
        }
        self.close_table_menu();
    }
}
