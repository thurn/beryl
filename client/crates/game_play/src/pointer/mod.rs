//! The pointer on the table: the card it hovers, the hand card that rises under it, and the card
//! it holds (docs/beryl-3d.md, "Motion the pointer drives").
//!
//! Where the pointer is, is state; when it moved is not. `Pointer::handle` stores its latest
//! position and phase. Each frame, `update` finds the card under it with a hit test on the frame
//! just built, because a card can move under a pointer that is still. When that card changes,
//! the hand card it left eases back down into the fan and the one it entered eases up out of it,
//! as the web interface's `RaisedCopy` (`ui/src/stage/raised.ts`) has them. How far up a card
//! is, from 0 in the fan to 1 raised, is a function of the time alone, so a frame drawn or
//! skipped between changes nothing.
//!
//! The input router chooses when a card is picked up and released. This module keeps the
//! held pose and its fall so a script can take over from where the player let it go.

mod held;

pub mod rows;

use beryl_protocol::protocol::{Side, ZoneName};
use glam::{Mat3, Quat, Vec2};

use crate::play::compile::Compiled;
use game_table::table::Place;
pub use game_table::table::takes_pointer;
use game_table::table::{Camera, Look, PILE_IDS};
use held::{Fall, Heft, Held};
use kit_base::data::pointer::PointerTuning;
use kit_base::pose::{Pose, Segment, between};
use kit_frame::frame::{ElementId, Frame, Lifted, Role, StateBits};
use kit_scene::hit::hit;
use kit_ui::input::{self, PointerKind, PointerPhase};
use kit_ui::layout::Shake;

/// The most hand cards that can be rising, raised or falling at once (`frame::LIFTED`).
const LIFTS: usize = kit_frame::frame::LIFTED;

/// Nanoseconds in a millisecond.
const MILLISECOND: f32 = 1_000_000.0;

/// What the pointer is doing on the table.
#[derive(Clone, Debug, PartialEq)]
pub struct Pointer {
    /// Where the mouse is, in logical points, while it is over the window. A touch never
    /// hovers, so a touch leaves this `None`.
    at: Option<Vec2>,
    /// The phase of the latest pointer event.
    phase: Option<PointerPhase>,
    /// The slot of the card under the pointer, as of the last frame.
    hovered: Option<usize>,
    /// The index (`PileShown::index`) of the pile under the pointer, as of the last frame.
    pile: Option<usize>,
    /// The hand cards out of their place in the fan: rising, raised or falling.
    lifts: Vec<Lift>,
    held: Option<Held>,
    heft: Heft,
    /// The card let go last, falling back to where it rests.
    fall: Option<Fall>,
    /// The card shaking because the manual table refused an action sent from it (`refuse`).
    shake: Option<(usize, Shake)>,
}

/// One card easing from `from` to `to` (0 at rest, 1 raised) since the time `since`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lift {
    slot: usize,
    from: f32,
    to: f32,
    since: u64,
    rise: Rise,
}

/// How a hovered card rises out of where it rests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rise {
    /// To the hand's inspection size (UI Conventions, "Hover inspection").
    Hand,
    /// A card in a prompt's tray, already large, turns upright and lifts a little in its place,
    /// as one hovered in Arena's "Order Cards" screen does.
    Tray,
}

impl Lift {
    /// How long the whole move takes: a full rise in `RISE_MS` and a full fall in `FALL_MS`,
    /// and a part of one in that part of the time, as `raisedAt` in raised.ts has it.
    fn length(&self, tuning: &PointerTuning) -> f32 {
        let full = if self.to > self.from {
            tuning.rise_ms
        } else {
            tuning.fall_ms
        };
        (self.to - self.from).abs() * full * MILLISECOND
    }

    /// How far up the card is at `now`, with the cubic ease out from web's `raisedAt`.
    fn at(&self, now: u64, tuning: &PointerTuning) -> f32 {
        let length = self.length(tuning);
        if length <= 0.0 {
            return self.to;
        }
        let elapsed = (now.saturating_sub(self.since) as f32 / length).min(1.0);
        let eased = 1.0 - (1.0 - elapsed).powi(3);
        self.from + (self.to - self.from) * eased
    }

    /// Whether the move has arrived by `now`.
    fn done(&self, now: u64, tuning: &PointerTuning) -> bool {
        now.saturating_sub(self.since) as f32 >= self.length(tuning)
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            at: None,
            phase: None,
            hovered: None,
            pile: None,
            lifts: Vec::with_capacity(LIFTS),
            ..Self::EMPTY
        }
    }
}

