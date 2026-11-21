//! The phone host's frame rate: which display ticks draw when the rate rule has chosen less
//! than the display's highest rate, and a loop paced as a phone's steps from 120 Hz to 60 Hz
//! when frames keep missing 120 Hz, leaving the rate in its `Platform` for the host to apply.
//! Also the phone host before the platform has passed a surface: it has no renderer yet, so a
//! lifecycle input is ignored and no frame is drawn.

use app_client::Input;
use app_client::{Client, Config};
use beryl_host::phone::{Platform, due};
use beryl_host::{Draw, FrameLoop, NoFeed, Phone, Shown};
use glam::Vec2;
use kit_frame::frame::{Frame, Wake};
use kit_ui::input::{Button, Lifecycle, Modifiers, Pointer, PointerKind, PointerPhase};

/// The interval of a 120 Hz display, in nanoseconds.
const INTERVAL_120: u64 = 8_333_333;

/// Which of `ticks` ticks of a display at `display_hz` draw with the rule at `hz`.
fn drawn(ticks: u64, hz: f32, display_hz: f32) -> Vec<u64> {
    let interval = (1e9 / f64::from(display_hz)).round() as u64;
    let mut last = None;
    let mut drawn = Vec::new();
    for tick in 0..ticks {
        let now = tick * interval;
        if due(last, now, hz, display_hz) {
            last = Some(now);
            drawn.push(tick);
        }
    }
    drawn
}

#[test]
fn a_display_faster_than_the_rate_draws_only_on_the_ticks_the_rate_falls_on() {
    assert_eq!(drawn(7, 60.0, 120.0), [0, 2, 4, 6]);
    assert_eq!(drawn(4, 60.0, 60.0), [0, 1, 2, 3]);
    assert_eq!(drawn(10, 40.0, 120.0), [0, 3, 6, 9]);
    assert!(due(None, 5, 30.0, 120.0), "a first tick always draws");
}

struct Discard;

impl Draw for Discard {
    fn draw(&mut self, _frame: &Frame) -> Option<Shown> {
        Some(Shown::default())
    }
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

#[test]
fn frames_that_keep_missing_120_hz_step_the_phone_to_60_hz() {
    let client = Client::new(Config {
        elements: 64,
        draws: 64,
        name_bytes: 1024,
    });
    let mut frames = FrameLoop::new(client, Platform::default()).paced(120.0, false);
    assert!((frames.ticks_ref().hz() - 120.0).abs() < 1e-3);
    // Each frame comes three 120 Hz intervals after the last: late at 120 Hz, on time at 60.
    let mut now = 0;
    for frame in 0..200 {
        frames.handle(moved(frame as f32));
        frames
            .tick(now, &mut Discard)
            .expect("an input wants a frame");
        now += 3 * INTERVAL_120;
    }
    let hz = frames.ticks_ref().hz();
    assert!((hz - 60.0).abs() < 1e-3, "stayed at {hz} Hz");
}

#[test]
fn low_memory_before_a_surface_is_ignored() {
    let mut phone = Phone::new(Client::new(Config::LARGEST), 60.0, None, |_| {
        Box::new(NoFeed)
    });
    phone.lifecycle(Lifecycle::LowMemory);
    phone.surface_lost();
    assert_eq!(phone.frame(0), Wake::Never);
}
