use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use beryl_card_store::STORE;
use beryl_table::deck::{DeckError, Decks, counts, parse_cod, parse_text, unknown};

#[test]
fn imports_the_documented_deck_list() {
    let deck = parse_text(
        "4 Brainstorm
4x Force of Will
1 Fable of the Mirror-Breaker // Reflection of Kiki-Jiki
3 Murktide Regent (MH2) 52

Sideboard
2 Surgical Extraction
SB: 1 Flusterstorm",
    );
    assert_eq!(
        deck.main,
        [
            ("Brainstorm".into(), 4),
            ("Force of Will".into(), 4),
            (
                "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki".into(),
                1
            ),
            ("Murktide Regent".into(), 3),
        ]
    );
    assert_eq!(
        deck.side,
        [
            ("Surgical Extraction".into(), 2),
            ("Flusterstorm".into(), 1)
        ]
    );
    assert_eq!(counts(&deck), (12, 3));
    assert!(deck.skipped.is_empty());
    assert!(unknown(&deck, &STORE).is_empty());
}

#[test]
fn text_forms_work_independently() {
    for (input, expected) in [
        ("4 Brainstorm", ("Brainstorm", 4)),
        ("4x Force of Will", ("Force of Will", 4)),
        (
            "1 Fable of the Mirror-Breaker // Reflection of Kiki-Jiki",
            ("Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", 1),
        ),
        (
            "1 Fable of the Mirror-Breaker",
            ("Fable of the Mirror-Breaker", 1),
        ),
        ("3 Murktide Regent (MH2) 52", ("Murktide Regent", 3)),
        (
            "1 Some Custom Card (Prototype)",
            ("Some Custom Card (Prototype)", 1),
        ),
    ] {
        let deck = parse_text(input);
        assert_eq!(deck.main, [(expected.0.to_owned(), expected.1)], "{input}");
        assert!(deck.side.is_empty(), "{input}");
        assert!(deck.skipped.is_empty(), "{input}");
    }
}

#[test]
fn text_sideboard_marker_on_one_line_does_not_change_the_next_line() {
    for input in [
        "1 Brainstorm\n\n2 Surgical Extraction",
        "1 Brainstorm\nSideboard\n2 Surgical Extraction",
        "SB: 2 Surgical Extraction\n1 Brainstorm",
    ] {
        let deck = parse_text(input);
        assert_eq!(deck.main, [("Brainstorm".into(), 1)], "{input}");
        assert_eq!(deck.side, [("Surgical Extraction".into(), 2)], "{input}");
        assert!(deck.skipped.is_empty(), "{input}");
    }
}

#[test]
fn reports_skipped_lines_and_keeps_unknown_names() {
    let deck = parse_text(
        "\nDeck copied from a site\n4 bRaInStOrM\n1 fable of the mirror-breaker\n\
         count missing\n0 Brainstorm\n2xx Brainstorm\n1 Brainstor\n1 My Custom Card\n\
         SB: 1 MY CUSTOM CARD",
    );
    assert_eq!(counts(&deck), (7, 1));
    assert_eq!(
        deck.skipped,
        [
            "Deck copied from a site",
            "count missing",
            "0 Brainstorm",
            "2xx Brainstorm"
        ]
    );
    assert_eq!(unknown(&deck, &STORE), ["Brainstor", "My Custom Card"]);
}

const COCKATRICE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<cockatrice_deck version="1">
    <deckname>Café &amp; Dragons</deckname>
    <comments>Deck made in Cockatrice</comments>
    <!-- Cards in a comment are not entries: <card number="8" name="Fake"/> -->
    <zone name="main">
        <card number="4" name="Brainstorm" setShortName="ICE"/>
        <card name='Dragon&#39;s Rage Channeler' number='3'/>
        <card name="Example &lt; &amp; > &#x263A;" number="1"/>
    </zone>
    <zone name="side">
        <card number="2" name="Surgical Extraction"/>
    </zone>
    <zone name="maybeboard"><card number="1" name="Force of Will"/></zone>
    <sideboard_plan><zone name="main"><card number="1" name="Flusterstorm"/></zone></sideboard_plan>
</cockatrice_deck>
"#;

#[test]
fn imports_cockatrice_name_and_zones() {
    let deck = parse_cod(COCKATRICE).unwrap();
    assert_eq!(deck.name, "Café & Dragons");
    assert_eq!(
        deck.main,
        [
            ("Brainstorm".into(), 4),
            ("Dragon's Rage Channeler".into(), 3),
            ("Example < & > ☺".into(), 1),
        ]
    );
    assert_eq!(deck.side, [("Surgical Extraction".into(), 2)]);
    assert_eq!(unknown(&deck, &STORE), ["Example < & > ☺"]);
}

#[test]
fn rejects_an_invalid_cockatrice_deck() {
    for input in [
        "1 Brainstorm",
        "<other/>",
        "<cockatrice_deck><zone name='main'></cockatrice_deck>",
        "<cockatrice_deck><zone name='main'><card name='Brainstorm'/></zone></cockatrice_deck>",
        "<cockatrice_deck><zone name='main'><card number='70000' name='Brainstorm'/></zone></cockatrice_deck>",
        "<cockatrice_deck><zone name='main'><card number='1' name='&custom;'/></zone></cockatrice_deck>",
    ] {
        assert!(
            matches!(parse_cod(input), Err(DeckError::InvalidCod)),
            "{input}"
        );
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("beryl-table-decks-{}-{now}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn saves_lists_replaces_loads_and_deletes_decks_by_name() {
    let scratch = Scratch::new();
    let directory = scratch.0.join("saved");
    let decks = Decks::new(directory.clone());
    assert!(decks.list().unwrap().is_empty());
    assert!(matches!(
        decks.save(&parse_text("1 Brainstorm")),
        Err(DeckError::EmptyName)
    ));

    let mut pasted = parse_text("Not a card line\n1 My Custom Card");
    pasted.name = "../Café".to_owned();
    decks.save(&pasted).unwrap();

    let imported = parse_cod(COCKATRICE).unwrap();
    decks.save(&imported).unwrap();
    fs::write(directory.join("unrelated.json"), "{}").unwrap();

    let reopened = Decks::new(directory);
    assert_eq!(reopened.list().unwrap(), ["../Café", "Café & Dragons"]);
    assert_eq!(reopened.load("../Café").unwrap(), pasted);
    assert_eq!(reopened.load("Café & Dragons").unwrap(), imported);
    assert!(!scratch.0.join("Café.json").exists());

    pasted.main.clear();
    reopened.save(&pasted).unwrap();
    assert_eq!(reopened.load("../Café").unwrap(), pasted);
    reopened.delete("../Café").unwrap();
    assert!(reopened.load("../Café").is_err());
    assert_eq!(reopened.list().unwrap(), ["Café & Dragons"]);
}