impl Pointer {
    /// A pointer that holds no room for lifts, which `update` swaps in while it borrows the
    /// show. Allocates nothing.
    pub(crate) const EMPTY: Self = Self {
        at: None,
        phase: None,
        hovered: None,
        pile: None,
        lifts: Vec::new(),
        held: None,
        heft: Heft::FLAT,
        fall: None,
        shake: None,
    };

    /// Records the pointer for hover and inspector motion.
    pub fn observe(&mut self, pointer: &input::Pointer) {
        self.phase = Some(pointer.phase);
        if pointer.kind == PointerKind::Mouse {
            self.at = match pointer.phase {
                PointerPhase::Cancel | PointerPhase::Leave => None,
                _ => Some(pointer.position),
            };
        }
    }

    /// Draws a carried card under the pointer.
    pub(crate) fn carry(&mut self, slot: usize, at: Vec2, hand_height: Option<f32>) {
        if let Some(held) = self.held.as_mut().filter(|held| held.slot == slot) {
            held.at = at;
        } else {
            self.heft = Heft::FLAT;
            self.held = Some(Held {
                slot,
                at,
                hand_height,
            });
        }
    }

    /// Starts the card's fall from the point where the gesture ended.
    pub fn release(
        &mut self,
        at: Option<Vec2>,
        camera: &Camera,
        tuning: &PointerTuning,
    ) -> Option<usize> {
        let mut held = self.held.take()?;
        if let Some(at) = at {
            held.at = at;
        }
        self.fall = Some(Fall {
            slot: held.slot,
            from: held.pose(camera, &self.heft, tuning),
            since: None,
            awaiting_play: false,
        });
        Some(held.slot)
    }

    /// Holds a just released card in place while its play is with the session.
    pub(crate) fn await_play(&mut self, slot: usize) {
        if let Some(fall) = self.fall.as_mut().filter(|fall| fall.slot == slot) {
            fall.awaiting_play = true;
        }
    }

    /// Starts the return to the fan if the session did not play the released card.
    pub(crate) fn resume_fall(&mut self, slot: usize) {
        if let Some(fall) = self.fall.as_mut().filter(|fall| fall.slot == slot) {
            fall.awaiting_play = false;
        }
    }

    /// Moves the pointer's poses to the slots that will draw them after two cards trade places.
    pub(crate) fn trade(&mut self, first: usize, second: usize) {
        let trade = |slot| match slot {
            slot if slot == first => second,
            slot if slot == second => first,
            slot => slot,
        };
        self.hovered = self.hovered.map(trade);
        for lift in &mut self.lifts {
            lift.slot = trade(lift.slot);
        }
        if let Some(fall) = &mut self.fall {
            fall.slot = trade(fall.slot);
        }
    }

    /// The slot of the card being carried.
    #[must_use]
    pub fn held(&self) -> Option<usize> {
        self.held.map(|held| held.slot)
    }

    /// Whether the pointer holds a card, or one it let go is still falling back at `now`: a
    /// script that starts then may take the card over from where it is (`hand_over`).
    pub(crate) fn carries(&self, now: u64, fall_ms: f32) -> bool {
        self.held().is_some() || self.fall.is_some_and(|fall| !fall.done(now, fall_ms))
    }

    /// The held card's lean, in radians: toward +x on the screen and toward +y (down it). It is
    /// zero while no card is held.
    #[must_use]
    pub fn lean(&self) -> Vec2 {
        if self.held().is_some() {
            self.heft.tilt()
        } else {
            Vec2::ZERO
        }
    }

    /// Advances the held card's lean to the frame at `now`, starts a shake asked for since the
    /// last frame and forgets one that is over. The client calls this before it builds each
    /// frame.
    pub fn advance(&mut self, now: u64, tuning: &PointerTuning) {
        if let Some(fall) = &mut self.fall
            && !fall.awaiting_play
        {
            fall.since.get_or_insert(now);
        }
        if let Some(held) = self.held {
            self.heft.advance(held, now, tuning);
        }
        if let Some((_, shake)) = &mut self.shake
            && !shake.advance(now)
        {
            self.shake = None;
        }
    }

    /// Shakes the card in `slot`, from the next frame, because the manual table refused the
    /// action a click on it sent. A shake under way on another card stops.
    pub fn refuse(&mut self, slot: usize, refusal_ms: f32) {
        self.shake = Some((slot, Shake::new(refusal_ms)));
    }

