use super::{STORE, Side, Store};

fn record(name: &str, id: &str) -> String {
    format!(
        r#"{{"name":"{name}","oracle_id":"o","scryfall_id":"{id}","set":"s","collector_number":"1","layout":"normal","faces":[{{"name":"{name}","type_line":"Creature","colors":[],"side":"front"}}]}}"#
    )
}

/// A store of these `key\tvalue` lines, sorted as `just cards` writes them.
fn store(mut lines: Vec<String>) -> Vec<u8> {
    lines.sort_unstable();
    lines
        .into_iter()
        .flat_map(|l| format!("{l}\n").into_bytes())
        .collect()
}

#[test]
fn a_lookup_in_fifty_thousand_records_probes_at_most_32_lines() {
    let names: Vec<String> = (0..50_000).map(|i| format!("Card {i:05}")).collect();
    let bytes = store(
        names
            .iter()
            .map(|n| format!("card:{n}\t{}", record(n, n)))
            .collect(),
    );
    let store = Store::new(&bytes);
    let mut most = 0;
    for name in &names {
        let (found, probes) = store.values(&format!("card:{name}"));
        assert_eq!(found.len(), 1, "{name}");
        most = most.max(probes);
    }
    for missing in [
        "card:",
        "card:Card 0",
        "card:Card 49999 ",
        "card:Zz",
        "token:A",
    ] {
        let (found, probes) = store.values(missing);
        assert!(found.is_empty(), "{missing}");
        most = most.max(probes);
    }
    assert!(most <= 32, "a lookup probed {most} lines");
    assert_eq!(store.card("Card 31415").unwrap().scryfall_id, "Card 31415");
}

#[test]
fn opening_parses_nothing_and_a_lookup_parses_only_what_it_returns() {
    // Every other value is not JSON at all: a store that parsed on opening, or parsed the lines
    // a search passes over, would fail.
    let mut lines: Vec<String> = (0..1000)
        .map(|i| format!("card:Junk {i:04}\tnot json"))
        .collect();
    lines.push(format!("card:Junk 0500 Found\t{}", record("Found", "f")));
    let bytes = store(lines);
    let store = Store::new(&bytes);
    assert_eq!(store.card("Junk 0500 Found").unwrap().name, "Found");
    assert!(store.card("Junk 0500 Missing").is_none());
}

#[test]
fn aliases_prefixes_and_shared_token_names() {
    let bytes = store(vec![
        format!("card:A\t{}", record("A", "a")),
        format!("card:A B\t{}", record("A B", "ab")),
        format!("card:Ab\t{}", record("Ab", "ab2")),
        format!("card:Æther\t{}", record("Æther", "ae")),
        format!("card:Fire // Ice\t{}", record("Fire // Ice", "fi")),
        r#"card:Fire	{"alias":"Fire // Ice","scryfall_id":"fi"}"#.to_owned(),
        format!("token:A\t{}", record("A", "t1")),
        format!("token:A\t{}", record("A", "t2")),
        format!("token:X // Y\t{}", record("X // Y", "x1")),
        format!("token:X // Y\t{}", record("X // Y", "x2")),
        r#"token:X	{"alias":"X // Y","scryfall_id":"x2"}"#.to_owned(),
    ]);
    let store = Store::new(&bytes);
    for (name, id) in [("A", "a"), ("A B", "ab"), ("Ab", "ab2"), ("Æther", "ae")] {
        assert_eq!(store.card(name).unwrap().scryfall_id, id, "{name}");
    }
    assert_eq!(store.card("Fire"), store.card("Fire // Ice"));
    let ids = |found: Vec<super::Record>| -> Vec<String> {
        found.into_iter().map(|r| r.scryfall_id).collect()
    };
    assert_eq!(ids(store.tokens("A")), ["t1", "t2"]);
    assert_eq!(ids(store.tokens("X")), ["x2"]);
    assert!(store.tokens("A B").is_empty());
    assert!(store.card("A ").is_none());
    assert!(store.card("").is_none());
}

