//! Messages from the gallery and the game's undo button.

use super::{GalleryMenu, GalleryQuery};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenPress {
    GalleryMenu(GalleryMenu),
    GalleryQuery(Option<GalleryQuery>),
    DismissMenu,
    Undo,
}