    /// How far across the screen the card in `slot` is shaken at `now`, in logical points.
    #[must_use]
    pub fn shaken(&self, slot: usize, now: u64) -> f32 {
        self.shake
            .filter(|(shaking, _)| *shaking == slot)
            .map_or(0.0, |(_, shake)| shake.offset(now))
    }

    /// Where the card in `slot` is drawn at `now` by `camera` because of the pointer, if it is
    /// held or falling back from where it was let go toward `rest`.
    #[must_use]
    pub fn carried(
        &self,
        slot: usize,
        rest: &Pose,
        (now, camera): (u64, &Camera),
        tuning: &PointerTuning,
    ) -> Option<Pose> {
        if let Some(held) = self.held.filter(|held| held.slot == slot) {
            return Some(held.pose(camera, &self.heft, tuning));
        }
        let fall = self
            .fall
            .filter(|fall| fall.slot == slot && !fall.done(now, tuning.fall_ms))?;
        Some(fall.at(rest, now, tuning.fall_ms))
    }

    /// Where the mouse is, in logical points, while it is over the window.
    #[must_use]
    pub fn position(&self) -> Option<Vec2> {
        self.at
    }

    /// The phase of the latest pointer event.
    #[must_use]
    pub fn phase(&self) -> Option<PointerPhase> {
        self.phase
    }

    /// The slot of the card under the pointer, as of the last frame.
    #[must_use]
    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    /// The index (`PileShown::index`) of the pile under the pointer, as of the last frame.
    #[must_use]
    pub fn hovered_pile(&self) -> Option<usize> {
        self.pile
    }

    /// How far up the hand card in `slot` is at `now`, from 0 in the fan to 1 raised.
    #[must_use]
    pub fn lift(&self, slot: usize, now: u64, tuning: &PointerTuning) -> f32 {
        self.lifts
            .iter()
            .find(|lift| lift.slot == slot)
            .map_or(0.0, |lift| lift.at(now, tuning))
    }

    /// Whether a hand card is rising or falling at `now`, a card is held, one let go is
    /// falling back, or a refused card is shaking.
    #[must_use]
    pub fn moving(&self, now: u64, tuning: &PointerTuning) -> bool {
        self.lifts.iter().any(|lift| !lift.done(now, tuning))
            || self.held().is_some()
            || self
                .fall
                .is_some_and(|fall| !fall.awaiting_play && !fall.done(now, tuning.fall_ms))
            || self.shake.is_some_and(|(_, shake)| !shake.done(now))
    }

    /// Starts the card in `slot` easing toward `to` at `now`, from where it is then. A card
    /// already out of its place keeps the way it rises; `rise` is for one that is not.
    fn ease(&mut self, slot: usize, to: f32, rise: Rise, now: u64, tuning: &PointerTuning) {
        let from = self.lift(slot, now, tuning);
        let mut lift = Lift {
            slot,
            from,
            to,
            since: now,
            rise,
        };
        if let Some(held) = self.lifts.iter_mut().find(|lift| lift.slot == slot) {
            if from > 0.0 {
                lift.rise = held.rise;
            }
            *held = lift;
            return;
        }
        if self.lifts.len() == LIFTS {
            self.lifts.remove(0);
        }
        self.lifts.push(lift);
    }

    /// Makes the card in `slot` the hovered one at `now`. The card hovered before falls back to
    /// where it rests; the new one rises as `rise` says, if it rises at all.
    fn hover(&mut self, slot: Option<usize>, rise: Option<Rise>, now: u64, tuning: &PointerTuning) {
        if slot == self.hovered {
            return;
        }
        if let Some(left) = self.hovered.take() {
            self.ease(left, 0.0, Rise::Hand, now, tuning);
        }
        self.hovered = slot;
        if let Some(slot) = slot
            && let Some(rise) = rise
        {
            self.ease(slot, 1.0, rise, now, tuning);
        }
    }
}

