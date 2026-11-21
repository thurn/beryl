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
//! A press on a card the open prompt lets the player pick up holds it once it has travelled
//! `DRAG_THRESHOLD_POINTS`, and the card then follows the pointer (`held`). Letting it go is
//! decided at the next frame, which `update` reports to the client as the card clicked, if the
//! gesture is one.

mod held;
pub use held::Hold;
pub(crate) mod refusal;

pub mod rows;
pub mod touch;

use beryl_protocol::protocol::{Side, ZoneName};
use glam::{Mat3, Quat, Vec2};

use crate::agent::hit;
use crate::play::compile::Compiled;
pub use crate::table::takes_pointer;
use crate::table::{CARD_HEIGHT, Camera, Look, PILE_IDS};
use held::{By, Fall, Grip, Lean};
use kit_base::data::space::pointer as data;
use kit_base::pose::between;
use kit_base::pose::{Curve, Pose, Segment};
use kit_frame::frame::{ElementId, Frame, Lifted, Place, Role, StateBits};
use kit_ui::input::{self, PointerKind, PointerPhase};
use refusal::Shake;

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
    /// The press on a card, from the button going down until it comes up.
    grip: Option<Grip>,
    /// The lean of the card held, since it was picked up.
    lean: Lean,
    /// A press on a card that ended since the last frame, which the next `update` decides the
    /// meaning of: the grip as it was let go, and whether it was cancelled.
    let_go: Option<(Grip, bool)>,
    /// The card let go last, falling back to where it rests.
    fall: Option<Fall>,
    /// Whether a held card let go anywhere is dropped where it was let go (`LetGo::Dropped`),
    /// as on the manual table, where the client finds the place it lands.
    drops_anywhere: bool,
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
        grip: None,
        lean: Lean::FLAT,
        let_go: None,
        fall: None,
        drops_anywhere: false,
        shake: None,
    };

    /// Makes a held card let go anywhere be dropped where it was let go (`LetGo::Dropped`), or
    /// clicked as on the game screen if not `anywhere`.
    pub fn drop_anywhere(&mut self, anywhere: bool) {
        self.drops_anywhere = anywhere;
    }

    /// Takes a pointer event: where the mouse is, or that it has left the window.
    pub fn handle(&mut self, pointer: &input::Pointer) {
        self.phase = Some(pointer.phase);
        match pointer.phase {
            PointerPhase::Move => {
                if let Some(grip) = &mut self.grip {
                    grip.moved(pointer.position);
                }
            }
            PointerPhase::Up => {
                if let Some(mut grip) = self.grip.take() {
                    grip.moved(pointer.position);
                    self.let_go = Some((grip, false));
                }
            }
            PointerPhase::Cancel | PointerPhase::Leave => self.cancel(),
            _ => {}
        }
        if pointer.kind != PointerKind::Mouse {
            return;
        }
        self.at = match pointer.phase {
            PointerPhase::Cancel | PointerPhase::Leave => None,
            _ => Some(pointer.position),
        };
    }

    /// Starts a press at `at` on the card in `slot`, which the pointer may pick up by the drags
    /// `hold` names. The client calls this as the button goes down over a card.
    pub fn press(&mut self, slot: usize, at: Vec2, hold: Hold) {
        self.grip = Some(Grip {
            slot,
            start: at,
            at,
            hold,
            moved: false,
            held: false,
            by: By::Mouse,
        });
        self.lean = Lean::FLAT;
    }

    /// Marks the press just started as a finger's, whose release without moving is a tap
    /// (`LetGo::Tap`). If `delayed`, it may not pick its card up until `ready`.
    pub fn by_touch(&mut self, delayed: bool) {
        if let Some(grip) = &mut self.grip {
            grip.by = By::Finger { ready: !delayed };
        }
    }

    /// Lets the press pick its card up from now on, if it has not moved yet.
    pub fn ready(&mut self) {
        if let Some(grip) = self.grip.as_mut().filter(|grip| !grip.moved) {
            grip.by = By::Finger { ready: true };
        }
    }

    /// Ends the press as a cancelled one: a card it holds falls back where it lay, and it
    /// clicks nothing. The client calls this when a touch held still opens a context menu.
    pub fn cancel(&mut self) {
        self.let_go = self.grip.take().map(|grip| (grip, true));
    }

    /// The slot of the card the pointer holds, if it holds one.
    #[must_use]
    pub fn held(&self) -> Option<usize> {
        self.grip.filter(|grip| grip.held).map(|grip| grip.slot)
    }

    /// Whether the pointer holds a card, or one it let go is still falling back at `now`: a
    /// script that starts then may take the card over from where it is (`hand_over`).
    pub(crate) fn carries(&self, now: u64) -> bool {
        self.held().is_some() || self.fall.is_some_and(|fall| !fall.done(now))
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
    pub fn advance(&mut self, now: u64) {
        if let Some(grip) = self.grip.filter(|grip| grip.held) {
            self.lean.advance(grip.at, now);
        }
        if let Some((_, shake)) = &mut self.shake
            && !shake.advance(now)
        {
            self.shake = None;
        }
    }

    /// Shakes the card in `slot`, from the next frame, because the manual table refused the
    /// action a click on it sent. A shake under way on another card stops.
    pub fn refuse(&mut self, slot: usize) {
        self.shake = Some((slot, Shake::new()));
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
    pub fn carried(&self, slot: usize, rest: &Pose, (now, camera): (u64, &Camera)) -> Option<Pose> {
        if let Some(grip) = self.grip.filter(|grip| grip.held && grip.slot == slot) {
            return Some(held::pose(camera, grip.at, self.lean.tilt()));
        }
        let fall = self
            .fall
            .filter(|fall| fall.slot == slot && !fall.done(now))?;
        Some(fall.at(rest, now))
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
    pub fn lift(&self, slot: usize, now: u64) -> f32 {
        self.lifts
            .iter()
            .find(|lift| lift.slot == slot)
            .map_or(0.0, |lift| lift.at(now))
    }

    /// Whether a hand card is rising or falling at `now`, a card is held, one let go is
    /// falling back, or a refused card is shaking.
    #[must_use]
    pub fn moving(&self, now: u64) -> bool {
        self.lifts.iter().any(|lift| !lift.done(now))
            || self.held().is_some()
            || self.fall.is_some_and(|fall| !fall.done(now))
            || self.shake.is_some_and(|(_, shake)| !shake.done(now))
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
/// pose, and a card shaking because the manual table refused its action is drawn that far
/// across the screen. A card a script is moving is drawn where the script has it. The card
/// under the pointer is marked `HOVERED` by `update`, once the frame's hit test has found it.
pub fn look(pointer: &Pointer, slot: usize, (now, camera): (u64, &Camera), look: &mut Look) {
    if look.state & StateBits::MOVING != 0 {
        return;
    }
    if let Some(pose) = pointer.carried(slot, &look.pose, (now, camera)) {
        look.state |= if pointer.held() == Some(slot) {
            StateBits::HELD
        } else {
            StateBits::MOVING
        };
        look.pose = pose;
        return;
    }
    let lift = pointer.lift(slot, now);
    if lift > 0.0 {
        look.pose = between(&look.pose, &raised(&look.pose, camera), lift);
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

/// Finds the card under the pointer in `frame`, the frame just built at `now`, and marks it
/// `HOVERED`. As the web interface's hand does, the hit test finds hand cards by their resting
/// places in the fan, which `rest` gives as each card's corners and depth by its slot, so that a
/// pointer over the part of the fan a raised card has left still finds that card. An element of
/// the flat layer under the pointer is marked `HOVERED` instead. A card a script is moving is
/// not hovered until it arrives, and nothing is hovered while a card is held.
///
/// A press on a card that ended since the last frame is decided here, and what it meant is
/// returned (`let_go`).
pub(crate) fn update(
    pointer: &mut Pointer,
    frame: &mut Frame,
    (rest, in_tray): (impl Fn(usize) -> Option<Pose>, impl Fn(usize) -> bool),
    (now, camera): (u64, &Camera),
) -> Option<LetGo> {
    pointer
        .lifts
        .retain(|lift| lift.to > 0.0 || !lift.done(now));
    let clicked = let_go(pointer, &rest, in_tray, (now, camera));
    hover_under(
        pointer,
        frame,
        |slot| rest(slot).map(|pose| camera.card_box(&pose)),
        now,
    );
    clicked
}

/// Decides what the press on a card that ended since the last frame means, at `now`, and
/// returns the card it clicked, as the web interface's `Pointer.release` and the prompt's
/// `#dropped` do. A press that never travelled `DRAG_THRESHOLD_POINTS` is a click on its card;
/// one that travelled that far without picking its card up, such as a drag that scrolled a
/// battlefield row, is not, as `Pointer.release` has it (`unheldDrag`). A card held and let go
/// higher on the screen than the top of where it rests, so over the battlefield or the stack, is
/// a click on it too, and one let go lower, back over the hand, is not. A card held up in a
/// tray (`in_tray`) and carried is let go where the pointer is, for the client to find the pile
/// it lands in (`LetGo::Dropped`), as the prompt's `#dropped` does for a tray. A held card let
/// go falls back from where it was to where it rests (`rest`), clicked or not, unless a script
/// moves it first. A cancelled press clicks nothing.
fn let_go(
    pointer: &mut Pointer,
    rest: impl Fn(usize) -> Option<Pose>,
    in_tray: impl Fn(usize) -> bool,
    (now, camera): (u64, &Camera),
) -> Option<LetGo> {
    let (grip, cancelled) = pointer.let_go.take()?;
    if !grip.held {
        let tap = if grip.by == By::Mouse {
            LetGo::Click(grip.slot)
        } else {
            LetGo::Tap(grip.slot)
        };
        return (!cancelled && !grip.moved).then_some(tap);
    }
    pointer.fall = Some(Fall {
        slot: grip.slot,
        from: held::pose(camera, grip.at, pointer.lean.tilt()),
        since: now,
    });
    if in_tray(grip.slot) || pointer.drops_anywhere {
        return (!cancelled).then_some(LetGo::Dropped {
            slot: grip.slot,
            at: grip.at,
        });
    }
    let resting = rest(grip.slot)?;
    let (corners, _) = camera.card_box(&resting);
    let top = corners
        .iter()
        .map(|corner| corner.y)
        .fold(f32::INFINITY, f32::min);
    (!cancelled && grip.at.y < top).then_some(LetGo::Click(grip.slot))
}

/// What a press on a card meant once it ended (`update`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LetGo {
    /// A click on the card in this slot.
    Click(usize),
    /// A finger's tap on the card in this slot: a click if the open prompt can use the card,
    /// and otherwise a request to show it whole.
    Tap(usize),
    /// The card in `slot`, held up in a tray, was carried and let go with the pointer at `at`,
    /// in logical points.
    Dropped { slot: usize, at: Vec2 },
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
) -> bool {
    let Some(slot) = pointer.held().or(pointer
        .fall
        .filter(|fall| !fall.done(now))
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
    let Some(from) = rest(slot).and_then(|pose| pointer.carried(slot, &pose, (now, camera))) else {
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
        pointer.grip = None;
    }
    true
}

/// Finds the card under the pointer in `frame` and marks it `HOVERED`, as `update` says, with
/// `rest` giving each card's resting corners and depth by its slot.
fn hover_under(
    pointer: &mut Pointer,
    frame: &mut Frame,
    rest: impl Fn(usize) -> Option<([Vec2; 4], f32)>,
    now: u64,
) {
    for lift in &pointer.lifts {
        let (Some(index), Some((corners, depth))) = (card(frame, lift.slot), rest(lift.slot))
        else {
            continue;
        };
        frame.lifted.push(Lifted {
            element: u32::try_from(index).expect("a frame holds far fewer than 2^32 elements"),
            corners,
            depth,
        });
    }
    pointer.pile = None;
    let Some(point) = pointer.at.filter(|_| pointer.held().is_none()) else {
        pointer.hover(None, false, now);
        return;
    };
    frame.swap_lifted();
    let reached = hit(frame, point).map(|index| index as usize);
    frame.swap_lifted();
    let Some(index) = reached else {
        pointer.hover(None, false, now);
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
