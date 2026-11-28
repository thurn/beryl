//! The card playground of a tools build, played through the real session thread: the testing
//! screen's card field finds Counterspell and lists its examples, choosing one plays it live,
//! and `]` or the dock's dropdown at the top of the table starts the next one. A bug report
//! saved during an example's game names the card and the variant.

use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use app_client::command::{Reply, Request};
use app_client::{Client, Config, Input};
use app_screens::screens::testing::playground::Card;
use beryl_client::bugs::Bugs;
use beryl_client::session::{self, Commands, Live};
use beryl_host::{Class, Inbox};
use beryl_protocol::protocol::Status;
use beryl_protocol::{GameUpdate, SessionId};
use glam::Vec2;
use kit_frame::frame::{Frame, Role};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Rect,
    Size,
};

/// The longest a test waits for the session: making a card's examples plays each of them.
const PATIENCE: Duration = Duration::from_secs(120);

/// A client on the Mac fed by a real session thread, with what the session has said of the
/// playground so far.
struct Rig {
    client: Client,
    commands: Commands,
    inbox: Receiver<Input>,
    now: u64,
    examples: Option<Card>,
    started: Option<SessionId>,
    latest: Option<GameUpdate>,
    saved: Option<Result<(), String>>,
    /// Whether the session has sent the agents, which open the start screen.
    started_up: bool,
}

impl Rig {
    /// On the start screen, with a recording listed so that the testing screen is offered.
    fn new() -> Self {
        let (sender, inbox) = channel();
        let inbox_end = Inbox::new(sender, || true);
        let commands = session::spawn(
            Class::Utility,
            {
                let inbox = inbox_end.clone();
                move || Live::new(None, inbox)
            },
            inbox_end,
        );
        let mut client = Client::new(
            crate::test_data::data(),
            Config {
                elements: 4096,
                draws: 4096,
                name_bytes: 131_072,
            },
        );
        client.handle(Input::Device(DeviceInput::Platform { mac: true }));
        client.handle(Input::Device(DeviceInput::Surface {
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 2.0,
            safe_area: Insets::default(),
        }));
        client.handle(Input::Recordings(vec!["a recording".to_owned()]));
        let mut rig = Self {
            client,
            commands,
            inbox,
            now: 0,
            examples: None,
            started: None,
            latest: None,
            saved: None,
            started_up: false,
        };
        rig.until("the start screen", |rig| rig.started_up);
        rig
    }

    /// The frame the client draws next, a second after the last.
    fn frame(&mut self) -> Frame {
        self.now += 1_000_000_000;
        self.client.frame(self.now).clone()
    }

    /// Sends the client's commands to the session and gives the client what came back.
    fn pump(&mut self) {
        while let Some(command) = self.client.next_command() {
            self.commands.send(command);
        }
        while let Ok(input) = self.inbox.recv_timeout(Duration::from_millis(20)) {
            match &input {
                Input::Reply(Reply::PlaygroundExamples(Ok(card))) => {
                    self.examples = Some(card.clone());
                }
                Input::Reply(Reply::PlaygroundStarted(started)) => {
                    self.started = Some(started.clone().expect("the example starts"));
                }
                Input::Update(update) => self.latest = Some((**update).clone()),
                Input::BugSaved(saved) => self.saved = Some(saved.clone()),
                Input::Agents(_) => self.started_up = true,
                _ => {}
            }
            self.client.handle(input);
        }
    }

