//! The frame loop with a test's own ticks: frames are drawn only on ticks, at most one per
//! tick however many inputs arrive, no tick is asked for while the client wants none, and a
//! paused clock draws one frame at the pause and no other until an input arrives.

use app_client::Input;
use app_client::{Client, Config};
use beryl_host::{Draw, FrameLoop, Shown, Sleep, Ticks};
use glam::Vec2;
use kit_frame::frame::Frame;
use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

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
    fn draw(&mut self, _frame: &Frame) -> Option<Shown> {
        self.frames += 1;
        Some(Shown::default())
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
    assert!(frames.tick(INTERVAL, &mut counter).is_some());
    assert!(frames.tick(2 * INTERVAL, &mut counter).is_none());
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

#[test]
fn a_paused_clock_draws_one_frame_at_the_pause_and_then_only_for_an_input() {
    let pause = 3 * INTERVAL;
    let mut frames = FrameLoop::new(client(), TestTicks::default()).paused_at(Some(pause));
    let mut counter = Counter::default();
    // A still table before the pause still wakes at the pause, to show it.
    assert_eq!(frames.schedule(INTERVAL), Sleep::Until(pause));
    let at_pause = frames
        .tick(pause + 1000, &mut counter)
        .expect("a frame at the pause");
    assert_eq!(at_pause.clock, pause);
    for tick in 4..20 {
        assert!(frames.tick(tick * INTERVAL, &mut counter).is_none());
        assert_eq!(frames.schedule(tick * INTERVAL), Sleep::UntilEvent);
    }
    frames.handle(moved(1.0));
    let after_input = frames
        .tick(30 * INTERVAL, &mut counter)
        .expect("a frame for the input");
    assert_eq!(after_input.clock, pause, "the clock moved past the pause");
    assert!(frames.tick(31 * INTERVAL, &mut counter).is_none());
    assert_eq!(counter.frames, 2);
}

#[test]
fn the_gap_since_the_last_frame_is_measured_only_while_the_ticks_run() {
    let mut frames = FrameLoop::new(client(), TestTicks::default());
    let mut counter = Counter::default();
    frames.handle(moved(1.0));
    let first = frames.tick(INTERVAL, &mut counter).expect("a frame");
    assert_eq!(first.since_last, None);
    frames.handle(moved(2.0));
    let second = frames.tick(2 * INTERVAL, &mut counter).expect("a frame");
    assert_eq!(second.since_last, Some(INTERVAL));
    // The table rests, the ticks stop, and the next frame starts a new run of ticks.
    assert!(frames.tick(3 * INTERVAL, &mut counter).is_none());
    frames.handle(moved(3.0));
    let later = frames.tick(10 * INTERVAL, &mut counter).expect("a frame");
    assert_eq!(later.since_last, None);
}

/// A feed that wants a frame at a fixed time, as a replayed recording wants one when its next
/// view is due.
struct DueAt(u64);

impl beryl_host::Feed for DueAt {
    fn wake(&self) -> kit_frame::frame::Wake {
        kit_frame::frame::Wake::At(self.0)
    }
}

/// A phone's host returns the loop's wake to the platform, which may stop its display link
/// until then. With the client still, that wake must still come from the feed and the pause,
/// or a replay would stop at its first still moment.
#[test]
fn the_loops_wake_counts_the_feed_and_the_pause() {
    use kit_frame::frame::Wake;
    let mut frames = FrameLoop::new(client(), TestTicks::default())
        .with_feed(Box::new(DueAt(5 * INTERVAL)))
        .paused_at(Some(3 * INTERVAL));
    frames.tick(INTERVAL, &mut Counter::default());
    assert_eq!(frames.client().wake(), Wake::Never);
    assert_eq!(frames.next_wake(INTERVAL), Wake::At(3 * INTERVAL));

    let mut frames =
        FrameLoop::new(client(), TestTicks::default()).with_feed(Box::new(DueAt(5 * INTERVAL)));
    frames.tick(INTERVAL, &mut Counter::default());
    assert_eq!(frames.next_wake(INTERVAL), Wake::At(5 * INTERVAL));
}
