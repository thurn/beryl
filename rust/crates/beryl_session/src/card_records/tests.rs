use std::path::Path;

use beryl_card_store::STORE;
use beryl_cards::CARDS;
use serde_json::Value;

use super::{CARD_BACK_ID, CardRecordsRequest, card_records, card_records_among};

/// Every name the engine gives a face, token or emblem: the names the UI asks for.
fn engine_names() -> Vec<&'static str> {
    let mut defs: Vec<_> = CARDS.cards().to_vec();
    let mut i = 0;
    while i < defs.len() {
        for &token in defs[i].tokens {
            if !defs.iter().any(|d| std::ptr::eq(*d, token)) {
                defs.push(token);
            }
        }
        i += 1;
    }
    let mut names: Vec<_> = defs
        .iter()
        .flat_map(|d| d.face_ixs().map(|f| d.face_name(f)))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The UI draws a card, token or emblem by the name the engine gives it, a face's name
/// (`ui/src/cards.ts`): the store must answer each.
#[test]
fn every_face_token_and_emblem_name_is_in_the_card_store() {
    let names = engine_names();
    let found = card_records(&CardRecordsRequest {
        names: names.iter().map(|&n| n.to_owned()).collect(),
    });
    let missing: Vec<_> = names
        .into_iter()
        .filter(|n| !found.cards.contains_key(*n) && !found.tokens.contains_key(*n))
        .collect();
    assert!(
        missing.is_empty(),
        "not in data/card-store.tsv (run `just cards`): {missing:?}"
    );
}

fn cards_json() -> Value {
    let crate_dir = std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo runs the tests");
    let path = Path::new(&crate_dir).join("../../../data/cards.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Strips Scryfall's `?<timestamp>` from an image URL: the store keeps nothing volatile.
fn without_timestamps(value: &mut Value) {
    match value {
        Value::String(s) => {
            if let Some((url, _)) = s.split_once(".jpg?") {
                *s = format!("{url}.jpg");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(without_timestamps),
        Value::Object(map) => {
            map.remove("made_by");
            map.values_mut().for_each(without_timestamps);
        }
        _ => {}
    }
}

/// A record is served as data/cards.json holds the card, so the UI's types need not change.
/// (This goes when the UI stops bundling data/cards.json.)
#[test]
fn records_are_served_as_the_bundled_card_data_holds_them() {
    let mut bundled = cards_json();
    without_timestamps(&mut bundled);
    let cards = bundled["cards"].as_array().unwrap();
    let names: Vec<String> = cards
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .collect();
    let found = card_records(&CardRecordsRequest { names });
    assert_eq!(found.card_back, bundled["card_back"]);
    for card in cards {
        let served = serde_json::to_value(&found.cards[card["name"].as_str().unwrap()]).unwrap();
        assert_eq!(&served, card);
    }
    for token in bundled["tokens"].as_array().unwrap() {
        let name = token["name"].as_str().unwrap();
        let printings = STORE.tokens(name);
        let id = token["scryfall_id"].as_str().unwrap();
        assert!(printings.iter().any(|t| t.scryfall_id == id), "{name}");
    }
    assert_eq!(
        STORE.card("Island").unwrap().card_back_id.as_deref(),
        Some(CARD_BACK_ID)
    );
}

#[test]
fn a_fixture_s_strings_find_the_names_among_them() {
    let found = card_records_among(["Island", "Main phase", "Island", "Samurai Token", ""]);
    assert_eq!(found.cards.keys().collect::<Vec<_>>(), ["Island"]);
    assert_eq!(found.tokens.keys().collect::<Vec<_>>(), ["Samurai Token"]);
}