    /// Pumps until `done` holds.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            self.pump();
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "never reached {what}");
        }
    }

    /// Presses the element named `name` with `role`.
    fn press(&mut self, role: Role, name: &str) {
        let frame = self.frame();
        let element = (frame.resolved.iter())
            .find(|element| element.role == role && frame.name(element.name) == name)
            .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let at = element.corners.iter().copied().sum::<Vec2>() / 4.0;
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.client
                .handle(Input::Device(DeviceInput::Pointer(Pointer {
                    id: 0,
                    kind: PointerKind::Mouse,
                    phase,
                    position: at,
                    button: Button::Primary,
                    wheel: Vec2::ZERO,
                    modifiers: Modifiers::default(),
                })));
        }
    }

    fn key(&mut self, code: KeyCode) {
        for phase in [KeyPhase::Down, KeyPhase::Up] {
            self.client.handle(Input::Device(DeviceInput::Key(Key {
                code,
                phase,
                modifiers: Modifiers::default(),
            })));
        }
    }

    /// Finds Counterspell on the testing screen, lists its examples, and plays the first, up
    /// to its first live decision with the dock naming it.
    fn playing_counterspell(&mut self) -> Card {
        self.key(KeyCode::T);
        self.until("the testing screen", |rig| {
            has(&rig.frame(), Role::Field, "card")
        });
        self.press(Role::Field, "card");
        (self.client).handle(Input::Device(DeviceInput::Paste("counterspel".to_owned())));
        self.until("Counterspell found", |rig| {
            has(&rig.frame(), Role::Button, "Counterspell")
        });
        self.press(Role::Button, "Counterspell");
        self.until("its examples", |rig| rig.examples.is_some());
        let card = self.examples.clone().expect("the examples");
        assert!(card.examples.len() >= 2, "{card:?}");
        let first = card.examples[0].text();
        self.until("its examples listed", |rig| {
            has(&rig.frame(), Role::Button, &first)
        });
        self.press(Role::Button, &first);
        self.live(&card, 0);
        card
    }

    /// Waits for the first live decision of the card's example at `index`, in the session
    /// the session said it started, and for the dock to name the example.
    fn live(&mut self, card: &Card, index: usize) {
        let before = self.started;
        let caption = card.caption(index);
        self.until(&caption, |rig| {
            let deciding = rig.started.is_some_and(|started| {
                Some(started) != before
                    && rig.latest.as_ref().is_some_and(|update| {
                        update.session == started
                            && matches!(update.view.status, Status::Deciding { .. })
                    })
            });
            deciding && has(&rig.frame(), Role::Button, &caption)
        });
    }
}

/// Whether `frame` has an element named `name` with `role`.
fn has(frame: &Frame, role: Role, name: &str) -> bool {
    (frame.resolved.iter()).any(|element| element.role == role && frame.name(element.name) == name)
}

#[test]
fn the_bracket_key_plays_the_next_example() {
    let mut rig = Rig::new();
    let card = rig.playing_counterspell();
    rig.key(KeyCode::BracketRight);
    rig.live(&card, 1);
}

#[test]
fn the_dropdown_plays_the_example_chosen() {
    let mut rig = Rig::new();
    let card = rig.playing_counterspell();
    rig.press(Role::Button, &card.caption(0));
    let second = card.examples[1].text();
    rig.until("the dropdown open", |rig| {
        has(&rig.frame(), Role::List, "playground examples")
    });
    rig.press(Role::Button, &second);
    rig.live(&card, 1);
}

#[test]
fn a_report_saved_during_an_example_names_the_card_and_variant() {
    let folder = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let card = rig.playing_counterspell();
    let mut bugs = Bugs::new(folder.path().to_owned());
    let latest = rig.latest.clone().expect("a view of the example");
    bugs.seen(&Input::Update(Box::new(latest)));
    bugs.save(
        &Request::SaveBug {
            description: "The counter never resolved".to_owned(),
            ai: false,
            region: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            window: Size {
                width: 1600.0,
                height: 900.0,
            },
        },
        None,
        Some(&rig.commands),
    );
    rig.until("the report saved", |rig| rig.saved.is_some());
    assert_eq!(rig.saved, Some(Ok(())));
    let report = report_text(folder.path());
    let named = format!("playground: {}/{}", card.slug, card.examples[0].name);
    assert!(report.contains(&named), "{report}");
}

/// The `report.md` of the one report saved in `folder`.
fn report_text(folder: &Path) -> String {
    let report = std::fs::read_dir(folder)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("report.md"))
        .find(|path| path.exists())
        .expect("a saved report");
    std::fs::read_to_string(report).unwrap()
}
