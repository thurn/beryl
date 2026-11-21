//! The integration tests of `beryl_render`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

#[path = "../../../../test_profiles.rs"]
mod test_profiles;

use test_profiles::SHIPPED;

/// A card's printed size from the repository's `card.json`, the size frames are built with.
fn card() -> glam::Vec2 {
    card_data().size()
}

/// The repository's `card.json`.
fn card_data() -> kit_base::data::Card {
    static CARD: std::sync::OnceLock<kit_base::data::Card> = std::sync::OnceLock::new();
    *CARD.get_or_init(|| {
        let text = include_str!("../../../../../../data/beryl3d/card.json");
        kit_base::data::Card::parse(text).expect("repository card")
    })
}

mod bloom;
mod enhanced;
mod faces;
mod light;
mod render;
mod shader_cost;
mod shadow;
mod tabletop;
mod text;

fn reference_camera() -> kit_space::camera::CameraData {
    use kit_base::data::profiles::Profiles;

    Profiles::parse(&SHIPPED, card_data())
        .unwrap()
        .fallback()
        .table
        .camera
}

/// The repository's lighting, `data/beryl3d/lighting.json`, which the hosts hand the renderer.
pub(crate) fn lighting() -> kit_base::data::lighting::Lighting {
    let text = beryl_test_files::read_to_string("data/beryl3d/lighting.json")
        .expect("read the lighting tuning");
    kit_base::data::lighting::Lighting::parse(&text).expect("the repository's lighting")
}
