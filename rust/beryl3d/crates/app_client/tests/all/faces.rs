//! Card faces: the client lists the match's faces from the card records it is given, and draws
//! each card from its face's layer once the face has arrived (docs/beryl-3d.md, "Card faces").

use app_client::Input;
use app_client::{Client, Config};
use beryl_card_records::{CardRecords, TokenMakers, card_records_among};
use game_probe::testing::{ScriptBuilder, ViewBuilder};
use game_table::faces::{CARD_BACK, FaceList};
use kit_frame::frame::{Frame, Material, Role, TextureArray};
use kit_probe::agent::checks::blank_cards;
use kit_probe::agent::{Context, Layout, Rule};
use kit_ui::input::{AssetReady, FaceId, FaceSize, Insets, Lifecycle, Size, Tier};

const SIZE: Size = Size {
    width: 1440.0,
    height: 900.0,
};

/// The sample view, with both seats' cards, and the card records of every name it shows.
fn sample() -> (ViewBuilder, CardRecords) {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let view = builder.build();
    let names = view.objects.iter().filter_map(|o| o.name.as_deref());
    let records = card_records_among(names, TokenMakers::NONE);
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

/// The names of the cards `frame` draws face up, each once, in the order first drawn.
fn drawn_names(frame: &Frame) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for resolved in frame.resolved.iter().filter(|r| r.role == Role::Card) {
        let name = frame.name(resolved.name);
        if !name.is_empty() && !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }
    names
}

#[test]
fn low_memory_forgets_full_size_faces_and_keeps_the_tier_at_baseline() {
    let mut client = client(|_| Vec::new());
    let names = drawn_names(client.frame(0));
    assert!(names.len() >= 3, "the sample draws three cards: {names:?}");
    let faces: Vec<FaceId> = names[..3]
        .iter()
        .map(|name| client.faces().find(name).expect("a drawn card has a face"))
        .collect();
    for (layer, &face) in (10..).zip(&faces) {
        client.handle(Input::Assets(board(face, layer)));
    }
    for (layer, &face) in (0..).zip(&faces[..2]) {
        client.handle(Input::Assets(AssetReady::Face {
            face,
            size: FaceSize::Full,
            layer,
        }));
    }
    assert_eq!(client.face_layers().full_layer_of(faces[1]), Some(1));
    client.handle(Input::Tier(Tier::Enhanced));
    assert_eq!(client.tier(), Tier::Enhanced);

    client.handle(Input::Lifecycle(Lifecycle::LowMemory));
    let layers_of = |client: &Client, face| {
        let layers = client.face_layers();
        (layers.board_layer_of(face), layers.full_layer_of(face))
    };
    for (layer, &face) in (10..).zip(&faces) {
        assert_eq!(layers_of(&client, face), (Some(layer), None));
    }
    assert_eq!(client.tier(), Tier::Baseline);
    let frame = client.frame(1);
    for (layer, name) in (10..).zip(&names[..2]) {
        let (drawn, records) = layers(frame, name, layer);
        assert!(
            drawn.iter().all(|&face| face == Some(layer)),
            "{name}: {drawn:?}"
        );
        assert!(
            records >= drawn.len(),
            "each {name} is drawn from layer {layer}"
        );
    }

    client.handle(Input::Tier(Tier::Enhanced));
    assert_eq!(
        client.tier(),
        Tier::Baseline,
        "the tier stays Baseline after low memory"
    );
}

#[test]
fn an_ability_on_the_stack_has_a_plate_face_of_its_own_beside_its_source_s() {
    let records = card_records_among(
        ["Snapcaster Mage", "Teferi, Time Raveler"],
        TokenMakers::NONE,
    );
    let list = FaceList::from_records(&records);
    let snapcaster = list.find("Snapcaster Mage").unwrap();
    let plate = list
        .plate("Snapcaster Mage", true, 0)
        .expect("its trigger has a plate");
    assert_ne!(plate, snapcaster);
    let listed = list.get(plate).unwrap();
    assert_eq!(listed.image, list.get(snapcaster).unwrap().image);
    assert!(listed.plate.as_ref().unwrap().rules.starts_with("When"));
    // A plate is found by its kind and its place among that kind's paragraphs.
    assert!(list.plate("Snapcaster Mage", false, 0).is_none());
    assert!(list.plate("Teferi, Time Raveler", false, 1).is_some());
    assert!(list.plate("Teferi, Time Raveler", false, 2).is_none());
}

/// Whether each card draw record of `frame` samples the full-size array, and its layer.
fn card_draws(frame: &Frame) -> Vec<(bool, u16)> {
    (frame.draws.iter())
        .filter(|d| d.material == Material::Card as u8)
        .map(|d| (d.flags & 0b11 == TextureArray::FullFaces as u8, d.layer))
        .collect()
}

#[test]
fn hand_and_stack_cards_draw_their_full_frame_and_permanents_their_battlefield_frame() {
    // The web interface draws the full frame, with the cost, type line and rules text, in the
    // hand and on the stack, and the battlefield frame on the battlefield (`Card.tsx`). A face's
    // board-size picture is the battlefield frame, so the hand and the stack ask for full size.
    let mut client = client(|list| list.all_ready().collect());
    client.frame(0);
    let wanted: Vec<FaceId> = client.wanted_faces().collect();
    let find = |client: &Client, name| client.faces().find(name).unwrap();
    assert_eq!(wanted.first(), Some(&CARD_BACK), "{wanted:?}");
    for name in ["Lightning Bolt", "Counterspell", "Memory Lapse", "Plains"] {
        assert!(wanted.contains(&find(&client, name)), "{name}: {wanted:?}");
    }
    for name in ["Serra Angel", "Pacifism", "Brainstorm"] {
        assert!(!wanted.contains(&find(&client, name)), "{name}: {wanted:?}");
    }
    assert_eq!(
        wanted[1],
        find(&client, "Lightning Bolt"),
        "the stack is asked for before the hand"
    );
    let full: Vec<(FaceId, u16)> = wanted.iter().copied().zip(100..).collect();
    for &(face, layer) in &full {
        client.handle(Input::Assets(AssetReady::Face {
            face,
            size: FaceSize::Full,
            layer,
        }));
    }
    let draws = card_draws(client.frame(1));
    let full_layer = |name| {
        full.iter()
            .find(|(f, _)| *f == find(&client, name))
            .unwrap()
            .1
    };
    for name in ["Lightning Bolt", "Counterspell"] {
        assert!(
            draws.contains(&(true, full_layer(name))),
            "{name} is drawn from its full-size layer: {draws:?}"
        );
    }
    let angel = find(&client, "Serra Angel");
    assert!(
        draws.contains(&(false, angel.0)),
        "Serra Angel is drawn from its board-size layer: {draws:?}"
    );
}
