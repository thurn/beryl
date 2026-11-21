//! A hand card picked up, carried and let go (docs/beryl-3d.md, "Motion the pointer drives"),
//! on `shape-zone`, whose first prompt is a priority stop at which the viewer may cast Trinket
//! Mage from their hand. The client is driven directly, so that each test sees the commands it
//! queues and the frames between pointer events.

use app_client::Input;
use app_client::command::SessionCommand;
use app_client::driver::{Recording, Responses, plays_on};
use app_client::{Client, Config};
use beryl_inspect::pages::recording;
use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::{Click, ClickRequest};
use beryl_protocol::protocol::{ObjectId, Side, Status};
use beryl_protocol::script::Subject;
use game_probe::agent::steps::no_jumps;
use glam::Vec2;
use kit_frame::frame::{Frame, Role, StateBits, Wake};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::Size;
use kit_ui::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase};

const CARD: &str = "Trinket Mage";
/// Trinket Mage's slot in `shape-zone`.
const SLOT: u16 = 10;
const MILLISECOND: u64 = 1_000_000;

/// The client at `shape-zone`'s first prompt, with the table at rest.
struct Table {
    client: Client,
    recording: Recording,
    views: Vec<GameUpdate>,
    now: u64,
}

impl Table {
    fn new() -> Self {
        let recording = recording("shape-zone", Side::You).unwrap_or_else(|why| panic!("{why}"));
        let views = recording.views().unwrap_or_else(|why| panic!("{why}"));
        let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
        client.record();
        client.handle(Input::Device(DeviceInput::Surface {
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        }));
        client.handle(Input::Update(Box::new(views[0].clone())));
        let mut table = Self {
            client,
            recording,
            views,
            now: 0,
        };
        while {
            table.frame();
            table.client.wake() != Wake::Never
        } {
            table.now += 16 * MILLISECOND;
        }
        table
    }

    fn frame(&mut self) -> &Frame {
        self.client.frame(self.now)
    }

    fn pointer(&mut self, phase: PointerPhase, position: Vec2) {
        self.client
            .handle(Input::Device(DeviceInput::Pointer(Pointer {
                id: 0,
                kind: PointerKind::Mouse,
                phase,
                position,
                button: Button::Primary,
                wheel: Vec2::ZERO,
                modifiers: Modifiers::default(),
            })));
    }

    /// The named card's element in the frame at the present time: its state, and the centre
    /// of its face on screen, where the diagonals of its corners cross.
    fn card(&mut self, name: &str) -> (StateBits, Vec2) {
        let frame = self.frame();
        let element = frame
            .resolved
            .iter()
            .find(|element| element.role == Role::Card && frame.name(element.name) == name)
            .unwrap_or_else(|| panic!("no card {name}"));
        (element.state, crossing(element.corners))
    }

    /// Presses on Trinket Mage, and moves the pointer `by` from there over `seconds`, a move
    /// and a frame `rate` times a second, checking at each frame that the card is held under
    /// the pointer. Returns where the pointer ends.
    fn carry(&mut self, by: Vec2, seconds: f32, rate: u32) -> Vec2 {
        let (_, start) = self.card(CARD);
        self.pointer(PointerPhase::Down, start);
        let frames = (seconds * rate as f32).round() as u32;
        let tick = 1_000_000_000 / u64::from(rate);
        let mut at = start;
        for frame in 1..=frames {
            at = start + by * (frame as f32 / frames as f32);
            self.now += tick;
            self.pointer(PointerPhase::Move, at);
            let (state, centre) = self.card(CARD);
            if start.distance(at) >= 6.0 {
                assert!(state.has(StateBits::HELD), "held at frame {frame}");
                assert!(
                    centre.distance(at) < 0.01,
                    "at {centre} under the pointer at {at}"
                );
            }
        }
        at
    }

    /// Holds the pointer still for `seconds`, a frame `rate` times a second.
    fn wait(&mut self, seconds: f32, rate: u32) {
        let tick = 1_000_000_000 / u64::from(rate);
        for _ in 0..(seconds * rate as f32).round() as u32 {
            self.now += tick;
            self.frame();
        }
    }

    fn lean_degrees(&self) -> Vec2 {
        let lean = self.client.pointer().lean();
        Vec2::new(lean.x.to_degrees(), lean.y.to_degrees())
    }

    fn commands(&mut self) -> Vec<SessionCommand> {
        std::iter::from_fn(|| self.client.next_command()).collect()
    }
}

/// Where the diagonals of the quadrilateral `corners` cross: the centre of a rectangle seen in
/// perspective.
fn crossing([a, b, c, d]: [Vec2; 4]) -> Vec2 {
    let (r, s) = (c - a, d - b);
    let t = (b - a).perp_dot(s) / r.perp_dot(s);
    a + r * t
}

/// Up the screen and to the right, 300 points long.
const DRAG: Vec2 = Vec2::new(180.0, -240.0);

