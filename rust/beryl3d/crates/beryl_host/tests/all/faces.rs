//! Face uploads through the asset service, with a real renderer and an empty scan
//! cache, so that every face is drawn as Beryl's frame and none is downloaded. Each test waits
//! on the wake the workers call after sending a face, never on the clock.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use app_client::Input;
use app_client::Press;
use app_client::{Client, Config};
use app_screens::screens::Screen;
use beryl_art::Origins;
use beryl_host::{Assets, Scans, When};
use beryl_protocol::card_records::CardRecords;
use beryl_render::{Limits, Renderer};
use game_table::faces::CARD_BACK;
use kit_ui::element::Action;
use kit_ui::input::FaceId;

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
        let mut renderer = Renderer::headless(Limits::Default).expect("a GPU adapter and device");
        let assets = Assets::start(&mut renderer, &Scans::in_dir(&dir), NOWHERE, move || {
            let _ = woke.send(());
        });
        Self {
            client: Client::new(Config::LARGEST),
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

/// The gallery's faces arrive as its own (`AssetReady::GalleryFace`), so that a match whose
/// faces have not landed still has none when the gallery closes.
#[test]
fn the_gallerys_faces_never_land_in_the_matchs() {
    let mut host = Host::new("gallery");
    host.client.handle(no_records());
    host.client
        .press(&Action::new(Press::Open(Screen::Gallery)));
    assert!(host.client.showing_gallery());
    host.until(|client| client.face_layers().board_layer_of(FaceId(0)).is_some());
    host.client.press(&Action::new(Press::Open(Screen::Start)));
    assert!(!host.client.showing_gallery());
    assert_eq!(host.board_layer(CARD_BACK), None);
}

/// Finished faces are held by the renderer and uploaded one board-size face a frame, only in
/// the call before the frame, so that two calls a frame never upload two. None is built on the
/// thread that delivers them.
#[test]
fn held_faces_are_uploaded_one_a_frame_before_it() {
    let mut host = Host::new("held");
    host.client
        .press(&Action::new(Press::Open(Screen::Gallery)));
    host.deliver(When::AfterFrame);
    let count = host.client.faces().faces().len();
    assert!(count > 2, "the gallery lists {count} faces");
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
}
