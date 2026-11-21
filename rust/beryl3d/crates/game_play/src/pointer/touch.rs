//! What the fingers on a touch screen are doing (docs/beryl-3d.md, "Hosts"; the phone's
//! gestures in docs/ui-conventions.md, "The phone"). The client acts on one finger at a time,
//! the active one, and this decides which finger that is, when a touch held still has been
//! held long enough, and when a double tap opens the bug report.
//!
//! - **One active finger.** The first finger down is the active one; the client's pointer,
//!   flat layer and rows hear only its events. A finger that lands within `TOGETHER_MS` of the
//!   first makes a multi-finger tap: the active finger's press ends with nothing done, and no
//!   finger of that touch reaches the game. A finger that lands later is another thumb, and is
//!   ignored while the first goes on, as the web's `ReportTaps` and `inMultiTouch` have it
//!   (`ui/src/mobile/report-taps.ts`).
//! - **A hold.** An active finger that stays within `DRAG_THRESHOLD_POINTS` of where it went
//!   down for `LONG_PRESS_MS` is due (`Touch::take_hold`), and the client opens the card
//!   inspector or the manual table's menu there. The hold is timed from the first frame after
//!   the finger went down, because an input has no time of its own.
//! - **A pickup.** On the manual table, where a held touch opens a card's menu, a finger may pick
//!   a card up only once it has been down `PICKUP_MS` without travelling the drag threshold, as
//!   the web's `TOUCH_DRAG_DELAY` has it (`ui/src/stage/pointer.ts`); before that, moving lets
//!   the hand or row move under it instead.
//! - **A double tap** opens the bug report: two taps with one finger on bare felt, or two taps
//!   with two or more fingers landing together anywhere. Each tap is short (`TAP_MS`), drifts at
//!   most `SLOP_POINTS`, and the second lands within `DOUBLE_MS` of the first's lift and within
//!   `SLOP_POINTS` of where it landed.
//!
//! The state is bounded by the pointer ids a host can send: one slot for each.

use glam::Vec2;

use kit_base::data::space::pointer::touch::{
    DOUBLE_MS, PICKUP_MS, SLOP_POINTS, TAP_MS, TOGETHER_MS,
};
use kit_base::data::space::pointer::{DRAG_THRESHOLD_POINTS, LONG_PRESS_MS};

/// Nanoseconds in a millisecond, the unit of `Client::frame`'s clock.
const MILLISECOND: u64 = 1_000_000;

/// The number of pointer ids, one slot each (`Pointer::id` is a `u8`).
const FINGERS: usize = 256;

/// What the client does with a finger that has just gone down (`Touch::down`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landed {
    /// It is the active finger: the game hears it.
    Active,
    /// It joined the active finger in time to make a multi-finger tap: the active finger's
    /// press ends with nothing done, and the game hears neither.
    Together,
    /// It landed too late to join the tap, or after the tap's fingers began lifting: the game
    /// does not hear it.
    Ignored,
}

/// What the fingers on the screen are doing, as the module describes.
#[derive(Clone, Debug, PartialEq)]
pub struct Touch {
    /// Where each finger down landed, by pointer id.
    fingers: Box<[Option<Vec2>; FINGERS]>,
    /// How many fingers are down.
    down: usize,
    /// The finger the game hears, and its hold and pickup.
    active: Option<Active>,
    /// The touch under way, from its first finger landing to its last lifting.
    tap: Option<Tap>,
    /// The tap before, waiting for a second one: when its last finger lifted and where its
    /// first landed.
    last: Option<(u64, Vec2)>,
}

/// The finger the game hears.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Active {
    id: u8,
    start: Vec2,
    /// Whether a held touch here means something, and whether a card it presses waits for
    /// `PICKUP_MS` before it can be picked up.
    hold: bool,
    pickup: bool,
    /// The `now` of the first frame after the finger went down, from which the hold and the
    /// pickup are timed.
    since: Option<u64>,
}

/// The touch under way, as `ReportTaps` keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tap {
    /// When and where its first finger landed, and whether on bare felt.
    start: u64,
    at: Vec2,
    felt: bool,
    /// Whether a second finger landed with the first.
    multi: bool,
    /// A finger moved too far or landed too late: the touch is no tap.
    spoiled: bool,
}

impl Default for Touch {
    fn default() -> Self {
        Self {
            fingers: Box::new([None; FINGERS]),
            down: 0,
            active: None,
            tap: None,
            last: None,
        }
    }
}

/// `milliseconds` as nanoseconds. The values in `space.json` are whole milliseconds.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn nanoseconds(milliseconds: f32) -> u64 {
    milliseconds as u64 * MILLISECOND
}

impl Touch {
    /// Finger `id` went down at `at` at `now`, on bare felt if `felt`. Says what the game makes
    /// of it.
    pub fn down(&mut self, id: u8, at: Vec2, now: u64, felt: bool) -> Landed {
        let slot = &mut self.fingers[usize::from(id)];
        if slot.replace(at).is_none() {
            self.down += 1;
        }
        let first = self.down == 1;
        let landed = match &mut self.tap {
            Some(tap) if !first => {
                if now.saturating_sub(tap.start) <= nanoseconds(TOGETHER_MS) {
                    tap.multi = true;
                    Landed::Together
                } else {
                    tap.spoiled = true;
                    Landed::Ignored
                }
            }
            _ => {
                self.tap = Some(Tap {
                    start: now,
                    at,
                    felt,
                    multi: false,
                    spoiled: false,
                });
                Landed::Active
            }
        };
        self.active = match landed {
            Landed::Active => Some(Active {
                id,
                start: at,
                hold: false,
                pickup: false,
                since: None,
            }),
            Landed::Together => None,
            Landed::Ignored => self.active,
        };
        landed
    }

