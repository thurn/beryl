//! Face IDs and ability plates at the boundary between the table and the asset workers.

use beryl_card_records::{CardRecordsRequest, TokenMakers, card_records};
use beryl_host::match_faces;
use beryl_protocol::card_records::{CARD_BACK_ID, CardRecords};
use game_table::faces::{CARD_BACK, FaceList, PrintedText};

#[test]
fn each_triggered_and_activated_paragraph_of_a_card_has_a_plate_face() {
    let records = card_records(
        &CardRecordsRequest {
            names: [
                "Island",
                "Snapcaster Mage",
                "Teferi, Time Raveler",
                "Remote Isle",
            ]
            .map(str::to_owned)
            .to_vec(),
        },
        TokenMakers::NONE,
    );
    let list = FaceList::from_records(&records);
    let faces = match_faces(&list);
    let plates = |name: &str| -> Vec<(bool, String)> {
        let source = list.find(name).unwrap();
        faces
            .faces
            .iter()
            .filter_map(|f| f.plate.as_ref())
            .filter(|plate| plate.source == source && !plate.choice)
            .map(|plate| (plate.triggered, plate.rules.clone()))
            .collect()
    };
    // Island's "({T}: Add {U}.)" is reminder text, which says no ability of its own.
    assert_eq!(plates("Island"), []);
    let snapcaster = plates("Snapcaster Mage");
    assert_eq!(snapcaster.len(), 1, "{snapcaster:?}");
    assert!(snapcaster[0].0 && snapcaster[0].1.starts_with("When"));
    // Teferi's static ability has no plate; its two loyalty abilities do.
    let teferi = plates("Teferi, Time Raveler");
    assert_eq!(teferi.len(), 2, "{teferi:?}");
    assert!(teferi.iter().all(|(triggered, _)| !triggered));
    assert!(teferi[0].1.starts_with("+1:"));
    let choice = list.choice_plate("Teferi, Time Raveler", 0).unwrap();
    let choice = faces.get(choice).unwrap();
    let plate = choice.plate.as_ref().unwrap();
    assert_eq!(plate.rules, teferi[0].1);
    assert!(plate.choice);
    // Cycling says its cost and colon only in its reminder text. It is the land's second
    // activated ability, after "{T}: Add {U}.", as the session numbers the choice to cycle.
    let isle = plates("Remote Isle");
    assert_eq!(isle.len(), 2, "{isle:?}");
    assert!(isle[1].1.starts_with("Cycling"));
    let cycle = list.choice_plate("Remote Isle", 1).unwrap();
    assert_eq!(
        faces.get(cycle).unwrap().plate.as_ref().unwrap().rules,
        isle[1].1
    );
    // Each plate face is a picture of its own in the compressed cache.
    let mut keys: Vec<&str> = faces.faces.iter().map(|f| f.key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), faces.faces.len());
}

#[test]
fn the_host_keeps_face_indices_and_cache_keys_when_preparing_a_gallery() {
    let rules = "When this enters, draw a card.";
    let mut list = FaceList::from_records(&CardRecords::none());
    list.add_preview(
        "Preview",
        "U",
        PrintedText {
            rules: Some(rules.to_owned()),
            ..PrintedText::default()
        },
    );
    list.add_plates();
    let faces = match_faces(&list);

    let back = faces.get(CARD_BACK).unwrap();
    assert_eq!(back.scan, format!("/backs/large/0/a/{CARD_BACK_ID}.jpg"));
    assert_eq!(back.key, format!("back-{CARD_BACK_ID}"));
    let source = list.find("Preview").unwrap();
    let preview = faces.get(source).unwrap();
    assert_eq!(preview.scan, "gallery/Preview");
    assert_eq!(preview.key, "Preview-front");
    let plate = faces.get(list.plate("Preview", true, 0).unwrap()).unwrap();
    assert_eq!(plate.name, "Preview");
    assert_eq!(plate.scan, preview.scan);
    assert_eq!(plate.key, "Preview-front-plate-t-a75b0d1b");
    assert_eq!(plate.plate.as_ref().unwrap().source, source);
}
