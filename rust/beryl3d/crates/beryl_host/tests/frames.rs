//! The frame loop with a test's own ticks: frames are drawn only on ticks, at most one per
//! tick however many inputs arrive, and no tick is asked for while the client wants none.

use beryl_host::{Draw, FrameLoop, Sleep, Ticks};
use beryl_scene::frame::Frame;
use beryl_scene::input::{Button, Input, Modifiers, Pointer, PointerKind, PointerPhase};
use beryl_scene::{Client, Config};
use glam::Vec2;

/// Ticks that only record what the loop asked for.
#[derive(Default)]
struct TestTicks {
    running: bool,
    /// How many times the loop asked for ticks to run.
    asked: u32,
}

impl Ticks for TestTicks {
    fn set_running(&mut self, running: bool) {
        self.running = running;
        self.asked += u32::from(running);
    }
}

#[derive(Default)]
struct Counter {
    frames: u32,
}

impl Draw for Counter {
    fn draw(&mut self, _frame: &Frame) {
        self.frames += 1;
    }
}

fn client() -> Client {
    Client::new(Config {
        elements: 64,
        draws: 64,
        name_bytes: 1024,
    })
}

fn moved(x: f32) -> Input {
    Input::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase: PointerPhase::Move,
        position: Vec2::new(x, 10.0),
        button: Button::None,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })
}

const INTERVAL: u64 = 8_333_333;

#[test]
fn fifty_inputs_over_ten_ticks_draw_at_most_ten_frames() {
    let mut frames = FrameLoop::new(client(), TestTicks::default());
    let mut counter = Counter::default();
    let mut now = 0;
    for tick in 0..10 {
        for input in 0..5 {
            frames.handle(moved((tick * 5 + input) as f32));
            assert_eq!(counter.frames, tick, "an input drew a frame outside a tick");
            assert_eq!(frames.schedule(now), Sleep::UntilEvent);
            assert!(frames.ticks().running, "an input did not start the ticks");
        }
        now += INTERVAL;
        frames.tick(now, &mut counter);
    }
    assert_eq!(counter.frames, 10);
}

#[test]
fn a_tick_with_nothing_new_draws_nothing() {
    let mut frames = FrameLoop::new(client(), TestTicks::default());
    let mut counter = Counter::default();
    frames.handle(moved(1.0));
    assert!(frames.tick(INTERVAL, &mut counter));
    assert!(!frames.tick(2 * INTERVAL, &mut counter));
    assert_eq!(counter.frames, 1);
}

#[test]
fn no_tick_is_asked_for_while_the_client_wants_no_frame() {
    let mut frames = FrameLoop::new(client(), TestTicks::default());
    let mut counter = Counter::default();
    for step in 0..5 {
        assert_eq!(frames.schedule(step * INTERVAL), Sleep::UntilEvent);
    }
    assert_eq!(frames.ticks().asked, 0);
    // After one input and its frame, the ticks stop again.
    frames.handle(moved(1.0));
    frames.schedule(5 * INTERVAL);
    assert_eq!(frames.ticks().asked, 1);
    frames.tick(6 * INTERVAL, &mut counter);
    frames.schedule(6 * INTERVAL);
    assert!(!frames.ticks().running);
    assert_eq!(frames.ticks().asked, 1);
}
