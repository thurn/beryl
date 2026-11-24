//! Integration tests for frame construction.

/// A card's printed size from the repository's `card.json`, the size frames are built with. It
/// is read once, so that a test counting a frame's allocations counts none for it.
fn card() -> glam::Vec2 {
    static CARD: std::sync::OnceLock<glam::Vec2> = std::sync::OnceLock::new();
    *CARD.get_or_init(|| {
        let text = include_str!("../../../../../data/client/card.json");
        kit_base::data::Card::parse(text)
            .expect("repository card")
            .size()
    })
}

mod keep_only;
mod order;
