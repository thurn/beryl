//! The game screen's corner and refused clicks (UI Conventions, "Chrome", "Undo" and "Why a
//! click is refused"), on `teferi-timing`, whose last prompt is a priority stop with answers to
//! take back and a hand card that Teferi, Time Raveler holds to sorcery speed. The client is
//! driven directly, so that each test reads the commands it queues: the recording driver
//! cannot replay an undo.

use std::str::FromStr;

use app_client::Input;
use app_client::command::SessionCommand;
use app_client::driver::{Duration, Moment, Page};
use app_client::{Client, Config};
use beryl_inspect::pages::{page, recording};
use beryl_protocol::DecisionId;
use beryl_protocol::protocol::{ObjectView, RefusalView, Side, Status};
use game_play::sound;
use glam::Vec2;
use kit_frame::frame::{Frame, Role, Wake};
use kit_probe::agent::Layout;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Size,
};

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};
const MILLISECOND: u64 = 1_000_000;

/// The client at a decision of a recording, with the table at rest, and the decision's id.
struct Table {
    client: Client,
    decision: DecisionId,
    now: u64,
}

impl Table {
    /// At the last view of `name`, or with `first`, at its first view that asks the viewer.
    fn new(name: &str, first: bool) -> Self {
        let recording = recording(name, Side::You).unwrap_or_else(|why| panic!("{why}"));
        let mut views = recording.views().unwrap_or_else(|why| panic!("{why}"));
        if first {
            let asks = (views.iter())
                .position(|update| matches!(update.view.status, Status::Deciding { .. }))
                .expect("a view that asks the viewer");
            views.truncate(asks + 1);
        }
        let Status::Deciding { decision } = &views.last().expect("a view").view.status else {
            panic!("{name}'s last view asks the viewer");
        };
        let decision = decision.id;
        let mut client = Client::new(Config::LARGEST);
        client.handle(Input::Surface {
            size: SIZE,
            scale: 1.0,
            safe_area: Insets::default(),
        });
        for view in views {
            client.handle(Input::Update(Box::new(view)));
        }
        let mut table = Self {
            client,
            decision,
            now: 0,
        };
        table.rest();
        // The stops the client sends the session as the game starts are not what is tested.
        table.commands();
        table
    }

    /// Draws frames until nothing waits for a time.
    fn rest(&mut self) {
        while {
            self.client.frame(self.now);
            self.client.wake() != Wake::Never
        } {
            self.now += 16 * MILLISECOND;
        }
    }

    fn frame(&mut self) -> &Frame {
        self.now += 16 * MILLISECOND;
        self.client.frame(self.now)
    }

    /// The centre of the element of `role` named `name` in the last frame.
    fn centre(&self, role: Role, name: &str) -> Option<Vec2> {
        let frame = self.client.last_frame();
        (frame.resolved.iter())
            .find(|element| element.role == role && frame.name(element.name) == name)
            .map(|element| (element.corners[0] + element.corners[2]) / 2.0)
    }

    /// Whether the last frame shows an element named `name`.
    fn shows(&self, name: &str) -> bool {
        let frame = self.client.last_frame();
        (frame.resolved.iter()).any(|element| frame.name(element.name) == name)
    }

    /// The mouse moved to `at`, pressed and released there, with frames after each, and the
    /// sounds those frames began.
    fn click(&mut self, at: Vec2) -> Vec<kit_frame::frame::Sound> {
        let mut sounds = Vec::new();
        for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
            self.client.handle(Input::Pointer(Pointer {
                id: 0,
                kind: PointerKind::Mouse,
                phase,
                position: at,
                button: if phase == PointerPhase::Move {
                    Button::None
                } else {
                    Button::Primary
                },
                wheel: Vec2::ZERO,
                modifiers: Modifiers::default(),
            }));
            sounds.extend(self.frame().sounds.iter().copied());
            // The click on a card is found in the frame after the release, and its sound
            // plays in the frame after that.
            sounds.extend(self.frame().sounds.iter().copied());
        }
        sounds
    }

    fn commands(&mut self) -> Vec<SessionCommand> {
        std::iter::from_fn(|| self.client.next_command()).collect()
    }
}

