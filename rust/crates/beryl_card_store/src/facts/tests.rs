use super::*;
use crate::STORE;

#[test]
fn front_types_place_creature_lands_in_the_creature_row() {
    let arbor = STORE.card("Dryad Arbor").unwrap();
    assert!(is_creature(&arbor));
    assert!(is_land(&arbor));
    assert!(is_permanent(&arbor));
    assert_eq!(row(&arbor), Row3::Creatures);

    let temple = STORE.card("Temple of Epiphany").unwrap();
    assert!(is_land(&temple));
    assert!(!is_creature(&temple));
    assert_eq!(row(&temple), Row3::Lands);

    let battle = STORE.card("Invasion of Amonkhet").unwrap();
    assert!(!is_creature(&battle));
    assert!(is_permanent(&battle));
    assert_eq!(row(&battle), Row3::Others);

    let jace = STORE.card("Jace, the Mind Sculptor").unwrap();
    assert!(is_planeswalker(&jace));
    assert!(is_legendary(&jace));
    assert!(!is_legendary(&arbor));
    assert!(!is_planeswalker(&arbor));
    assert!(!is_permanent(&STORE.card("Gitaxian Probe").unwrap()));
}

#[test]
fn attachment_subtypes_come_from_the_front_type_line() {
    for name in ["All That Glitters", "Swiftfoot Boots", "Darksteel Garrison"] {
        assert!(is_attachment(&STORE.card(name).unwrap()), "{name}");
    }
    assert!(!is_attachment(&STORE.card("Black Lotus").unwrap()));
}

#[test]
fn displayed_stats_apply_counters_to_printed_or_manual_values() {
    let nightmare = STORE.card("Nightmare").unwrap();
    let printed = printed_stats(&nightmare.faces[0]);
    assert_eq!(printed, Some((0, 0)));
    let arbor = STORE.card("Dryad Arbor").unwrap();
    assert_eq!(printed_stats(&arbor.faces[0]), Some((1, 1)));

    assert_eq!(shown_stats(printed, None, 2, 1), Some((1, 1)));
    assert_eq!(shown_stats(printed, Some((4, 5)), 1, 2), Some((3, 4)));

    let temple = STORE.card("Temple of Epiphany").unwrap();
    assert_eq!(printed_stats(&temple.faces[0]), None);
    assert_eq!(shown_stats(None, None, 1, 0), None);
    assert_eq!(shown_stats(None, Some((2, 3)), 1, 0), Some((3, 4)));
}

#[test]
fn starting_counters_come_from_printed_loyalty_or_defense() {
    let jace = STORE.card("Jace, the Mind Sculptor").unwrap();
    let battle = STORE.card("Invasion of Amonkhet").unwrap();
    let arbor = STORE.card("Dryad Arbor").unwrap();
    assert_eq!(starting_counters(&jace.faces[0]), Some(("loyalty", 3)));
    assert_eq!(starting_counters(&battle.faces[0]), Some(("defense", 4)));
    assert_eq!(starting_counters(&arbor.faces[0]), None);
}

#[test]
fn enters_tapped_ignores_reminder_text_and_sentences_with_unless() {
    let temple = STORE.card("Temple of Epiphany").unwrap();
    let fortress = STORE.card("Glacial Fortress").unwrap();
    assert!(enters_tapped(&temple.faces[0]));
    assert!(!enters_tapped(&fortress.faces[0]));
    let donatello = STORE.card("Donatello, Gadget Master").unwrap();
    assert!(!enters_tapped(&donatello.faces[0]));

    let mut face = temple.faces[0].clone();
    face.oracle_text = Some("(This enters tapped. (Reminder.))\n{T}: Add {U}.".to_owned());
    assert!(!enters_tapped(&face));
    face.oracle_text =
        Some("This enters the battlefield tapped.\nDraw unless you pay {1}.".to_owned());
    assert!(enters_tapped(&face));
}

#[test]
fn mana_value_counts_the_symbols_on_the_face() {
    for (name, expected) in [
        ("Dryad Arbor", 0),
        ("Jace, the Mind Sculptor", 4),
        ("Hangarback Walker", 0),
        ("Spectral Procession", 6),
        ("Gitaxian Probe", 1),
        ("Slippery Bogle", 1),
    ] {
        let card = STORE.card(name).unwrap();
        assert_eq!(mana_value(&card.faces[0]), expected, "{name}");
    }
}

#[test]
fn keywords_and_token_links_come_from_the_record() {
    let walker = STORE.card("Hangarback Walker").unwrap();
    let nightmare = STORE.card("Nightmare").unwrap();
    assert_eq!(keywords(&nightmare), ["Flying"]);
    assert!(has_keyword(&nightmare, "fLYING"));
    assert!(!has_keyword(&nightmare, "Fly"));
    let spider = STORE.card("Spider-Man India").unwrap();
    assert!(has_keyword(&spider, "PAVITR'S SEVĀ"));

    let links = tokens_made(&walker);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].name, "Thopter Token");
    assert!(
        STORE
            .tokens(&links[0].name)
            .iter()
            .any(|token| token.scryfall_id == links[0].scryfall_id)
    );
    assert!(tokens_made(&nightmare).is_empty());
}