    /// Times a hold for the active finger: one held still `LONG_PRESS_MS` is then due.
    pub fn wait_for_hold(&mut self) {
        if let Some(active) = &mut self.active {
            active.hold = true;
        }
    }

    /// Times a pickup for the active finger: a card it pressed may be picked up once it has
    /// been down `PICKUP_MS` without moving (`take_pickup`).
    pub fn wait_for_pickup(&mut self) {
        if let Some(active) = &mut self.active {
            active.pickup = true;
        }
    }

    /// Finger `id` moved to `at`. Returns whether it is the active finger. Moving
    /// `DRAG_THRESHOLD_POINTS` from where it landed ends its hold and its wait for a pickup,
    /// and moving more than `SLOP_POINTS` means the touch is no tap.
    pub fn moved(&mut self, id: u8, at: Vec2) -> bool {
        if let (Some(landed), Some(tap)) = (self.fingers[usize::from(id)], &mut self.tap)
            && landed.distance(at) > SLOP_POINTS
        {
            tap.spoiled = true;
        }
        let Some(active) = self.active.as_mut().filter(|active| active.id == id) else {
            return false;
        };
        if active.start.distance(at) >= DRAG_THRESHOLD_POINTS {
            active.hold = false;
            active.pickup = false;
        }
        true
    }

    /// Finger `id` lifted at `now`. Returns whether it was the active finger, and whether its
    /// lift finished a double tap.
    pub fn up(&mut self, id: u8, now: u64) -> (bool, bool) {
        let active = self.lift(id);
        if self.down > 0 {
            return (active, false);
        }
        let Some(tap) = self.tap.take() else {
            return (active, false);
        };
        let tapped = !tap.spoiled
            && now.saturating_sub(tap.start) <= nanoseconds(TAP_MS)
            && (tap.multi || tap.felt);
        if !tapped {
            self.last = None;
            return (active, false);
        }
        let double = self.last.take().is_some_and(|(end, at)| {
            tap.start.saturating_sub(end) <= nanoseconds(DOUBLE_MS)
                && at.distance(tap.at) <= SLOP_POINTS
        });
        if !double {
            self.last = Some((now, tap.at));
        }
        (active, double)
    }

    /// The host took finger `id` (a system gesture): the touch is no tap. Returns whether it was
    /// the active finger.
    pub fn cancel(&mut self, id: u8) -> bool {
        if self.fingers[usize::from(id)].is_none() {
            return false;
        }
        let active = self.lift(id);
        if let Some(tap) = &mut self.tap {
            tap.spoiled = true;
        }
        if self.down == 0 {
            self.tap = None;
        }
        self.last = None;
        active
    }

    /// Forgets every finger, as when the surface changes or the host cancels every touch.
    /// Returns whether there was an active finger.
    pub fn reset(&mut self) -> bool {
        self.fingers.fill(None);
        self.down = 0;
        self.tap = None;
        self.last = None;
        self.active.take().is_some()
    }

    /// Whether `id` is the active finger.
    #[must_use]
    pub fn is_active(&self, id: u8) -> bool {
        self.active.is_some_and(|active| active.id == id)
    }

    /// Starts timing the active finger's hold and pickup at the frame at `now`, if they have
    /// not begun.
    pub fn frame(&mut self, now: u64) {
        if let Some(active) = &mut self.active
            && (active.hold || active.pickup)
        {
            active.since.get_or_insert(now);
        }
    }

    /// Where the active finger landed, if its hold is due at `now`; the hold then ends. The
    /// finger stays active, so the rest of its events still reach the game, which has ended
    /// its press.
    pub fn take_hold(&mut self, now: u64) -> Option<Vec2> {
        let active = self.active.as_mut().filter(|active| active.hold)?;
        let due = active.since? + nanoseconds(LONG_PRESS_MS);
        if now < due {
            return None;
        }
        active.hold = false;
        Some(active.start)
    }

    /// Whether the active finger's wait for a pickup is over at `now`; the wait then ends.
    pub fn take_pickup(&mut self, now: u64) -> bool {
        let Some(active) = self.active.as_mut().filter(|active| active.pickup) else {
            return false;
        };
        if active
            .since
            .is_none_or(|since| now < since + nanoseconds(PICKUP_MS))
        {
            return false;
        }
        active.pickup = false;
        true
    }

    /// When the client should next draw a frame for a hold or a pickup that comes due, once
    /// its timing has begun.
    #[must_use]
    pub fn due(&self) -> Option<u64> {
        let active = self.active?;
        let since = active.since?;
        let hold = active.hold.then(|| since + nanoseconds(LONG_PRESS_MS));
        let pickup = active.pickup.then(|| since + nanoseconds(PICKUP_MS));
        match (hold, pickup) {
            (Some(hold), Some(pickup)) => Some(hold.min(pickup)),
            (hold, pickup) => hold.or(pickup),
        }
    }

    /// Takes finger `id` off the screen. Returns whether it was the active finger, which then
    /// is no longer.
    fn lift(&mut self, id: u8) -> bool {
        if self.fingers[usize::from(id)].take().is_some() {
            self.down -= 1;
        }
        let active = self.is_active(id);
        if active {
            self.active = None;
        }
        active
    }
}