/// Changes `look`, how the card in `slot` is drawn at `now` by `camera`, for what `pointer` is
/// doing with it: a hand card that is up out of the fan is drawn that far toward its raised
/// pose, and a card shaking because the manual table refused its action is drawn that far
/// across the screen. A card a script is moving is drawn where the script has it. The card
/// under the pointer is marked `HOVERED` by `update`, once the frame's hit test has found it.
pub fn look(
    pointer: &Pointer,
    slot: usize,
    (now, camera): (u64, &Camera),
    tuning: &PointerTuning,
    look: &mut Look,
) {
    if look.state & StateBits::MOVING != 0 {
        return;
    }
    if let Some(pose) = pointer.carried(slot, &look.pose, (now, camera), tuning) {
        look.state |= if pointer.held() == Some(slot) {
            StateBits::HELD
        } else {
            StateBits::MOVING
        };
        look.pose = pose;
        return;
    }
    let lift = pointer.lift(slot, now, tuning);
    if lift > 0.0 {
        let rise = pointer.lifts.iter().find(|lift| lift.slot == slot);
        let (scale, mm) = match rise.map(|lift| lift.rise) {
            Some(Rise::Tray) => (tuning.tray_raise_scale, tuning.tray_raise_mm),
            _ => (tuning.raise_scale, tuning.raise_mm),
        };
        let raised = raised(&look.pose, camera, (scale, mm), tuning);
        // The raised pose is closer to the camera. Weight the interpolation by the two
        // projection divisors so the ease describes progress on screen, as on the web.
        let divisor = |pose: &Pose| (camera.view_projection * pose.position.extend(1.0)).w;
        let lifted = lift * divisor(&look.pose);
        let resting = (1.0 - lift) * divisor(&raised);
        look.pose = between(&look.pose, &raised, lifted / (lifted + resting));
        // The web client's raised copy paints over the life orb and the step icons until it lands.
        if look.interactive {
            look.state |= StateBits::RAISED;
        }
    }
    let shaken = pointer.shaken(slot, now);
    let points_per_millimetre = camera.points_per_millimetre(look.pose.position);
    if shaken != 0.0 && points_per_millimetre > 0.0 {
        let [right, _, _] = camera.axes();
        look.pose.position += right * (shaken / points_per_millimetre);
    }
}

/// Where a card resting at `rest` is drawn when it is all the way up: turned upright, `scale`
/// times as large about its foot, and `mm` of its own millimetres higher up the screen. It is
/// then moved toward the eye, to `NEARER` of its distance and smaller by as much, which leaves
/// it where it was on the screen but in front of every other card beside it.
fn raised(rest: &Pose, camera: &Camera, (scale, mm): (f32, f32), tuning: &PointerTuning) -> Pose {
    let [right, up, back] = camera.axes();
    let grown = (scale - 1.0) * camera.data.card.height / 2.0;
    let position = rest.position + up * (mm + grown) * rest.scale;
    let mut pose = kit_space::arrange::spot(
        camera.eye + (position - camera.eye) * tuning.nearer,
        Quat::from_mat3(&Mat3::from_cols(right, up, back)),
        rest.scale * scale * tuning.nearer,
        *rest,
    );
    let (corners, _) = camera.card_box(&pose);
    let left = corners
        .iter()
        .fold(f32::INFINITY, |left, corner| left.min(corner.x));
    let right_edge = corners
        .iter()
        .fold(f32::NEG_INFINITY, |right, corner| right.max(corner.x));
    let safe_left = camera.window.safe_area.left;
    let safe_right = camera.window.size.width - camera.window.safe_area.right;
    let shift = if right_edge - left > safe_right - safe_left {
        ((safe_left + safe_right) - (left + right_edge)) * 0.5
    } else if left < safe_left {
        safe_left - left
    } else if right_edge > safe_right {
        safe_right - right_edge
    } else {
        0.0
    };
    let points_per_millimetre = camera.points_per_millimetre(pose.position);
    if shift != 0.0 && points_per_millimetre > 0.0 {
        pose.position += right * (shift / points_per_millimetre);
    }
    pose
}

/// Finds the card under the pointer in `frame`, the frame just built at `now`, and marks it
/// `HOVERED`. As the web interface's hand does, the hit test finds hand cards by their resting
/// places in the fan, which `rest` gives as each card's pose by its slot, so that a
/// pointer over the part of the fan a raised card has left still finds that card. An element of
/// the flat layer under the pointer is marked `HOVERED` instead. A card a script is moving or one
/// behind a modal shade (`!hoverable(slot)`) is not hovered, and nothing is hovered while a card
/// is held. `in_tray` says whether the card in a slot is one a prompt holds up in its tray, which
/// rises less.
pub(crate) fn update(
    pointer: &mut Pointer,
    frame: &mut Frame,
    (rest, in_tray, hoverable): (
        impl Fn(usize) -> Option<Pose>,
        impl Fn(usize) -> bool,
        impl Fn(usize) -> bool,
    ),
    (now, camera): (u64, &Camera),
    tuning: &PointerTuning,
) {
    pointer
        .lifts
        .retain(|lift| lift.to > 0.0 || !lift.done(now, tuning));
    hover_under(
        pointer,
        frame,
        (rest, in_tray, hoverable),
        (now, camera),
        tuning,
    );
}

