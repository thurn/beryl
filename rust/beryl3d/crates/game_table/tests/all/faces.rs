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
