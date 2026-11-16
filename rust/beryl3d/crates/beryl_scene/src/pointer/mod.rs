//! The pointer on the table: the card it hovers, and the hand card that rises under it
//! (docs/beryl-3d.md, "Motion the pointer drives").
//!
//! Where the pointer is, is state; when it moved is not. `Pointer::handle` stores its latest
//! position and phase. Each frame, `update` finds the card under it with a hit test on the frame
//! just built, because a card can move under a pointer that is still. When that card changes,
//! the hand card it left eases back down into the fan and the one it entered eases up out of it,
//! as the web interface's `RaisedCopy` (`ui/src/stage/raised.ts`) has them. How far up a card
//! is, from 0 in the fan to 1 raised, is a function of the time alone, so a frame drawn or
//! skipped between changes nothing.

use beryl_protocol::protocol::{Side, ZoneName};
use glam::{Mat3, Quat, Vec2};

use crate::agent::hit;
use crate::data::space::pointer as data;
use crate::frame::{ElementId, Frame, Place, Role, StateBits};
use crate::input::{self, PointerKind, PointerPhase};
use crate::motion::between;
use crate::pose::{Curve, Pose};
use crate::table::{CARD_HEIGHT, Camera, Look};

/// The most hand cards that can be rising, raised or falling at once. One is raised at a time,
/// and a pointer swept across a hand leaves each card it crossed falling for `FALL_MS`; past
/// this many, the card that started falling first lands at once.
const LIFTS: usize = 32;

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
    /// The hand cards out of their place in the fan: rising, raised or falling.
    lifts: Vec<Lift>,
    /// The corners and depth each raised card's element had before `update` put its resting box
    /// in their place for the hit test, by the element's index. Kept here so that a frame
    /// allocates nothing.
    saved: Vec<(usize, [Vec2; 4], f32)>,
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
    fn length(&self) -> f32 {
        let full = if self.to > self.from {
            data::RISE_MS
        } else {
            data::FALL_MS
        };
        (self.to - self.from).abs() * full * MILLISECOND
    }

    /// How far up the card is at `now`, easing out toward `to`.
    fn at(&self, now: u64) -> f32 {
        let length = self.length();
        if length <= 0.0 {
            return self.to;
        }
        let share = now.saturating_sub(self.since) as f32 / length;
        self.from + (self.to - self.from) * Curve::EASE_OUT.ease(share)
    }

    /// Whether the move has arrived by `now`.
    fn done(&self, now: u64) -> bool {
        now.saturating_sub(self.since) as f32 >= self.length()
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            at: None,
            phase: None,
            hovered: None,
            lifts: Vec::with_capacity(LIFTS),
            saved: Vec::with_capacity(LIFTS),
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
        lifts: Vec::new(),
        saved: Vec::new(),
    };

    /// Takes a pointer event: where the mouse is, or that it has left the window.
    pub fn handle(&mut self, pointer: &input::Pointer) {
        self.phase = Some(pointer.phase);
        if pointer.kind != PointerKind::Mouse {
            return;
        }
        self.at = match pointer.phase {
            PointerPhase::Cancel | PointerPhase::Leave => None,
            _ => Some(pointer.position),
        };
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

    /// How far up the hand card in `slot` is at `now`, from 0 in the fan to 1 raised.
    #[must_use]
    pub fn lift(&self, slot: usize, now: u64) -> f32 {
        self.lifts
            .iter()
            .find(|lift| lift.slot == slot)
            .map_or(0.0, |lift| lift.at(now))
    }

    /// Whether a hand card is rising or falling at `now`.
    #[must_use]
    pub fn moving(&self, now: u64) -> bool {
        self.lifts.iter().any(|lift| !lift.done(now))
    }

    /// Starts the hand card in `slot` easing toward `to` at `now`, from where it is then.
    fn ease(&mut self, slot: usize, to: f32, now: u64) {
        let from = self.lift(slot, now);
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
    fn hover(&mut self, slot: Option<usize>, rises: bool, now: u64) {
        if slot == self.hovered {
            return;
        }
        if let Some(left) = self.hovered.take() {
            self.ease(left, 0.0, now);
        }
        self.hovered = slot;
        if let Some(slot) = slot
            && rises
        {
            self.ease(slot, 1.0, now);
        }
    }
}

/// Changes `look`, how the card in `slot` is drawn at `now` by `camera`, for what `pointer` is
/// doing with it: a hand card that is up out of the fan is drawn that far toward its raised
/// pose. A card a script is moving is drawn where the script has it. The card under the pointer
/// is marked `HOVERED` by `update`, once the frame's hit test has found it.
pub fn look(pointer: &Pointer, slot: usize, (now, camera): (u64, &Camera), look: &mut Look) {
    if look.state & StateBits::MOVING != 0 {
        return;
    }
    let lift = pointer.lift(slot, now);
    if lift > 0.0 {
        look.pose = between(&look.pose, &raised(&look.pose, camera), lift);
    }
}

/// Where a hand card resting at `rest` is drawn when it is all the way up: turned upright,
/// `RAISE_SCALE` times as large about its foot, and `RAISE_MM` of its own millimetres higher up
/// the screen. It is then moved toward the eye, to `NEARER` of its distance and smaller by as
/// much, which leaves it where it was on the screen but in front of every other card of the
/// hand.
fn raised(rest: &Pose, camera: &Camera) -> Pose {
    let [right, up, back] = camera.axes();
    let grown = (data::RAISE_SCALE - 1.0) * CARD_HEIGHT / 2.0;
    let position = rest.position + up * (data::RAISE_MM + grown) * rest.scale;
    Pose {
        position: camera.eye + (position - camera.eye) * data::NEARER,
        rotation: Quat::from_mat3(&Mat3::from_cols(right, up, back)),
        scale: rest.scale * data::RAISE_SCALE * data::NEARER,
        ..*rest
    }
}

/// Whether a card resting at `place` takes the pointer, once it is face up and the viewer knows
/// its name: one on the battlefield, on the stack or in the viewer's hand. A pile is one
/// element, so no card in it does. The table leaves out a face-up card with no name, such as a
/// copy of a face-down creature, because every interactive element has a name.
#[must_use]
pub fn takes_pointer(place: Place) -> bool {
    matches!(
        place,
        Place::Card {
            zone: ZoneName::Battlefield | ZoneName::Stack,
            ..
        } | Place::Card {
            zone: ZoneName::Hand,
            side: Side::You,
            ..
        }
    )
}

/// Finds the card under the pointer in `frame`, the frame just built at `now`, and marks it
/// `HOVERED`. As the web interface's hand does, the hit test finds hand cards by their resting
/// places in the fan, which `rest` gives as each card's corners and depth by its slot, so that a
/// pointer over the part of the fan a raised card has left still finds that card. An element of
/// the flat layer under the pointer is marked `HOVERED` instead. A card a script is moving is
/// not hovered until it arrives.
pub(crate) fn update(
    pointer: &mut Pointer,
    frame: &mut Frame,
    rest: impl Fn(usize) -> Option<([Vec2; 4], f32)>,
    now: u64,
) {
    pointer
        .lifts
        .retain(|lift| lift.to > 0.0 || !lift.done(now));
    let Some(point) = pointer.at else {
        pointer.hover(None, false, now);
        return;
    };
    pointer.saved.clear();
    for lift in &pointer.lifts {
        let (Some(index), Some((corners, depth))) = (card(frame, lift.slot), rest(lift.slot))
        else {
            continue;
        };
        let element = &mut frame.resolved[index];
        pointer.saved.push((index, element.corners, element.depth));
        element.corners = corners;
        element.depth = depth;
    }
    let reached = hit(frame, point).map(|index| index as usize);
    for &(index, corners, depth) in &pointer.saved {
        frame.resolved[index].corners = corners;
        frame.resolved[index].depth = depth;
    }
    let Some(index) = reached else {
        pointer.hover(None, false, now);
        return;
    };
    let element = &mut frame.resolved[index];
    if element.role != Role::Card {
        element.state.0 |= StateBits::HOVERED;
        pointer.hover(None, false, now);
        return;
    }
    if element.state.has(StateBits::MOVING) {
        // The script takes the card from where it lay in the fan, so it is no longer up.
        let slot = element.id.0 as usize;
        pointer.lifts.retain(|lift| lift.slot != slot);
        pointer.hover(None, false, now);
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
    pointer.hover(Some(element.id.0 as usize), rises, now);
}

/// The index in `frame` of the card element of the object in `slot`.
fn card(frame: &Frame, slot: usize) -> Option<usize> {
    let id = ElementId(u32::try_from(slot).ok()?);
    frame
        .resolved
        .iter()
        .position(|element| element.role == Role::Card && element.id == id)
}
