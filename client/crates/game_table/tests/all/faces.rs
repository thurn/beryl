//! The table's list of unique face images, in face ID order.

use beryl_card_records::{TokenMakers, card_records_among};
use beryl_protocol::testing::ViewBuilder;
use game_table::faces::{CARD_BACK, FaceList};

#[test]
fn the_card_back_is_the_first_face_and_a_picture_is_listed_once() {
    let view = ViewBuilder::sample().build();
    let names = view
        .objects
        .iter()
        .filter_map(|object| object.name.as_deref());
    let records = card_records_among(names, TokenMakers::NONE);
    let list = FaceList::from_records(&records);
    assert_eq!(list.get(CARD_BACK).unwrap().image, records.card_back);
    let island = list.find("Island").expect("Island has a face");
    assert_ne!(island, CARD_BACK);
    let images: Vec<&str> = list.faces().iter().map(|f| f.image.as_str()).collect();
    let mut distinct = images.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(images.len(), distinct.len(), "each picture is listed once");
}

#[test]
fn an_object_with_a_printing_finds_that_printings_face_and_others_find_the_card() {
    let ixs: Vec<_> = beryl_card_records::PRINTINGS
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name == "Island")
        .map(|(i, _)| beryl_card_records::PrintingIx(i as u32))
        .collect();
    assert!(ixs.len() > 1, "the deck has several Island printings");
    let mut records = card_records_among(["Island"], TokenMakers::NONE);
    beryl_card_records::add_printings(&mut records, ixs.iter().copied());
    let list = FaceList::from_records(&records);
    let mut object = ViewBuilder::sample().build().objects[0].clone();
    object.name = Some("Island".to_owned());
    let ordinary = list.find_object(&object).expect("the card has a face");
    let mut shown = Vec::new();
    for &ix in &ixs {
        object.printing = Some(ix);
        let face = list.find_object(&object).expect("the printing has a face");
        assert_ne!(face, ordinary, "a printing is not the ordinary art");
        assert!(!shown.contains(&face), "each printing has its own face");
        shown.push(face);
    }
    object.printing = None;
    assert_eq!(list.find_object(&object), Some(ordinary));
    assert_eq!(
        list.find("Island"),
        Some(ordinary),
        "the name keeps its art"
    );
}

#[test]
fn an_activated_ability_is_found_by_its_colon_outside_quotes_and_bare_reminder_text() {
    use game_table::faces::says_ability;
    let activated = |paragraph| says_ability(paragraph, false);
    assert!(activated("{T}: Add {U}."));
    assert!(activated(
        "Cycling {2} ({2}, Discard this card: Draw a card.)"
    ));
    assert!(!activated("({T}: Add {U}.)"));
    assert!(!activated(
        "Create a Treasure token. (It's an artifact with \"{T}, Sacrifice this artifact: Add one mana of any color.\")"
    ));
    assert!(!activated("Flying"));
}

#[test]
fn an_ability_wears_the_picture_of_the_printing_its_source_object_shows() {
    use beryl_protocol::protocol::{Side, StackItem, ZoneName};
    let name = "Dandân";
    let ix = beryl_card_records::PRINTINGS
        .iter()
        .position(|p| p.name == name)
        .map(|i| beryl_card_records::PrintingIx(i as u32))
        .expect("the Dandân deck has a Dandân printing");
    let mut records = card_records_among([name], TokenMakers::NONE);
    beryl_card_records::add_printings(&mut records, [ix]);
    let list = FaceList::from_records(&records);
    let image = |face| list.get(face).unwrap().image.as_str();
    let printed = list.printing(ix).expect("the printing has a face");
    let ordinary = list.find(name).expect("the card has a face");
    let koi = list
        .plate(name, Some(ix), true, 0)
        .expect("the printing has the plate");
    let plain = list
        .plate(name, None, true, 0)
        .expect("the card has the plate");
    assert_eq!(
        image(koi),
        image(printed),
        "the plate covers the printing's art"
    );
    assert_eq!(
        image(plain),
        image(ordinary),
        "the default printing keeps its art"
    );
    assert_eq!(
        list.get(koi).unwrap().plate.as_ref().unwrap().rules,
        list.get(plain).unwrap().plate.as_ref().unwrap().rules,
        "both plates say the same ability"
    );

    // On the stack, each player's trigger takes the printing of its own source object.
    let mut builder = ViewBuilder::new();
    let mine = builder.card(ZoneName::Battlefield, Side::You, name).slot;
    let theirs = builder
        .card(ZoneName::Battlefield, Side::Opponent, name)
        .slot;
    let mut view = builder.build();
    for object in &mut view.objects {
        if object.slot == mine {
            object.printing = Some(ix);
        }
    }
    let trigger = |source, controller| StackItem::Ability {
        source: Some(source),
        ability: 1,
        granted: false,
        name: name.to_owned(),
        triggered: true,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller,
        manual: None,
        targets: Vec::new(),
        x: 0,
    };
    view.stack = vec![trigger(mine, Side::You), trigger(theirs, Side::Opponent)];
    assert_eq!(view.ability_printing(&view.stack[0]), Some(ix));
    assert_eq!(view.ability_printing(&view.stack[1]), None);
    let whole = list.read_whole(&view);
    assert!(
        whole.faces().contains(&koi),
        "my trigger is read as the koi"
    );
    assert!(
        whole.faces().contains(&plain),
        "theirs keeps the ordinary art"
    );
}
