//! Face uploads through the asset service, with a real renderer and an empty scan
//! cache, so that every face is drawn as Beryl's frame and none is downloaded. Each test waits
//! on the wake the workers call after sending a face, never on the clock.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use app_client::Input;
use app_client::{Client, Config};
use app_screens::screens::Screen;
use beryl_art::Origins;
use beryl_host::{Assets, Scans, When};
use beryl_protocol::card_records::CardRecords;
use beryl_render::{Limits, Renderer};
use game_table::faces::CARD_BACK;
use kit_ui::element::Action;
use kit_ui::input::{FaceId, FaceSize};

/// An address where nothing listens, so that a scan the empty cache lacks fails at once.
const NOWHERE: Origins<'static> = Origins {
    cards: "http://127.0.0.1:9",
    backs: "http://127.0.0.1:9",
};

struct Host {
    client: Client,
    renderer: Renderer,
    assets: Assets,
    /// One message each time a worker sent a face, or the service woke the host after a frame.
    wakes: Receiver<()>,
    dir: PathBuf,
    /// The client's clock, a frame further on each `frame`.
    now: u64,
}

impl Host {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("beryl-host-faces-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (woke, wakes) = mpsc::channel();
        let renderer = Renderer::headless(Limits::Default, crate::test_data::data().lighting)
            .expect("a GPU adapter and device");
        let assets = Assets::start(&Scans::in_dir(&dir), NOWHERE, move || {
            let _ = woke.send(());
        });
        Self {
            client: Client::new(crate::test_data::data(), Config::LARGEST),
            renderer,
            assets,
            wakes,
            dir,
            now: 0,
        }
    }

    fn deliver(&mut self, when: When) {
        self.assets
            .deliver(&mut self.client, &mut self.renderer, when);
    }

    /// One turn of a host's frame loop: delivers, makes a frame and delivers again.
    fn frame(&mut self) {
        self.deliver(When::BeforeFrame);
        self.now += 16_666_667;
        self.client.prepare(self.now);
        self.client.frame(self.now);
        self.deliver(When::AfterFrame);
    }

    /// Makes frames until `done`, waiting between them for a worker or for the nudge the service
    /// gives while faces wait, as a host's event loop does.
    fn until(&mut self, done: impl Fn(&Client) -> bool) {
        loop {
            self.frame();
            if done(&self.client) {
                return;
            }
            self.wait_for_delivery();
            // A host's wake reaches the client as an input, which leaves its wake `Now` until
            // the next frame: the uploads must not take that for an animating table.
            self.client.handle(Input::Nudge);
        }
    }

    fn wait_for_delivery(&self) {
        self.wakes
            .recv_timeout(Duration::from_mins(2))
            .expect("the workers wake the host");
    }

