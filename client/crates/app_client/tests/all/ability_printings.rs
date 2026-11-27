//! An ability on the stack is drawn over the picture of the printing its source object shows,
//! played from the views that bug reports saved: a Dandân shown as its Secret Lair printing puts
//! its trigger on the stack over the same art, and a Secret Lair Lonely Sandbar's cycling ability
//! wears its activated-ability plate over the Secret Lair art.

use app_client::driver::{Duration, Page, Recording};
use beryl_protocol::protocol::Side;
use game_probe::story::{Body, Story};
use game_table::faces::FaceList;
use game_table::table::labels::ABILITY_ROLE;
use kit_frame::frame::Frame;
use kit_ui::input::FaceId;

/// The image of the picture `face` shows: its plate's source for an ability's face.
fn picture(faces: &FaceList, face: FaceId) -> &str {
    let listed = faces.get(face).expect("the frame draws a listed face");
    let source = listed.plate.as_ref().map_or(face, |plate| plate.source);
    &faces
        .get(source)
        .expect("a plate covers a listed face")
        .image
}

/// For each ability card `frame` draws: its source's name, the image it shows, and whether it
/// wears a plate. The driver gives each face the layer numbered as its `FaceId`.
fn ability_cards<'f>(frame: &'f Frame, faces: &'f FaceList) -> Vec<(String, String, bool)> {
    (frame.resolved.iter())
        .filter(|element| element.role == ABILITY_ROLE && element.opacity > 0.0)
        .filter_map(|element| {
            let face = FaceId(element.face?);
            let plated = faces.get(face)?.plate.is_some();
            let name = frame.name(element.name).to_owned();
            Some((name, picture(faces, face).to_owned(), plated))
        })
        .collect()
}

/// The images of the cards named `name` that `frame` draws face up.
fn card_pictures(frame: &Frame, faces: &FaceList, name: &str) -> Vec<String> {
    (frame.resolved.iter())
        .filter(|element| {
            element.role == kit_frame::frame::Role::Card && frame.name(element.name) == name
        })
        .filter_map(|element| Some(picture(faces, FaceId(element.face?)).to_owned()))
        .collect()
}

/// Plays the capture story `source` to its end, and checks every frame that draws an ability
/// card: the ability wears a plate, and shows the picture of a card of its source's name that
/// the table draws, which is not the card's ordinary art.
fn abilities_wear_their_source_printing(source: &str, story: &str) {
    let Body::Capture { capture, .. } = Story::parse(source).expect("the story parses").body else {
        panic!("{story} is a capture story");
    };
    let size = kit_ui::input::Size {
        width: 1728.0,
        height: 1117.0,
    };
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(capture.messages).into(),
        Side::You,
        size,
    )
    .expect("the views can be read")
    .named(story);
    let mut seen = 0;
    for _ in 0..200 {
        page.step(Duration::from_millis(25));
        let faces = page.faces().clone();
        let frame = page.frame();
        for (name, shown, plated) in ability_cards(frame, &faces) {
            assert!(plated, "{story}: {name}'s ability wears no plate");
            let ordinary = faces
                .find(&name)
                .map(|face| picture(&faces, face).to_owned());
            assert_ne!(
                Some(&shown),
                ordinary.as_ref(),
                "{story}: {name}'s ability shows the card's ordinary art"
            );
            let sources = card_pictures(frame, &faces, &name);
            assert!(
                sources.contains(&shown),
                "{story}: {name}'s ability shows {shown}, and its source shows {sources:?}"
            );
            seen += 1;
        }
    }
    assert!(seen > 0, "{story} draws an ability card");
}

#[test]
fn a_trigger_wears_the_secret_lair_printing_its_source_shows() {
    abilities_wear_their_source_printing(
        include_str!("../../../game_table/stories/dandan-trigger-printing.story"),
        "dandan-trigger-printing",
    );
}

#[test]
fn a_cycling_ability_wears_its_plate_over_the_printing_its_source_shows() {
    abilities_wear_their_source_printing(
        include_str!("../../../game_table/stories/cycling-ability-printing.story"),
        "cycling-ability-printing",
    );
}
