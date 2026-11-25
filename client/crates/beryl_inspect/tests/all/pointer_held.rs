//! A hand card picked up, carried and let go (docs/beryl-3d.md, "Motion the pointer drives"),
//! tested at priority with a castable Trinket Mage and with two playable Plains. The client is
//! driven directly, so each test sees the commands it queues and the frames between events.

use app_client::Input;
use app_client::command::{Reply, SessionCommand};
use app_client::driver::{Recording, Responses, plays_on};
use app_client::{Client, Config};
use beryl_inspect::pages::{page, recording};
use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::{Click, ClickReply, ClickRequest};
use beryl_protocol::protocol::{ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::Subject;
use game_probe::agent::steps::no_jumps;
use game_table::table::Place;
use glam::{Vec2, Vec3};
use kit_base::data::profiles::SizeClass;
use kit_frame::frame::{Frame, Material, Resolved, Role, StateBits, Wake};
use kit_scene::hit::uncovered;
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::Size;
use kit_ui::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase};

const CARD: &str = "Trinket Mage";
/// Trinket Mage's slot in `shape-zone`.
const SLOT: u16 = 10;
const MILLISECOND: u64 = 1_000_000;

/// The client at a fixture's first prompt, with the table at rest.
struct Table {
    client: Client,
    recording: Recording,
    views: Vec<GameUpdate>,
    now: u64,
}

impl Table {
    fn new() -> Self {
        Self::recording("shape-zone")
    }