#[test]
fn a_drag_leans_the_same_at_60_and_120_frames_a_second() {
    let mut leans = Vec::new();
    for rate in [60, 120] {
        let mut table = Table::new();
        table.carry(DRAG, 0.25, rate);
        let moving = table.lean_degrees();
        table.wait(0.5, rate);
        leans.push((moving, table.lean_degrees()));
    }
    let [(moving_60, still_60), (moving_120, still_120)] = leans[..] else {
        unreachable!()
    };
    assert!(
        moving_60.length() > 1.0,
        "it leans while carried: {moving_60}"
    );
    assert!(
        moving_60.distance(moving_120) < 0.5,
        "{moving_60} at 60 and {moving_120} at 120 frames a second"
    );
    assert!(
        still_60.distance(still_120) < 0.05,
        "after holding still, {still_60} and {still_120}"
    );
}

#[test]
fn a_long_gap_between_frames_moves_the_lean_no_more_than_the_longest_gap() {
    let mut changes = Vec::new();
    for gap in [50, 2000] {
        let mut table = Table::new();
        table.carry(DRAG, 0.25, 60);
        let before = table.lean_degrees();
        table.now += gap * MILLISECOND;
        table.frame();
        changes.push(table.lean_degrees().distance(before));
    }
    assert!(changes[0] > 0.0, "the lean moved: {changes:?}");
    assert!(changes[1] <= changes[0] + 1e-6, "{changes:?}");
}

#[test]
fn dragging_a_castable_card_onto_the_battlefield_casts_it_from_where_it_was_let_go() {
    let mut table = Table::new();
    let Status::Deciding { decision } = &table.views[0].view.status else {
        panic!("the first view asks the viewer");
    };
    let (decision, revision) = (decision.id, decision.prompt.revision);
    let at = table.carry(DRAG, 0.25, 60);
    let (_, held) = table.card(CARD);
    table.pointer(PointerPhase::Up, at);
    table.frame();
    let commands = table.commands();
    let on = Subject::Object {
        slot: ObjectId(SLOT),
    };
    assert_eq!(
        commands,
        [SessionCommand::Click(ClickRequest {
            decision,
            revision,
            click: Click::On { on },
        })]
    );
    let answer = Responses::new(&table.recording)
        .answer(&commands[0])
        .unwrap_or_else(|why| panic!("{why}"));
    assert!(answer.plays_on, "casting it answers the prompt");
    table.client.handle(Input::Reply(answer.reply));
    for update in &table.views[1..] {
        table.client.handle(Input::Update(Box::new(update.clone())));
        if !plays_on(update) {
            break;
        }
    }
    let (state, centre) = table.card(CARD);
    assert!(state.has(StateBits::MOVING), "the cast moves it");
    assert!(
        centre.distance(held) < 0.01,
        "its flight starts at {centre}, where it was let go at {held}"
    );
    for script in table.client.history() {
        assert_eq!(no_jumps(script), []);
    }
}

#[test]
fn a_card_let_go_over_the_hand_or_cancelled_goes_back_and_sends_nothing() {
    for phase in [PointerPhase::Up, PointerPhase::Cancel] {
        let mut table = Table::new();
        let (_, rest) = table.card(CARD);
        let by = if phase == PointerPhase::Up {
            Vec2::new(0.0, 40.0)
        } else {
            DRAG
        };
        let at = table.carry(by, 0.1, 60);
        table.pointer(phase, at);
        table.frame();
        assert_eq!(table.commands(), [], "{phase:?}");
        // Off the hand, so that the card does not rise under the pointer once it is back.
        table.pointer(PointerPhase::Move, Vec2::new(800.0, 450.0));
        table.wait(0.2, 60);
        let (state, centre) = table.card(CARD);
        assert!(!state.has(StateBits::HELD | StateBits::MOVING), "{phase:?}");
        assert!(centre.distance(rest) < 0.01, "{phase:?}: back at {rest}");
    }
}

#[test]
fn a_card_the_prompt_does_not_mark_cannot_be_picked_up() {
    let mut table = Table::new();
    let island = "Island";
    let (_, start) = table.card(island);
    table.pointer(PointerPhase::Down, start);
    table.now += 16 * MILLISECOND;
    table.pointer(PointerPhase::Move, start + DRAG);
    let (state, centre) = table.card(island);
    assert!(!state.has(StateBits::HELD));
    assert!(centre.distance(start) < 0.01, "it stays where it lies");
}

#[test]
fn a_drag_that_picks_nothing_up_clicks_nothing_and_a_press_clicks() {
    // A drag on a card no prompt lets the player pick up, such as one that scrolls a
    // battlefield row, ends with no click, as the web interface's `Pointer.release` has it.
    let mut table = Table::new();
    let island = "Island";
    let (_, start) = table.card(island);
    table.pointer(PointerPhase::Down, start);
    table.now += 16 * MILLISECOND;
    table.pointer(PointerPhase::Move, start + DRAG);
    table.frame();
    table.pointer(PointerPhase::Up, start + DRAG);
    table.frame();
    assert_eq!(table.commands(), []);
    // A press that does not travel the drag threshold is a click on the card.
    table.now += 16 * MILLISECOND;
    table.pointer(PointerPhase::Down, start);
    table.pointer(PointerPhase::Up, start + Vec2::new(2.0, 0.0));
    table.frame();
    let clicked = table.commands();
    assert!(
        matches!(
            clicked.as_slice(),
            [SessionCommand::Click(ClickRequest {
                click: Click::On {
                    on: Subject::Object { .. }
                },
                ..
            })]
        ),
        "{clicked:?}"
    );
}
