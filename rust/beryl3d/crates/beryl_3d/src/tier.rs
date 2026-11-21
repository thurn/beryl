//! Choosing the tier at startup (docs/beryl-3d.md, "Choosing a tier"). Before the first frame,
//! with the setting at Automatic, the app draws the `stress-255` board in the Enhanced tier
//! into an offscreen target of the window's size once unmeasured and then
//! `frames` from `tier-probe.json` times, waiting for the GPU after each, and chooses Enhanced when the slowest of them took less than
//! `share` of the display's refresh interval. The setting at Baseline or
//! Enhanced fixes the tier instead, and measures nothing.
//!
//! On this Mac (an Apple M5 Max at 1600 by 900 points, scale 1, while the machine ran other
//! sessions' checks) choosing the tier took about 50 ms, which is the time the measurement
//! adds before the first frame.

use kit_ui::input::Input as DeviceInput;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use app_client::Input;
use app_client::settings::{Graphics, Settings};
use app_client::{Client, Config};
use beryl_host::{Chosen, Opening};
use beryl_protocol::protocol::SessionId;
use beryl_protocol::{GameUpdate, performance_probe};
use beryl_render::{Renderer, Target};
use kit_ui::input::{Insets, Size, Tier};
use serde::{Deserialize, Serialize};

/// How many permanents the measured board holds: the `stress-255` gate's board.
const PERMANENTS: u16 = 255;

/// What the measurement found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    /// The slowest of the frames drawn, or `None` when none was, because the renderer has no
    /// Enhanced tier.
    pub slowest: Option<Duration>,
    /// The display's refresh interval.
    pub interval: Duration,
    /// The tier the measurement chose.
    pub chosen: Tier,
}

/// Overrides the measurement's verdict (`--probe pass|fail`, tools builds only), so that a test
/// of the tier choice does not depend on how loaded the machine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
}

/// The frame log's `tier` line: how the tier was chosen at startup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tiered {
    /// The setting: `automatic`, `baseline` or `enhanced`.
    pub setting: String,
    /// The slowest measured frame, in milliseconds, when frames were measured.
    pub slowest_ms: Option<f64>,
    /// How long choosing the tier took before the first frame, in milliseconds.
    #[serde(default)]
    pub took_ms: f64,
    /// The display's refresh interval, in milliseconds.
    pub interval_ms: f64,
    /// The tier the client starts in: `baseline` or `enhanced`.
    pub tier: String,
}

