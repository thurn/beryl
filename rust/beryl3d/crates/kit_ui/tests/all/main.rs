//! Integration tests of the flat layer.

/// A card's printed size from the repository's `card.json`, the size frames are built with. It
/// is read once, so that a test counting a frame's allocations counts none for it.
fn card() -> glam::Vec2 {
    static CARD: std::sync::OnceLock<glam::Vec2> = std::sync::OnceLock::new();
    *CARD.get_or_init(|| {
        let text = include_str!("../../../../../../data/beryl3d/card.json");
        kit_base::data::Card::parse(text)
            .expect("repository card")
            .size()
    })
}

mod backdrop;
mod elements;
mod paints;
mod transitions;
