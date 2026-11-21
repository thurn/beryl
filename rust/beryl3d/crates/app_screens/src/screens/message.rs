//! Messages from the gallery.

use super::{GalleryMenu, GalleryQuery};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenPress {
    GalleryMenu(GalleryMenu),
    GalleryQuery(Option<GalleryQuery>),
    DismissMenu,
}
