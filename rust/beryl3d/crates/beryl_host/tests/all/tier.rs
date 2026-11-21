//! The tier in the frame loop (docs/beryl-3d.md, "Choosing a tier"): an Enhanced tier chosen
//! automatically drops to Baseline when frames run late, before the rate steps, and never
//! otherwise; a change of the setting during play changes the tier or the drop.

use app_client::Input;
use app_client::Press;
use app_client::settings::{Graphics, Setting};
use app_client::{Client, Config};
use beryl_host::rate::Step;
use beryl_host::{Chosen, Draw, FrameLoop, Shown, Ticks};
use glam::Vec2;
use kit_base::data::space::frame_rate;
use kit_frame::frame::Frame;
use kit_ui::element::Action;
use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase, Tier};

/// Ticks at whatever rate the loop asks for; the test sets each tick's time itself.
#[derive(Default)]
struct AnyTicks;

impl Ticks for AnyTicks {
    fn set_running(&mut self, _running: bool) {}
}

struct Shows;

impl Draw for Shows {
    fn draw(&mut self, _frame: &Frame) -> Option<Shown> {
        Some(Shown::default())
    }
}

/// A sixtieth of a second, in nanoseconds: on time at 60 Hz.
const ON_TIME: u64 = 16_666_667;
/// Two sixtieths: more than one and a half intervals late at 60 Hz, so every such frame
/// misses.
const LATE: u64 = 2 * ON_TIME;

const ENHANCED: Chosen = Chosen {
    graphics: Graphics::Automatic,
    tier: Tier::Enhanced,
    enhanced_available: true,
};

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

/// Plays animating frames, the first `settle_frames` on time and every one after late, and
/// returns each step the loop took with the index of the frame after the window settled that
/// took it (0 for the first late frame).
fn steps(frames: &mut FrameLoop<AnyTicks>, late_frames: usize) -> Vec<(usize, Step)> {
    let mut now = 0;
    let mut steps = Vec::new();
    // One frame before the settling ones, which has no gap to measure.
    for index in 0..=frame_rate::SETTLE_FRAMES + late_frames {
        now += if index <= frame_rate::SETTLE_FRAMES {
            ON_TIME
        } else {
            LATE
        };
        frames.handle(moved(index as f32));
        let ticked = frames
            .tick(now, &mut Shows)
            .expect("an input draws a frame");
        if let Some(step) = ticked.step {
            assert!(
                index > frame_rate::SETTLE_FRAMES,
                "a step while the window settled: {step:?} at frame {index}"
            );
            steps.push((index - frame_rate::SETTLE_FRAMES - 1, step));
        }
    }
    steps
}

#[test]
fn an_automatic_enhanced_tier_drops_to_baseline_before_the_rate_steps() {
    let mut frames = FrameLoop::new(client(), AnyTicks)
        .paced(60.0, ENHANCED.tier_can_drop())
        .with_tier(ENHANCED);
    assert_eq!(frames.client().tier(), Tier::Enhanced);
    let steps = steps(&mut frames, 120);
    let (at, first) = steps[0];
    assert_eq!(
        first,
        Step::Baseline,
        "the first step drops the tier: {steps:?}"
    );
    assert!(
        at < 120,
        "the tier dropped {at} frames after the first miss"
    );
    assert_eq!(frames.client().tier(), Tier::Baseline);
    assert_eq!(
        steps
            .iter()
            .filter(|(_, step)| *step == Step::Baseline)
            .count(),
        1
    );
}

#[test]
fn a_tier_that_may_not_drop_stays_enhanced_while_the_rate_steps() {
    let chosen = Chosen {
        graphics: Graphics::Enhanced,
        ..ENHANCED
    };
    assert!(!chosen.tier_can_drop());
    let mut frames = FrameLoop::new(client(), AnyTicks)
        .paced(60.0, chosen.tier_can_drop())
        .with_tier(chosen);
    let steps = steps(&mut frames, 120);
    assert!(!steps.is_empty(), "late frames step the rate");
    assert!(
        steps.iter().all(|(_, step)| *step != Step::Baseline),
        "{steps:?}"
    );
    assert_eq!(frames.client().tier(), Tier::Enhanced);
}

#[test]
fn a_fixed_rate_still_drops_the_tier_and_never_steps_the_rate() {
    let mut frames = FrameLoop::new(client(), AnyTicks)
        .fixed_at(60.0, ENHANCED.tier_can_drop())
        .with_tier(ENHANCED);
    let steps = steps(&mut frames, 120);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].1, Step::Baseline);
    assert!(steps[0].0 < 120);
    assert_eq!(frames.client().tier(), Tier::Baseline);
}

/// Presses the setting and lets the loop see the settings the client asks to save.
fn set(frames: &mut FrameLoop<AnyTicks>, graphics: Graphics) {
    frames
        .client()
        .press(&Action::new(Press::Setting(Setting::Graphics(graphics))));
    while frames.next_request().is_some() {}
}

#[test]
fn the_setting_changed_during_play_changes_the_tier_and_its_drop() {
    let mut frames = FrameLoop::new(client(), AnyTicks)
        .paced(60.0, ENHANCED.tier_can_drop())
        .with_tier(ENHANCED);
    // Fixed to Enhanced: the tier stays Enhanced however late the frames.
    set(&mut frames, Graphics::Enhanced);
    let fixed = steps(&mut frames, 60);
    assert!(
        fixed.iter().all(|(_, step)| *step != Step::Baseline),
        "{fixed:?}"
    );
    assert_eq!(frames.client().tier(), Tier::Enhanced);
    // Fixed to Baseline: drawn in Baseline at once.
    set(&mut frames, Graphics::Baseline);
    assert_eq!(frames.client().tier(), Tier::Baseline);
    // Back to Automatic in Baseline: the tier stays Baseline, and Enhanced comes back only by
    // asking for it.
    set(&mut frames, Graphics::Automatic);
    assert_eq!(frames.client().tier(), Tier::Baseline);
}

#[test]
fn enhanced_on_a_renderer_without_it_stays_baseline() {
    let chosen = Chosen {
        tier: Tier::Baseline,
        enhanced_available: false,
        ..ENHANCED
    };
    let mut frames = FrameLoop::new(client(), AnyTicks)
        .paced(60.0, chosen.tier_can_drop())
        .with_tier(chosen);
    set(&mut frames, Graphics::Enhanced);
    assert_eq!(frames.client().tier(), Tier::Baseline);
}
