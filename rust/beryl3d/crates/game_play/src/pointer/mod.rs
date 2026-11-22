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
pub use game_table::table::takes_pointer;
use game_table::table::{Camera, Look, PILE_IDS};
use held::{Fall, Lean};
use kit_base::data::pointer::PointerTuning;
use kit_base::pose::between;
use kit_base::pose::{Curve, Pose, Segment};
use kit_frame::frame::{ElementId, Frame, Lifted, Place, Role, StateBits};
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
    /// The slot and position of the card carried under the pointer.
    held: Option<(usize, Vec2)>,
    lean: Lean,
    /// The card let go last, falling back to where it rests.
    fall: Option<Fall>,
    /// The card shaking because the manual table refused an action sent from it (`refuse`).
    shake: Option<(usize, Shake)>,
}

/// One hand card easing from `from` to `to` (0 in the fan, 1 raised) since the time `since`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lift {
    slot: usize,
    from: f32,
    to: f32,
    since: u64,
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

    /// How far up the card is at `now`, easing out toward `to`.
    fn at(&self, now: u64, tuning: &PointerTuning) -> f32 {
        let length = self.length(tuning);
        if length <= 0.0 {
            return self.to;
        }
        let share = now.saturating_sub(self.since) as f32 / length;
        self.from + (self.to - self.from) * Curve::EASE_OUT.ease(share)
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
        lean: Lean::FLAT,
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
    pub fn carry(&mut self, slot: usize, at: Vec2) {
        if self.held() != Some(slot) {
            self.lean = Lean::FLAT;
        }
        self.held = Some((slot, at));
    }

    /// Starts the card's fall from the point where the gesture ended.
    pub fn release(
        &mut self,
        at: Option<Vec2>,
        camera: &Camera,
        tuning: &PointerTuning,
    ) -> Option<usize> {
        let (slot, was) = self.held.take()?;
        self.fall = Some(Fall {
            slot,
            from: held::pose(
                camera,
                at.unwrap_or(was),
                self.lean.tilt(),
                tuning.held_height_mm,
            ),
            since: None,
        });
        Some(slot)
    }

    /// The slot of the card being carried.
    #[must_use]
    pub fn held(&self) -> Option<usize> {
        self.held.map(|(slot, _)| slot)
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
            self.lean.tilt()
        } else {
            Vec2::ZERO
        }
    }

    /// Advances the held card's lean to the frame at `now`, starts a shake asked for since the
    /// last frame and forgets one that is over. The client calls this before it builds each
    /// frame.
    pub fn advance(&mut self, now: u64, tuning: &PointerTuning) {
        if let Some(fall) = &mut self.fall {
            fall.since.get_or_insert(now);
        }
        if let Some((_, at)) = self.held {
            self.lean.advance(at, now, tuning);
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
        if let Some((_, at)) = self.held.filter(|(held, _)| *held == slot) {
            return Some(held::pose(
                camera,
                at,
                self.lean.tilt(),
                tuning.held_height_mm,
            ));
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
                .is_some_and(|fall| !fall.done(now, tuning.fall_ms))
            || self.shake.is_some_and(|(_, shake)| !shake.done(now))
    }

    /// Starts the hand card in `slot` easing toward `to` at `now`, from where it is then.
    fn ease(&mut self, slot: usize, to: f32, now: u64, tuning: &PointerTuning) {
        let from = self.lift(slot, now, tuning);
        let lift = Lift {
            slot,
            from,
            to,
            since: now,
        };
        if let Some(held) = self.lifts.iter_mut().find(|lift| lift.slot == slot) {
            *held = lift;
            return;
        }
        if self.lifts.len() == LIFTS {
            self.lifts.remove(0);
        }
        self.lifts.push(lift);
    }

    /// Makes the card in `slot` the hovered one at `now`. The hand card hovered before falls
    /// back into the fan; the new one rises out of it if it is a hand card (`rises`).
    fn hover(&mut self, slot: Option<usize>, rises: bool, now: u64, tuning: &PointerTuning) {
        if slot == self.hovered {
            return;
        }
        if let Some(left) = self.hovered.take() {
            self.ease(left, 0.0, now, tuning);
        }
        self.hovered = slot;
        if let Some(slot) = slot
            && rises
        {
            self.ease(slot, 1.0, now, tuning);
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
        look.pose = between(&look.pose, &raised(&look.pose, camera, tuning), lift);
    }
    let shaken = pointer.shaken(slot, now);
    let points_per_millimetre = camera.points_per_millimetre(look.pose.position);
    if shaken != 0.0 && points_per_millimetre > 0.0 {
        let [right, _, _] = camera.axes();
        look.pose.position += right * (shaken / points_per_millimetre);
    }
}

/// Where a hand card resting at `rest` is drawn when it is all the way up: turned upright,
/// `RAISE_SCALE` times as large about its foot, and `RAISE_MM` of its own millimetres higher up
/// the screen. It is then moved toward the eye, to `NEARER` of its distance and smaller by as
/// much, which leaves it where it was on the screen but in front of every other card of the
/// hand.
fn raised(rest: &Pose, camera: &Camera, tuning: &PointerTuning) -> Pose {
    let [right, up, back] = camera.axes();
    let grown = (tuning.raise_scale - 1.0) * camera.data.card.height / 2.0;
    let position = rest.position + up * (tuning.raise_mm + grown) * rest.scale;
    kit_space::arrange::spot(
        camera.eye + (position - camera.eye) * tuning.nearer,
        Quat::from_mat3(&Mat3::from_cols(right, up, back)),
        rest.scale * tuning.raise_scale * tuning.nearer,
        *rest,
    )
}

/// Finds the card under the pointer in `frame`, the frame just built at `now`, and marks it
/// `HOVERED`. As the web interface's hand does, the hit test finds hand cards by their resting
/// places in the fan, which `rest` gives as each card's pose by its slot, so that a
/// pointer over the part of the fan a raised card has left still finds that card. An element of
/// the flat layer under the pointer is marked `HOVERED` instead. A card a script is moving is
/// not hovered until it arrives, and nothing is hovered while a card is held.
pub(crate) fn update(
    pointer: &mut Pointer,
    frame: &mut Frame,
    rest: impl Fn(usize) -> Option<Pose>,
    (now, camera): (u64, &Camera),
    tuning: &PointerTuning,
) {
    pointer
        .lifts
        .retain(|lift| lift.to > 0.0 || !lift.done(now, tuning));
    hover_under(pointer, frame, rest, (now, camera), tuning);
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
        segment.from = from;
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
    rest: impl Fn(usize) -> Option<Pose>,
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
        });
    }
    pointer.pile = None;
    let Some(point) = pointer.at.filter(|_| pointer.held().is_none()) else {
        pointer.hover(None, false, now, tuning);
        return;
    };
    frame.swap_lifted();
    let reached = hit(frame, point).map(|index| index as usize);
    frame.swap_lifted();
    let Some(index) = reached else {
        pointer.hover(None, false, now, tuning);
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
        pointer.hover(None, false, now, tuning);
        return;
    }
    if element.state.has(StateBits::MOVING) {
        // The script takes the card from where it lay in the fan, so it is no longer up.
        let slot = element.id.0 as usize;
        pointer.lifts.retain(|lift| lift.slot != slot);
        pointer.hover(None, false, now, tuning);
        return;
    }
    element.state.0 |= StateBits::HOVERED;
    let rises = matches!(
        element.place,
        Place::Card {
            zone: ZoneName::Hand,
            side: Side::You,
            ..
        }
    );
    pointer.hover(Some(element.id.0 as usize), rises, now, tuning);
}

/// The index in `frame` of the card element of the object in `slot`.
fn card(frame: &Frame, slot: usize) -> Option<usize> {
    let id = ElementId(u32::try_from(slot).ok()?);
    frame
        .resolved
        .iter()
        .position(|element| element.role == Role::Card && element.id == id)
}
