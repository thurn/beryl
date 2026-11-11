use beryl_card_store::STORE;
use beryl_cards::CARDS;

use super::{
    CARD_BACK_ID, CardRecordsRequest, NAMELESS_TOKEN_ID, card_records, card_records_among,
};

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

/// The card back served is the one the store's cards have.
#[test]
fn the_card_back_is_the_one_cards_in_the_store_have() {
    assert_eq!(
        STORE.card("Island").unwrap().card_back_id.as_deref(),
        Some(CARD_BACK_ID)
    );
}

/// The art of a token with no name is a Copy token's.
#[test]
fn the_nameless_token_is_a_copy_token_in_the_store() {
    let copies = STORE.tokens("Copy");
    assert!(copies.iter().any(|t| t.scryfall_id == NAMELESS_TOKEN_ID));
    let found = card_records(&CardRecordsRequest { names: Vec::new() });
    assert!(
        found
            .nameless_token
            .contains("/art_crop/front/6/8/6870a2af-")
    );
}

/// Many tokens share a name: a card's is the one its record links, whichever the store lists
/// first.
#[test]
fn a_token_is_the_printing_its_card_makes() {
    let names = ["Insect Token", "Human Token", "Spirit Token", "Map Token"];
    let found = card_records(&CardRecordsRequest {
        names: names.map(str::to_owned).into(),
    });
    let shown = |name: &str| {
        let face = &found.tokens[name].faces[0];
        (
            face.power.clone(),
            face.toughness.clone(),
            face.colors.join(""),
        )
    };
    let stats = |power: &str, toughness: &str, colors: &str| {
        (
            Some(power.to_owned()),
            Some(toughness.to_owned()),
            colors.to_owned(),
        )
    };
    // Overlord of the Mistmoors' Insects, Adeline's Humans, Staff of the Storyteller's Spirit.
    assert_eq!(shown("Insect Token"), stats("2", "1", "W"));
    assert_eq!(shown("Human Token"), stats("1", "1", "W"));
    assert_eq!(shown("Spirit Token"), stats("1", "1", "W"));
    for (name, maker) in [
        ("Insect Token", "Overlord of the Mistmoors"),
        ("Map Token", "Get Lost"),
    ] {
        let linked = STORE.card(maker).unwrap().tokens;
        assert_eq!(found.tokens[name].scryfall_id, linked[0].scryfall_id);
    }
}

#[test]
fn a_fixture_s_strings_find_the_names_among_them() {
    let found = card_records_among(["Island", "Main phase", "Island", "Samurai Token", ""]);
    assert_eq!(found.cards.keys().collect::<Vec<_>>(), ["Island"]);
    assert_eq!(found.tokens.keys().collect::<Vec<_>>(), ["Samurai Token"]);
}