/// Measures how fast `renderer` draws the Enhanced tier in a window of `size` logical points at
/// `scale`, on a display of `display_hz`. Draws nothing when the renderer has no Enhanced tier.
#[must_use]
pub fn probe(renderer: &mut Renderer, size: Size, scale: f32, display_hz: f32) -> Probe {
    let interval = Duration::from_secs_f64(1.0 / f64::from(display_hz.max(1.0)));
    if !renderer.enhanced_available() {
        return Probe {
            slowest: None,
            interval,
            chosen: Tier::Baseline,
        };
    }
    let data = crate::data::embedded().expect("app tuning");
    let tuning = data.tier_probe;
    let mut client = Client::new(data, Config::LARGEST);
    client.handle(Input::Device(DeviceInput::Surface {
        size,
        scale,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(GameUpdate {
        session: SessionId(1),
        view: performance_probe(PERMANENTS),
    })));
    client.handle(Input::Device(DeviceInput::Tier(Tier::Enhanced)));
    client.prepare(0);
    let frame = client.frame(0);
    let width = (size.width * scale).round().max(1.0) as u32;
    let height = (size.height * scale).round().max(1.0) as u32;
    let mut target = Target::offscreen(renderer, width, height);
    // One frame first, not measured: the first draw into a target makes its multisampled color
    // and depth buffers and first uses each pipeline, which took ten times an ordinary frame.
    renderer.render(frame, &mut target);
    renderer.finish();
    let mut slowest = Duration::ZERO;
    for _ in 0..tuning.frames {
        let started = Instant::now();
        renderer.render(frame, &mut target);
        renderer.finish();
        slowest = slowest.max(started.elapsed());
    }
    let chosen = if slowest.as_secs_f64() < f64::from(tuning.share) * interval.as_secs_f64() {
        Tier::Enhanced
    } else {
        Tier::Baseline
    };
    Probe {
        slowest: Some(slowest),
        interval,
        chosen,
    }
}

/// Chooses the tier the client starts in, for the setting `graphics`, measuring when it is
/// Automatic, with `verdict` replacing the measurement's verdict where given. Returns the
/// choice for the host and the frame log's line.
pub fn choose(
    renderer: &mut Renderer,
    opening: Opening,
    graphics: Graphics,
    verdict: Option<Verdict>,
) -> (Chosen, Tiered) {
    let started = Instant::now();
    let enhanced_available = renderer.enhanced_available();
    let interval = Duration::from_secs_f64(1.0 / f64::from(opening.display_hz.max(1.0)));
    let (wanted, slowest) = match graphics {
        Graphics::Baseline => (Tier::Baseline, None),
        Graphics::Enhanced => (Tier::Enhanced, None),
        Graphics::Automatic => {
            let size = Size {
                width: opening.width as f32 / opening.scale,
                height: opening.height as f32 / opening.scale,
            };
            let probe = probe(renderer, size, opening.scale, opening.display_hz);
            let chosen = match verdict {
                Some(Verdict::Pass) => Tier::Enhanced,
                Some(Verdict::Fail) => Tier::Baseline,
                None => probe.chosen,
            };
            (chosen, probe.slowest)
        }
    };
    let tier = if wanted == Tier::Enhanced && !enhanced_available {
        eprintln!(
            "beryl3d: the Enhanced tier was chosen, but this GPU cannot draw it (no four-sample \
             multisampling, or a downlevel device), so the tier is Baseline"
        );
        Tier::Baseline
    } else {
        wanted
    };
    let tiered = Tiered {
        setting: word(graphics).to_owned(),
        slowest_ms: slowest.map(|slowest| slowest.as_secs_f64() * 1e3),
        took_ms: started.elapsed().as_secs_f64() * 1e3,
        interval_ms: interval.as_secs_f64() * 1e3,
        tier: match tier {
            Tier::Baseline => "baseline",
            Tier::Enhanced => "enhanced",
        }
        .to_owned(),
    };
    let chosen = Chosen {
        graphics,
        tier,
        enhanced_available,
    };
    (chosen, tiered)
}

/// The frame log's `tier` line, once the host's setup has chosen the tier.
pub(crate) type Pending = Rc<RefCell<Option<Tiered>>>;

/// The host's setup before the first frame, which chooses the tier for the graphics setting of
/// `settings` (Automatic without any) with `choose`, and where it leaves the frame log's line
/// for the app's feed to write once the log exists.
pub(crate) fn setup(
    settings: Option<&Settings>,
    verdict: Option<Verdict>,
) -> (
    impl FnOnce(&mut Renderer, Opening) -> Chosen + use<>,
    Pending,
) {
    let graphics = settings.map_or(Graphics::Automatic, |settings| settings.graphics);
    let pending = Pending::default();
    let tiered = Rc::clone(&pending);
    let setup = move |renderer: &mut Renderer, opening: Opening| {
        let (chosen, line) = choose(renderer, opening, graphics, verdict);
        *tiered.borrow_mut() = Some(line);
        chosen
    };
    (setup, pending)
}

/// The word the command line and the frame log use for `graphics`.
#[must_use]
pub fn word(graphics: Graphics) -> &'static str {
    match graphics {
        Graphics::Automatic => "automatic",
        Graphics::Baseline => "baseline",
        Graphics::Enhanced => "enhanced",
    }
}

/// The setting `word` names, as `--tier` writes it.
#[must_use]
pub fn graphics(word: &str) -> Option<Graphics> {
    [Graphics::Automatic, Graphics::Baseline, Graphics::Enhanced]
        .into_iter()
        .find(|graphics| self::word(*graphics) == word)
}
