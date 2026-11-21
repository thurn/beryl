//! Messages from the gallery.

use super::{GalleryMenu, GalleryQuery};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GalleryMessage {
    GalleryMenu(GalleryMenu),
    GalleryQuery(Option<GalleryQuery>),
    DismissMenu,
    GalleryMenuChoice,
}
