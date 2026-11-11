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
    assert_eq!(jace.faces[1].side, Side::Back);
    assert_eq!(jace.faces[1].loyalty.as_deref(), Some("5"));
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
    assert!(STORE.tokens("Treasure Token").len() > 1);
    assert!(STORE.card("Nonexistent Card").is_none());
}