/// Hands the card the pointer holds or let go over to `compiled`, a script that starts at
/// `now`: if the script moves the card, it starts from where the pointer has the card then,
/// instead of from where the card rests (`rest`), and the pointer lets the card go. Each of the
/// card's first segments that starts from its resting place starts from there instead, and one
/// that holds it there holds it where the pointer had it, so that the card waits where it was
/// let go until the script carries it on, as the web interface's taken drop does. Returns
/// whether it did, so that the script's record is written again.
pub(crate) fn hand_over(
    pointer: &mut Pointer,
    compiled: &mut Compiled,
    rest: impl Fn(usize) -> Option<Pose>,
    (now, camera): (u64, &Camera),
    tuning: &PointerTuning,
) -> bool {
    let Some(slot) = pointer.held().or(pointer
        .fall
        .filter(|fall| !fall.done(now, tuning.fall_ms))
        .map(|fall| fall.slot))
    else {
        return false;
    };
    let Some(track) = compiled.tracks.get_mut(slot) else {
        return false;
    };
    let Some(resting) = track.segments().first().map(|first| first.from) else {
        return false;
    };
    let Some(from) =
        rest(slot).and_then(|pose| pointer.carried(slot, &pose, (now, camera), tuning))
    else {
        return false;
    };
    let moved = |segment: &mut Segment| {
        if segment.from != resting {
            return false;
        }
        segment.start_at(from);
        let holds = segment.to == resting;
        if holds {
            segment.to = from;
        }
        holds
    };
    for segment in track.segments_mut() {
        if !moved(segment) {
            break;
        }
    }
    for record in compiled
        .segments
        .iter_mut()
        .filter(|record| record.slot == slot)
    {
        if !moved(&mut record.segment) {
            break;
        }
    }
    pointer.fall = None;
    if pointer.held() == Some(slot) {
        pointer.held = None;
    }
    true
}

/// Finds the card under the pointer in `frame` and marks it `HOVERED`, as `update` says, with
/// `rest` giving each card's resting pose by its slot.
fn hover_under(
    pointer: &mut Pointer,
    frame: &mut Frame,
    (rest, in_tray, hoverable): (
        impl Fn(usize) -> Option<Pose>,
        impl Fn(usize) -> bool,
        impl Fn(usize) -> bool,
    ),
    (now, camera): (u64, &Camera),
    tuning: &PointerTuning,
) {
    for lift in &pointer.lifts {
        let (Some(index), Some(pose)) = (card(frame, lift.slot), rest(lift.slot)) else {
            continue;
        };
        let (corners, depth) = camera.card_box(&pose);
        frame.lifted.push(Lifted {
            element: u32::try_from(index).expect("a frame holds far fewer than 2^32 elements"),
            corners,
            depth,
            pose: Some(pose),
            raised: false,
        });
    }
    pointer.pile = None;
    let Some(point) = pointer.at.filter(|_| pointer.held().is_none()) else {
        pointer.hover(None, None, now, tuning);
        return;
    };
    frame.swap_lifted();
    let reached = hit(frame, point).map(|index| index as usize);
    frame.swap_lifted();
    let Some(index) = reached else {
        pointer.hover(None, None, now, tuning);
        return;
    };
    let element = &mut frame.resolved[index];
    if element.role != Role::Card {
        element.state.0 |= StateBits::HOVERED;
        if element.role == Role::Pile {
            pointer.pile = element
                .id
                .0
                .checked_sub(PILE_IDS)
                .map(|index| index as usize);
        }
        pointer.hover(None, None, now, tuning);
        return;
    }
    let slot = element.id.0 as usize;
    if !hoverable(slot) || element.state.has(StateBits::MOVING) {
        if element.state.has(StateBits::MOVING) {
            // The script takes the card from where it lay in the fan, so it is no longer up.
            pointer.lifts.retain(|lift| lift.slot != slot);
        }
        pointer.hover(None, None, now, tuning);
        return;
    }
    element.state.0 |= StateBits::HOVERED;
    let rise = if in_tray(slot) {
        Some(Rise::Tray)
    } else {
        matches!(
            Place::in_frame(element.place),
            Some(Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                ..
            })
        )
        .then_some(Rise::Hand)
    };
    pointer.hover(Some(slot), rise, now, tuning);
}

/// The index in `frame` of the card element of the object in `slot`.
fn card(frame: &Frame, slot: usize) -> Option<usize> {
    let id = ElementId(u32::try_from(slot).ok()?);
    frame
        .resolved
        .iter()
        .position(|element| element.role == Role::Card && element.id == id)
}