#[test]
fn the_committed_store_answers_by_card_face_and_token_name() {
    let jace = STORE.card("Jace, Vryn's Prodigy").unwrap();
    assert_eq!(jace.name, "Jace, Vryn's Prodigy // Jace, Telepath Unbound");
    assert_eq!(STORE.card("Jace, Telepath Unbound"), Some(jace.clone()));
    assert_eq!(
        STORE.card_ignoring_case("jace, telepath unbound"),
        Some(jace.clone())
    );
    assert_eq!(jace.faces[1].side, Side::Back);
    assert_eq!(jace.faces[1].loyalty.as_deref(), Some("5"));
    // The first printing places the card's file; a token has none.
    assert_eq!(jace.first_set.as_deref(), Some("ori"));
    assert_eq!(jace.first_released.as_deref(), Some("2015-07-17"));
    let emblem = &jace.tokens[0];
    assert!(
        STORE
            .tokens(&emblem.name)
            .iter()
            .any(|t| t.scryfall_id == emblem.scryfall_id)
    );
    // A card and the token it makes share a name, and stay apart.
    let trainer = STORE.card("Thundertrap Trainer").unwrap();
    let offspring = STORE.tokens("Thundertrap Trainer");
    assert_eq!(offspring.len(), 1);
    assert_ne!(trainer.scryfall_id, offspring[0].scryfall_id);
    assert!(offspring[0].first_set.is_none());
    assert!(STORE.tokens("Treasure Token").len() > 1);
    assert_eq!(
        STORE.search_tokens("treasure token", 1),
        STORE.tokens("Treasure Token")[..1]
    );
    assert!(STORE.card("Nonexistent Card").is_none());
}

#[test]
fn card_search_returns_prefixes_before_substrings_in_name_order() {
    let bytes = store(vec![
        format!("card:Ash Bolt\t{}", record("Ash Bolt", "ash")),
        format!("card:bolt Apprentice\t{}", record("bolt Apprentice", "app")),
        format!("card:Bolt Dragon\t{}", record("Bolt Dragon", "dragon")),
        format!("card:Élan Bolt\t{}", record("Élan Bolt", "elan")),
        "card:Unrelated\tinvalid JSON".to_owned(),
        format!("token:Bolt Token\t{}", record("Bolt Token", "token")),
    ]);
    let store = Store::new(&bytes);
    let names = |query, limit| {
        store
            .search(query, limit)
            .into_iter()
            .map(|record| record.name)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        names("bOLt", 10),
        ["bolt Apprentice", "Bolt Dragon", "Ash Bolt", "Élan Bolt"]
    );
    assert_eq!(
        names("BOLT", 3),
        ["bolt Apprentice", "Bolt Dragon", "Ash Bolt"]
    );
    assert_eq!(names("élAN", 1), ["Élan Bolt"]);
    assert!(names("Bolt", 0).is_empty());
    assert!(names("", 20).is_empty());
    assert!(names("missing", 20).is_empty());
}

#[test]
fn token_search_keeps_distinct_printings_and_skips_aliases() {
    let bytes = store(vec![
        format!("card:Gem\t{}", record("Gem", "card")),
        format!("token:A Gem\t{}", record("A Gem", "substring")),
        format!("token:Gem\t{}", record("Gem", "token-1")),
        format!("token:Gem\t{}", record("Gem", "token-2")),
        r#"token:Gem Alias	{"alias":"Gem","scryfall_id":"token-1"}"#.to_owned(),
    ]);
    let store = Store::new(&bytes);
    let ids = |query, limit| {
        store
            .search_tokens(query, limit)
            .into_iter()
            .map(|record| record.scryfall_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids("gEM", 4), ["token-1", "token-2", "substring"]);
    assert_eq!(ids("gem", 1), ["token-1"]);
}

#[test]
fn exact_case_insensitive_lookup_resolves_card_face_aliases() {
    let bytes = store(vec![
        format!("card:Élan // Bolt\t{}", record("Élan // Bolt", "card")),
        r#"card:Élan	{"alias":"Élan // Bolt","scryfall_id":"card"}"#.to_owned(),
        format!("token:Élan\t{}", record("Élan", "token")),
    ]);
    let store = Store::new(&bytes);
    let card = store.card("Élan // Bolt").unwrap();
    assert_eq!(store.card_ignoring_case("éLAN"), Some(card.clone()));
    assert_eq!(store.card_ignoring_case("éLAN // bOLT"), Some(card.clone()));
    assert!(store.card_ignoring_case("él").is_none());
    assert_eq!(store.search("élan", 2), [card]);
}
