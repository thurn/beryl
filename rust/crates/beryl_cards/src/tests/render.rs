//! Every card renders to rules text (plan K13).

use crate::CARDS;

/// Every card, written in a file or complete from the card store, renders each face without
/// a panic, with a line for each paragraph and no placeholder: no card's ability is a Rust
/// body, and each delayed or granted ability is found on its face. How closely the text
/// matches Oracle's is measured elsewhere, against Scryfall.
#[test]
fn every_card_renders_to_rules_text() {
    for def in CARDS.cards().iter().chain(CARDS.from_store()) {
        for face in def.face_ixs() {
            let text = beryl_card::render::render_face(def, face);
            assert!(
                !text.contains('[') && !text.lines().any(str::is_empty),
                "{}: {text}",
                def.face_name(face)
            );
        }
    }
}