    fn recording(name: &str) -> Self {
        let recording = recording(name, Side::You).unwrap_or_else(|why| panic!("{why}"));
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

    fn card_slot(&mut self, slot: ObjectId) -> Resolved {
        *self
            .frame()
            .resolved
            .iter()
            .find(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
            .unwrap_or_else(|| panic!("no card in slot {}", slot.0))
    }

    fn press_point(&mut self, slot: ObjectId) -> Vec2 {
        let frame = self.frame();
        let index = frame
            .resolved
            .iter()
            .position(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
            .expect("the card is drawn");
        let visible =
            uncovered(frame, u32::try_from(index).unwrap()).expect("the card can be pressed");
        Vec2::new(
            visible.x + visible.width / 2.0,
            visible.y + visible.height / 2.0,
        )
    }

    fn play_on(&mut self) {
        for update in &self.views[1..] {
            self.client.handle(Input::Update(Box::new(update.clone())));
            if !plays_on(update) {
                break;
            }
        }
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
        let (state, corners) = self.card_face(name);
        (state, crossing(corners))
    }

    fn card_face(&mut self, name: &str) -> (StateBits, [Vec2; 4]) {
        let frame = self.frame();
        let element = frame
            .resolved
            .iter()
            .find(|element| element.role == Role::Card && frame.name(element.name) == name)
            .unwrap_or_else(|| panic!("no card {name}"));
        (element.state, element.corners)
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
fn playing_a_hand_twin_preserves_the_chosen_pose_and_the_other_cards_order() {
    // The first Plains is offered by the session. The second is the recorded click,
    // with an Island between them in the fan.
    let [offered, island, chosen] = [ObjectId(0), ObjectId(1), ObjectId(2)];
    for drag in [true, false] {
        let mut table = Table::recording("hand-pair-twin");
        let other = crossing(table.card_slot(offered).corners);
        let start = table.press_point(chosen);
        table.pointer(PointerPhase::Down, start);
        let flight_from = if drag {
            let at = start + DRAG;
            table.pointer(PointerPhase::Move, at);
            let held = table.card_slot(chosen);
            assert!(held.state.has(StateBits::HELD));
            table.pointer(PointerPhase::Up, at);
            // A delayed answer must still start the flight from where the card was dropped.
            table.wait(0.5, 60);
            assert!(crossing(table.card_slot(chosen).corners).distance(at) < 0.01);
            at
        } else {
            let from = crossing(table.card_slot(chosen).corners);
            table.pointer(PointerPhase::Up, start);
            from
        };
        let [command] = &table.commands()[..] else {
            panic!("handling the second Plains sends one click");
        };
        let answer = Responses::new(&table.recording)
            .answer(command)
            .unwrap_or_else(|why| panic!("{why}"));
        assert!(answer.plays_on);
        if drag {
            table.client.handle(Input::Reply(answer.reply));
            table.play_on();
        } else {
            // The next view can arrive before the click reply.
            table.play_on();
            table.client.handle(Input::Reply(answer.reply));
        }
        let flying = table.card_slot(offered);
        assert!(flying.state.has(StateBits::MOVING));
        let from = crossing(flying.corners);
        assert!(
            from.distance(flight_from) < 0.01,
            "flight began at {from}, expected {flight_from}"
        );
        assert!(crossing(table.card_slot(chosen).corners).distance(other) < 0.01);

        table.wait(0.8, 60);
        assert!(matches!(
            Place::in_frame(table.card_slot(offered).place),
            Some(Place::Card {
                zone: ZoneName::Battlefield,
                ..
            })
        ));
        assert!(
            crossing(table.card_slot(chosen).corners).x
                < crossing(table.card_slot(island).corners).x
        );
    }
}

#[test]
fn a_drop_that_leaves_priority_open_returns_the_card_to_its_place() {
    let mut table = Table::recording("hand-pair-twin");
    let slot = ObjectId(2);
    let start = table.press_point(slot);
    let rest = crossing(table.card_slot(slot).corners);
    let Status::Deciding { decision } = &table.views[0].view.status else {
        panic!("the first view asks the viewer");
    };
    let prompt = decision.prompt.clone();
    let at = start + DRAG;
    table.pointer(PointerPhase::Down, start);
    table.pointer(PointerPhase::Move, at);
    table.pointer(PointerPhase::Up, at);
    assert_eq!(table.commands().len(), 1);
    table
        .client
        .handle(Input::Reply(Reply::Click(Ok(ClickReply::Open { prompt }))));
    table.wait(0.8, 60);
    assert!(crossing(table.card_slot(slot).corners).distance(rest) < 0.01);
}

#[test]
fn a_held_hand_card_casts_a_visible_shadow_while_moving_and_at_rest() {
    let lighting = crate::test_data::data().lighting.shadow;
    let floor = lighting.contact.above;
    for (recording, slot) in [
        ("hand-pair-twin", ObjectId(1)),
        ("shape-zone", ObjectId(SLOT)),
    ] {
        let mut table = Table::recording(recording);
        let rest = table.card_slot(slot).pose;
        let start = table.press_point(slot);
        table.pointer(PointerPhase::Down, start);
        // Cross empty felt, pause long enough for the lean to settle, then move again.
        let mut last = Vec2::ZERO;
        for tick in 0_u32..54 {
            let at = Vec2::new(
                1_060.0 + tick.min(10) as f32 * 14.0,
                415.0 - tick.saturating_sub(43) as f32 * 9.0,
            );
            table.now += 16 * MILLISECOND;
            if at != last {
                table.pointer(PointerPhase::Move, at);
                last = at;
            }
            let card = table.card_slot(slot);
            assert!(card.state.has(StateBits::HELD));
            let pose = card.pose.expect("the held card is in table space");
            let expected = pose.position.truncate() + lighting.fall() * pose.position.z;
            let frame = table.frame();
            let shadow = frame
                .draws
                .iter()
                .find(|draw| {
                    draw.material == Material::ContactShadow as u8
                        && Vec2::new(draw.model[3], draw.model[7]).distance(expected) < 0.01
                        && (draw.model[11] - floor).abs() < 0.01
                })
                .unwrap_or_else(|| {
                    panic!("{recording} tick {tick}: no shadow on the felt at {expected}")
                });
            // Measure the midpoints of the shadow's short edges, inset by its blur. At
            // least one must be below the face so its solid part is clear of the glow.
            let project = |y: f32| {
                frame.view_projection.project_point3(Vec3::new(
                    shadow.model[1] * y + shadow.model[3],
                    shadow.model[5] * y + shadow.model[7],
                    shadow.model[11],
                ))
            };
            let edge = frame.card.y * 0.5 - 2.0 * shadow.params[0];
            let shadow_bottom = (1.0 - project(-edge).y.min(project(edge).y)) * 450.0;
            let card_bottom = card
                .corners
                .into_iter()
                .map(|point| point.y)
                .fold(0.0, f32::max);
            assert!(
                shadow_bottom - card_bottom > 12.0,
                "{recording} tick {tick}: shadow ends at {shadow_bottom}, face ends at {card_bottom}"
            );
        }

        table.pointer(PointerPhase::Cancel, last);
        table.wait(0.25, 60);
        let released = table.card_slot(slot);
        assert!(!released.state.has(StateBits::HELD));
        assert_eq!(released.pose, rest, "the shadow returns with the card");
        assert_eq!(table.commands(), []);
    }
}

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
fn a_held_hand_card_keeps_the_web_face_size_across_the_battlefield() {
    // WebKit measurements at a regular desktop size, a window taller than its 16:10 stage,
    // and a phone with a narrower hand.
    for (width, height, layout, face) in [
        (1600.0, 900.0, SizeClass::Regular, Vec2::new(193.0, 269.6)),
        (1728.0, 1117.0, SizeClass::Regular, Vec2::new(231.6, 323.5)),
        (393.0, 759.0, SizeClass::Compact, Vec2::new(100.0, 139.6)),
    ] {
        for at in [
            Vec2::new(width * 0.74, height * 0.51),
            Vec2::new(width * 0.30, height * 0.30),
        ] {
            let mut page = page("shape-zone", Side::You, layout, Size { width, height })
                .expect("shape-zone recording");
            page.seek(&"prompt:1".parse().expect("prompt moment"))
                .expect("priority to cast");
            page.still();
            page.run(&format!(
                "hold card \"{CARD}\" over {},{}\nstep 700ms",
                at.x, at.y
            ));
            let held = page.find(&format!("card \"{CARD}\""));
            assert!(held.state.has(StateBits::HELD), "card under the pointer");
            let size = Vec2::new(held.rect.width, held.rect.height);
            assert!(
                (size - face).abs().max_element() < 1.0,
                "at {width}×{height}, held at {at}: {size}, web face {face}"
            );
            let corners = page.frame().resolved[held.index as usize].corners;
            assert!(crossing(corners).distance(at) < 0.01);
            let faults = page.faults();
            assert!(
                faults.is_empty(),
                "{width}×{height}, held at {at}: {faults:?}"
            );
        }
    }
}

#[test]
fn dragging_a_castable_card_onto_the_battlefield_casts_it_from_where_it_was_let_go() {
    let mut table = Table::new();
    let Status::Deciding { decision } = &table.views[0].view.status else {
        panic!("the first view asks the viewer");
    };
    let (decision, revision) = (decision.id, decision.prompt.revision);
    let at = table.carry(DRAG, 0.25, 60);
    let (_, held) = table.card_face(CARD);
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
    table.play_on();
    let (state, corners) = table.card_face(CARD);
    assert!(state.has(StateBits::MOVING), "the cast moves it");
    for (start, before_release) in corners.into_iter().zip(held) {
        assert!(
            start.distance(before_release) < 0.01,
            "the cast starts with a corner at {start}, held at {before_release}"
        );
    }
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

/// A hand card dragged out and let go over the battlefield flies to the stack from where it was
/// let go and never heads back toward its slot in the hand, whether the cast resolves at once
/// (Trinket Mage in `shape-zone`) or stops on the stack for its payment (Staff of the Storyteller
/// in `pay-default`). The cast's flight is a spline with a waypoint placed between the hand slot
/// and the stack, which once stayed there when the flight was moved to start at the release, so
/// the card dipped toward the hand before rising to the stack. The generic motion check cannot
/// see this: recordings replay no drag, and the bent path was smooth, with no jump in it, so
/// this test samples the card's flight frame by frame.
#[test]
fn a_card_dragged_out_to_cast_never_heads_back_toward_its_hand_slot() {
    for (name, card, slot) in [
        ("shape-zone", CARD, ObjectId(SLOT)),
        ("pay-default", "Staff of the Storyteller", ObjectId(1)),
    ] {
        let mut table = Table::recording(name);
        // A recording may start by setting the stops, which this test does not answer.
        table.commands();
        let rest = crossing(table.card_slot(slot).corners);
        let start = table.press_point(slot);
        table.pointer(PointerPhase::Down, start);
        let mut at = start;
        for frame in 1..=15 {
            at = start + DRAG * (frame as f32 / 15.0);
            table.now += 16 * MILLISECOND;
            table.pointer(PointerPhase::Move, at);
        }
        assert!(table.card_slot(slot).state.has(StateBits::HELD), "{name}");
        table.pointer(PointerPhase::Up, at);
        table.frame();
        let commands = table.commands();
        let [command] = &commands[..] else {
            panic!("{name}: the drop sends one click, not {commands:?}");
        };
        // `pay-default` recorded no reply to the click. Its next view closes the decision
        // alone, as a view the session sends ahead of the reply does.
        if let Ok(answer) = Responses::new(&table.recording).answer(command) {
            table.client.handle(Input::Reply(answer.reply));
        }
        table.play_on();
        let let_go = at.distance(rest);
        let mut last = at;
        for frame in 0..60 {
            let (_, centre) = table.card(card);
            assert!(
                centre.distance(rest) >= let_go - 1.0,
                "{name}: at frame {frame} {card} is at {centre}, nearer its hand slot at {rest} \
                 than where it was let go at {at}"
            );
            last = centre;
            table.wait(0.016, 60);
        }
        assert!(
            last.distance(at) > 100.0,
            "{name}: {card} flew on to the stack"
        );
    }
}
