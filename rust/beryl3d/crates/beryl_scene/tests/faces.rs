//! Card faces: the client lists the match's faces from the card records it is given, and draws
//! each card from its face's layer once the face has arrived (docs/beryl-3d.md, "Card faces").

use beryl_scene::agent::checks::blank_cards;
use beryl_scene::agent::{Context, Layout, Rule};
use beryl_scene::faces::{CARD_BACK, FaceList};
use beryl_scene::frame::{Frame, Material, Role};
use beryl_scene::input::{AssetReady, FaceId, FaceSize, Input, Insets, Size};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use beryl_scene::{Client, Config};
use beryl_session::card_records::{CardRecords, card_records_among};

const SIZE: Size = Size {
    width: 1440.0,
    height: 900.0,
};

/// The sample view, with both seats' cards, and the card records of every name it shows.
fn sample() -> (ViewBuilder, CardRecords) {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let view = builder.build();
    let records = card_records_among(view.objects.iter().filter_map(|o| o.name.as_deref()));
    (builder, records)
}

/// A client given the sample's card records and view, and then `faces`.
fn client(faces: impl FnOnce(&FaceList) -> Vec<AssetReady>) -> Client {
    let (builder, records) = sample();
    let mut client = Client::new(Config::LARGEST);
    client.handle(Input::Surface {
        size: SIZE,
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Cards(Box::new(records)));
    for ready in faces(client.faces()) {
        client.handle(Input::Assets(ready));
    }
    client.handle(Input::Update(Box::new(builder.update())));
    client
}

fn board(face: FaceId, layer: u16) -> AssetReady {
    AssetReady::Face {
        face,
        size: FaceSize::Board,
        layer,
    }
}

/// The face layer of each card named `name`, and how many card draw records sample `layer`.
fn layers(frame: &Frame, name: &str, layer: u16) -> (Vec<Option<u16>>, usize) {
    let faces: Vec<_> = frame
        .resolved
        .iter()
        .filter(|r| r.role == Role::Card && frame.name(r.name) == name)
        .map(|r| r.face)
        .collect();
    assert!(!faces.is_empty(), "the frame draws no {name}");
    let draws = frame
        .draws
        .iter()
        .filter(|d| d.material == Material::Card as u8 && d.layer == layer)
        .count();
    (faces, draws)
}

fn blank(frame: &Frame) -> Vec<String> {
    let context = Context {
        recording: "sample".to_owned(),
        seat: beryl_protocol::protocol::Side::You,
        layout: Layout::Desktop,
        size: SIZE,
        safe_area: Insets::default(),
        at: 0,
    };
    blank_cards(frame, &context)
        .iter()
        .inspect(|fault| assert_eq!(fault.rule, Rule::BlankCard))
        .map(|fault| fault.elements[0].locator.clone())
        .collect()
}

#[test]
fn the_card_back_is_the_first_face_and_a_picture_is_listed_once() {
    let (_, records) = sample();
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
fn a_face_that_has_not_arrived_leaves_its_cards_on_layer_zero_and_blank() {
    let mut client = client(|_| Vec::new());
    let frame = client.frame(0);
    let (faces, _) = layers(frame, "Island", 0);
    assert!(faces.iter().all(Option::is_none));
    let cards = frame
        .resolved
        .iter()
        .filter(|r| r.role == Role::Card)
        .count();
    assert_eq!(
        layers(frame, "Island", 0).1,
        cards,
        "every card is drawn from layer 0"
    );
    assert!(
        blank(frame)
            .iter()
            .any(|locator| locator.contains("Island"))
    );
}

#[test]
fn a_face_arriving_gives_its_cards_that_layer() {
    let mut client = client(|list| vec![board(list.find("Island").unwrap(), 7)]);
    let frame = client.frame(0);
    let (faces, draws) = layers(frame, "Island", 7);
    assert!(faces.iter().all(|&face| face == Some(7)), "{faces:?}");
    assert_eq!(draws, faces.len(), "each Island is drawn from layer 7");
    let blank = blank(frame);
    assert!(!blank.iter().any(|locator| locator.contains("Island")));
    assert!(
        !blank.is_empty(),
        "the faces that have not arrived are still blank"
    );
}

#[test]
fn no_card_is_blank_once_every_face_has_arrived() {
    let mut client = client(|list| list.all_ready().collect());
    assert_eq!(blank(client.frame(0)), Vec::<String>::new());
}