    fn board_layer(&self, face: FaceId) -> Option<u16> {
        self.client.face_layers().board_layer_of(face)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn no_records() -> Input {
    Input::Cards(Box::new(CardRecords::none()))
}

/// A match's records given again, as when a game of the same decklists is resumed, leave its
/// faces with no layers in the client: the host must prepare and upload them again although
/// the list is equal to the last.
#[test]
fn every_face_list_given_is_prepared_even_one_equal_to_the_last() {
    let mut host = Host::new("again");
    host.client.handle(no_records());
    host.until(|client| client.face_layers().board_layer_of(CARD_BACK).is_some());
    host.client.handle(no_records());
    assert_eq!(host.board_layer(CARD_BACK), None);
    host.until(|client| client.face_layers().board_layer_of(CARD_BACK).is_some());
    assert_eq!(host.board_layer(CARD_BACK), Some(CARD_BACK.0));
}

fn open_table_story(client: &mut Client) {
    client.handle(Input::Stories(app_client::stories::embedded()));
    client.dispatch(&Action::new(app_screens::screens::start::Message::Open(
        Screen::Gallery,
    )));
    client.dispatch(&Action::new(app_screens::screens::gallery::Message::Open(
        "story/game_table/zones-and-card-states".to_owned(),
    )));
}

#[test]
fn story_face_uploads_do_not_change_the_matchs_layers() {
    let mut host = Host::new("gallery");
    host.client.handle(no_records());
    open_table_story(&mut host.client);
    assert!(host.client.face_generation().0);
    host.until(|client| client.face_layers().board_layer_of(FaceId(0)).is_some());
    host.client
        .dispatch(&Action::new(app_screens::screens::start::Message::Open(
            Screen::Start,
        )));
    assert!(!host.client.face_generation().0);
    assert_eq!(host.board_layer(CARD_BACK), None);
}

/// Finished faces are held by the renderer and uploaded one board-size face a frame, only in
/// the call before the frame, so that two calls a frame never upload two. None, and no
/// placeholder, is built on the thread that delivers them.
#[test]
fn held_faces_are_uploaded_one_a_frame_before_it() {
    let mut host = Host::new("held");
    // The renderer draws its startup placeholder back on this thread; a match's are the
    // workers'.
    let placeholders = beryl_assets::placeholders_drawn_on_this_thread();
    let records = beryl_card_records::card_records_among(
        ["Island", "Plains", "Forest"],
        beryl_card_records::TokenMakers::NONE,
    );
    host.client.handle(Input::Cards(Box::new(records)));
    host.client.frame(0);
    host.deliver(When::AfterFrame);
    let count = host.client.faces().faces().len();
    assert!(count > 2, "the match lists {count} faces");
    assert_eq!(host.renderer.waiting(), 0);
    while host.renderer.waiting() < count {
        host.wait_for_delivery();
        host.deliver(When::AfterFrame);
    }
    assert_eq!(host.renderer.waiting(), count);
    let landed = |host: &Host| {
        (0..count)
            .filter(|&at| host.board_layer(FaceId(at as u16)).is_some())
            .count()
    };
    assert_eq!(landed(&host), 0, "a call after the frame uploads nothing");
    host.deliver(When::BeforeFrame);
    assert_eq!(landed(&host), 1);
    assert_eq!(host.renderer.waiting(), count - 1);
    host.deliver(When::AfterFrame);
    assert_eq!(landed(&host), 1);
    // The thread that calls `deliver`, a host's frame thread, only uploads: the workers read,
    // draw and compress every face.
    assert_eq!(beryl_assets::faces_built_on_this_thread(), 0);
    assert_eq!(beryl_assets::frames_drawn_on_this_thread(), 0);
    assert_eq!(
        beryl_assets::placeholders_drawn_on_this_thread(),
        placeholders
    );
}

/// Lonely Sandbar's cycling ability on the stack, at a seated game's 30th turn, from the
/// report "All of the card textures blinked out of existence for a second and were replaced
/// with the diagonal lines placeholder art": the view before it and the view with it, whose
/// text "Cycling {U}" no card prints, so the client adds a plate face for it to its list.
const CYCLING_PLATE: &str = include_str!("../fixtures/cycling-plate.json");

/// A face added to the list during play (here a plate for an ability's text) keeps every face
/// already uploaded in its layer. The client once numbered the grown list as a new one, the
/// host made its arrays again from placeholders, and every card on the table showed its
/// placeholder until all its faces were prepared again, which took over a second on a loaded
/// machine.
#[test]
fn a_face_added_during_play_keeps_the_faces_uploaded() {
    let mut host = Host::new("added");
    let messages: Vec<beryl_protocol::ipc::FixtureMessage> = serde_json::from_value(
        serde_json::from_str::<serde_json::Value>(CYCLING_PLATE).unwrap()["messages"].take(),
    )
    .expect("the report's messages");
    let records =
        beryl_card_records::fixture_message(&messages, beryl_card_records::TokenMakers::NONE);
    let beryl_protocol::ipc::FixtureMessage::Response { value, .. } = records else {
        panic!("card records are a response");
    };
    let records: CardRecords = serde_json::from_value(value).expect("card records");
    let views = app_client::driver::Recording(messages)
        .views()
        .expect("the report's views");
    let [before, with_plate] = <[_; 2]>::try_from(views).expect("two views");
    host.client.handle(Input::Cards(Box::new(records)));
    host.client.handle(Input::Update(Box::new(before)));
    let uploaded = |host: &Host, count: usize| {
        (0..count).all(|at| {
            let face = FaceId(at as u16);
            host.board_layer(face).is_some()
                && host.renderer.layer_uploaded(FaceSize::Board, face.0)
        })
    };
    let count = host.client.faces().faces().len();
    loop {
        host.frame();
        if uploaded(&host, count) {
            break;
        }
        host.wait_for_delivery();
        host.client.handle(Input::Nudge);
    }
    let generation = host.client.face_generation();
    host.client.handle(Input::Update(Box::new(with_plate)));
    let grown = host.client.faces().faces().len();
    assert!(grown > count, "the plate adds a face to the {count} listed");
    assert_eq!(host.client.face_generation(), generation);
    loop {
        host.frame();
        assert!(
            uploaded(&host, count),
            "a face uploaded before the plate was added lost its layer"
        );
        if uploaded(&host, grown) {
            break;
        }
        host.wait_for_delivery();
        host.client.handle(Input::Nudge);
    }
    assert_eq!(host.client.last_frame().faceless_cards(), 0);
}