#[test]
fn undo_shows_under_the_bug_button_and_takes_back_the_open_decisions_answer() {
    let mut table = Table::new("teferi-timing", false);
    let gear = table.centre(Role::Button, "Settings").expect("the gear");
    let bug = table
        .centre(Role::Button, "Report bug")
        .expect("the bug button");
    let undo = table.centre(Role::Button, "Undo").expect("undo");
    // One column at the top right: the gear, the bug button under it, undo under that.
    assert!((gear.x - bug.x).abs() < 0.5 && (bug.x - undo.x).abs() < 0.5);
    assert!(gear.y < bug.y && bug.y < undo.y, "{gear} {bug} {undo}");
    assert!(gear.x > SIZE.width - 60.0, "{gear}");
    table.click(undo);
    assert_eq!(
        table.commands(),
        [SessionCommand::Undo(table.decision)],
        "the button"
    );
    for phase in [KeyPhase::Down, KeyPhase::Up] {
        table.client.handle(Input::Key(Key {
            code: KeyCode::Z,
            phase,
            modifiers: Modifiers {
                command: true,
                ..Modifiers::default()
            },
        }));
    }
    table.frame();
    assert_eq!(
        table.commands(),
        [SessionCommand::Undo(table.decision)],
        "Cmd+Z"
    );
}

#[test]
fn with_nothing_to_take_back_undo_is_left_out_with_no_gap_kept() {
    let mut table = Table::new("pay-default", true);
    table.frame();
    assert!(!table.shows("Undo"));
    let gear = table.centre(Role::Button, "Settings").expect("the gear");
    let bug = table
        .centre(Role::Button, "Report bug")
        .expect("the bug button");
    let bottom = {
        let frame = table.client.last_frame();
        let corner = (frame.resolved.iter())
            .find(|element| frame.name(element.name) == "corner")
            .expect("the corner");
        corner.corners[2].y
    };
    assert!(bottom - bug.y < 30.0, "the corner ends at the bug button");
    assert!(bug.y - gear.y < 60.0, "{gear} {bug}");
}

#[test]
fn a_refused_click_plays_the_refusal_sound_and_sends_nothing() {
    let mut table = Table::new("teferi-timing", false);
    let recording = recording("teferi-timing", Side::You).unwrap_or_else(|why| panic!("{why}"));
    let views = recording.views().unwrap_or_else(|why| panic!("{why}"));
    let last = &views.last().expect("a view").view;
    let Status::Deciding { decision } = &last.status else {
        panic!("the last view asks the viewer");
    };
    let refused = (decision.refused.iter())
        .find(|refused| matches!(refused.reason, RefusalView::Timing { by: Some(_) }))
        .expect("a card Teferi refuses");
    let card: &ObjectView = (last.objects.iter())
        .find(|object| object.slot == refused.slot)
        .expect("the refused card");
    let name = card.name.as_deref().expect("a named card");
    let at = table.centre(Role::Card, name).expect("the card in hand");
    let sounds = table.click(at);
    assert!(
        sounds.iter().any(|played| played.id == sound::id("no")),
        "{sounds:?}"
    );
    assert_eq!(table.commands(), [], "the click goes no further");
}

/// Whether `page`'s frame now shows an element named `name`.
fn page_shows(page: &mut Page, name: &str) -> bool {
    let frame = page.frame();
    (frame.resolved.iter()).any(|element| frame.name(element.name) == name)
}

#[test]
fn a_refused_click_shows_the_reason_under_the_inspector_and_a_word_under_the_permanent() {
    // Played through the recording driver, which gives the client the faces the inspector
    // waits for.
    let mut page = page("teferi-timing", Side::You, Layout::Desktop, SIZE)
        .unwrap_or_else(|why| panic!("{why}"));
    let moment = Moment::from_str("prompt:4").unwrap_or_else(|why| panic!("{why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.still();
    let reason = "Can't play: Sorcery speed: Teferi";
    assert!(!page_shows(&mut page, reason));
    page.press("card \"Thirst for Discovery\"");
    page.step(Duration::from_str("300ms").expect("a duration"));
    assert!(page_shows(&mut page, reason));
    assert!(page_shows(&mut page, "Sorcery speed only"));
    // The word under Teferi goes after 1.8 seconds, wherever the pointer is; the reason stays
    // under the inspector while the pointer stays on the card.
    page.step(Duration::from_str("1600ms").expect("a duration"));
    assert!(!page_shows(&mut page, "Sorcery speed only"));
    assert!(page_shows(&mut page, reason));
    page.hover("card \"Thundertrap Trainer\"");
    page.step(Duration::from_str("300ms").expect("a duration"));
    assert!(!page_shows(&mut page, reason), "the pointer left the card");
}
