//! Integration tests for table-space motion.

/// A card's printed size from the repository's `card.json`, the size frames are built with.
fn card() -> glam::Vec2 {
    card_data().size()
}

/// The repository's `card.json`.
fn card_data() -> kit_base::data::Card {
    static CARD: std::sync::OnceLock<kit_base::data::Card> = std::sync::OnceLock::new();
    *CARD.get_or_init(|| {
        let text = include_str!("../../../../../data/client/card.json");
        kit_base::data::Card::parse(text).expect("repository card")
    })
}

mod motion;

mod arrange;

mod cascade_fan;

mod fan_rows;
