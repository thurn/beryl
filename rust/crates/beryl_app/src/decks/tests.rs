use std::fs;

use super::*;

#[test]
fn pasted_decks_keep_unknown_and_skipped_names_in_their_profile() {
    let directory = tempfile::tempdir().unwrap();
    let first = Decks::new(directory.path().join("first/decks"));
    let second = Decks::new(directory.path().join("second/decks"));
    let reply = decks::import(
        &first,
        DeckImportRequest {
            name: "Practice".to_owned(),
            text: Some("3 island\n2 My Card\nInvalid line\nSB: 1 Plains".to_owned()),
            cod: None,
        },
    )
    .unwrap();

    assert_eq!(
        reply.main,
        [("island".to_owned(), 3), ("My Card".to_owned(), 2)]
    );
    assert_eq!(reply.side, [("Plains".to_owned(), 1)]);
    assert_eq!(reply.unknown, ["My Card"]);
    assert_eq!(reply.skipped, ["Invalid line"]);
    assert_eq!(
        decks::list(&first).unwrap(),
        [SavedDeck {
            deck: reply,
            main_count: 5,
            side_count: 1,
        }]
    );
    assert!(decks::list(&second).unwrap().is_empty());
    let request = DeckDeleteRequest {
        name: "Practice".to_owned(),
    };
    decks::delete(&first, &request).unwrap();
    assert!(decks::list(&first).unwrap().is_empty());
}

#[test]
fn an_opened_cockatrice_deck_uses_the_requested_save_name() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Deck.COD");
    fs::write(
        &path,
        r#"<cockatrice_deck><deckname>Original</deckname>
        <zone name="main"><card name="Island" number="4"/></zone>
        </cockatrice_deck>"#,
    )
    .unwrap();
    let DeckOpenReply::Cod { text } = decks::read_file(&path).unwrap() else {
        panic!("expected a Cockatrice deck");
    };
    let library = Decks::new(directory.path().join("decks"));
    let reply = decks::import(
        &library,
        DeckImportRequest {
            name: "Renamed".to_owned(),
            text: None,
            cod: Some(text),
        },
    )
    .unwrap();
    assert_eq!(reply.name, "Renamed");
    assert_eq!(reply.main, [("Island".to_owned(), 4)]);
    assert!(reply.skipped.is_empty());

    for extension in ["txt", "dec"] {
        let path = path.with_extension(extension);
        fs::write(&path, "4 Island\n").unwrap();
        assert_eq!(
            decks::read_file(&path).unwrap(),
            DeckOpenReply::Text {
                text: "4 Island\n".to_owned()
            }
        );
    }
}

#[test]
fn search_returns_printed_characteristics_and_token_printings() {
    let [card]: [CardSearchHit; 1] = decks::search(&CardSearchRequest {
        text: "delver of secrets".to_owned(),
        tokens: false,
        limit: 1,
    })
    .try_into()
    .unwrap();
    assert_eq!(card.name, "Delver of Secrets // Insectile Aberration");
    assert!(card.type_line.starts_with("Creature"));
    assert_eq!(card.mana_cost.as_deref(), Some("{U}"));
    assert_eq!(card.power.as_deref(), Some("1"));
    assert_eq!(card.toughness.as_deref(), Some("1"));
    assert!(card.image.ends_with(&format!("{}.jpg", card.scryfall_id)));

    let links = decks::tokens(&CardTokensRequest {
        name: "get lost".to_owned(),
    });
    let search = decks::search(&CardSearchRequest {
        text: "Map Token".to_owned(),
        tokens: true,
        limit: 20,
    });
    assert!(links.iter().any(|link| {
        search
            .iter()
            .any(|hit| link.name == hit.name && link.scryfall_id == hit.scryfall_id)
    }));
}
