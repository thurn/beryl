//! The cards a prompt holds up in its tray (docs/beryl-3d.md, "Objects and placement"): where
//! the table rests them while the tray is up, and how they move into it and back out of it.
//! Where in the tray each card lies is `tray_layout`'s.

use beryl_protocol::protocol::{ObjectId, PlayerView};

use super::scene::SceneObject;
use super::tray_layout::{Held, TrayPoses, TraySpace, tray_poses};
use super::{Camera, Placement, Rest, Table};
use crate::frame::{Band, Draw, FrameBuilder, OBJECTS};
use crate::input::FaceSize;
use crate::motion::between;
use crate::pose::{Curve, Pose};

impl Placement {
    /// Gives each card `held` names its place in the tray instead of its zone's, laid out in
    /// `space` and seen through `camera`. A card keeps its zone's `Place`, so that a step still
    /// finds it in its zone, and is drawn, wherever in its zone it lies.
    pub fn hold(&mut self, held: &Held, space: TraySpace, camera: &Camera) {
        for (slot, pose) in tray_poses(held, space, camera).cards {
            if let Some(rest) = self.get(slot).copied() {
                self.set(
                    slot,
                    Rest {
                        pose,
                        visible: true,
                        ..rest
                    },
                );
            }
        }
    }
}

/// The cards of a tray on their way into it or back out of it: for each slot, where the card
/// was drawn as the move began, which it goes from to where it now rests over `over` (in
/// nanoseconds since the client started), eased by `curve`. A move that is waiting for its
/// first frame, or for the script that answers a closed prompt to start, has no `over` yet,
/// only its length.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrayMotion {
    /// Where each card held up lies in the tray, and, by slot, where it rests in its zone and
    /// whether it is drawn there, which scripts are played from (`Table::rests`).
    poses: Vec<(ObjectId, Pose)>,
    /// The card backs a put-back's library is drawn with in the tray, the deepest first.
    library: Vec<Pose>,
    zone: Vec<Option<(Pose, bool)>>,
    from: Vec<Option<Pose>>,
    over: Option<(u64, u64)>,
    /// A move that has not begun yet (`over` is `None`).
    waiting: Option<Wait>,
    /// The cards a let-go puts back, by slot, which `Show::place_let_go` looks for in the steps
    /// of the script that answers the closed prompt.
    letting: Vec<usize>,
    curve: Curve,
    /// Whether the cards are going back out of the tray.
    leaving: bool,
    /// The cards the tray let go that a script took over, which wait where the tray held them
    /// up until their own step carries them on (`Show::hand_over_tray`).
    handed: Vec<usize>,
    /// The `now` of the latest frame.
    now: u64,
}

impl TrayMotion {
    /// Room for every slot, so that no move allocates.
    pub(crate) fn new() -> Self {
        Self {
            poses: Vec::new(),
            library: Vec::new(),
            zone: vec![None; OBJECTS],
            from: vec![None; OBJECTS],
            over: None,
            waiting: None,
            letting: Vec::new(),
            curve: Curve::EASE_OUT,
            leaving: false,
            handed: Vec::new(),
            now: 0,
        }
    }

    /// How far through the move is at `now`, eased, or `None` once it is over. A move that has
    /// not begun has the cards where they were drawn as it was asked for.
    fn share(&self, now: u64) -> Option<f32> {
        let Some((from, to)) = self.over else {
            return self.waiting.map(|_| self.curve.ease(0.0));
        };
        if now >= to {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a move lasts well under a second of nanoseconds"
        )]
        let elapsed = now.saturating_sub(from) as f32 / (to - from).max(1) as f32;
        Some(self.curve.ease(elapsed))
    }

    /// Notes that the card in `slot` moves from `pose`.
    fn starts(&mut self, slot: usize, pose: Pose) {
        if let Some(from) = self.from.get_mut(slot) {
            *from = Some(pose);
        }
    }
}

/// A move of the tray's cards that has not begun: how long it takes, in nanoseconds, and the
/// number of the script whose start places it (`Table::start_let_go`), if it waits for one
/// rather than for the next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wait {
    length: u64,
    script: Option<usize>,
}

impl Table {
    /// Holds up the cards `held` names, or none: from the next placement on, they rest in the
    /// tray rather than in their zones.
    pub fn hold(&mut self, held: Option<Held>) {
        self.held = held;
    }

    /// Covers the table for the open prompt, or uncovers it: while the prompt lists what it asks
    /// about over the table on a shade of its own, such as an order's waiting triggers, no card
    /// of the table takes a press (`pressable`).
    pub fn cover_for_prompt(&mut self, covers: bool) {
        self.prompt_covers = covers;
    }

    /// Whether the card in `slot` is held up in the tray.
    #[must_use]
    pub fn in_tray(&self, slot: usize) -> bool {
        self.held.as_ref().is_some_and(|held| {
            (held.piles.iter().flatten()).any(|card| usize::from(card.0) == slot)
        })
    }

    /// Whether a press may reach the card in `slot`, resting at `place`: one where the pointer
    /// reaches cards, while no tray is up; and while one is, only a card in it, as the shade the
    /// web interface lays over the table under a tray takes the pointer (`coversTable` in
    /// `ui/src/stage/prompt.ts`). None while the table is covered, by a viewer or by the open
    /// prompt (`cover_for_prompt`). Scripts are played from where the cards rest in their
    /// zones: only the table's own resting places put them in the tray.
    ///
    /// The table takes no press either while a card the tray let go still waits where it was
    /// held up for its own step (`waiting`, from `handed_waiting`): it stands over the table as
    /// it did in the tray, and nothing under it could be reached.
    pub(super) fn pressable(&self, slot: usize, place: crate::frame::Place, waiting: bool) -> bool {
        match self.held {
            _ if self.covered() => false,
            // The hand held below the window lies under the opening, whose overlay takes the
            // pointer, as the web's does; the cards the opening shows in its boxes still take
            // presses.
            _ if self.hand_away() && is_your_hand(place) && !self.in_box(slot) => false,
            Some(_) => self.in_tray(slot),
            None => !waiting && !self.prompt_covers && crate::pointer::takes_pointer(place),
        }
    }

    /// Whether a card the tray let go, which the script playing in `motion` took over, still
    /// waits where the tray held it up: the script's track has not yet begun to move it.
    pub(super) fn handed_waiting(&self, motion: Option<super::Motion<'_>>) -> bool {
        motion.is_some_and(|motion| {
            (self.tray.handed.iter())
                .any(|&slot| (motion.tracks.get(slot)).is_some_and(|track| !track.begun(motion.t)))
        })
    }

    /// The cards held up in the tray, if any.
    #[must_use]
    pub fn held(&self) -> Option<&Held> {
        self.held.as_ref()
    }

    /// Where each card of `held` lies in the tray for this window, by slot, and the backs of a
    /// put-back's library.
    #[must_use]
    pub fn tray_poses(&self, held: &Held) -> TrayPoses {
        tray_poses(held, self.layout().tray, self.camera())
    }

    /// Holds up the cards of `held`, each going from where it is drawn now to its place in
    /// `poses` (from `tray_poses`) over `over`, in nanoseconds since the client started, eased
    /// by `curve`. A put-back's library is drawn there at once. Allocates nothing.
    pub fn lift(&mut self, held: Held, poses: TrayPoses, over: (u64, u64), curve: Curve) {
        let TrayPoses {
            cards: poses,
            library,
        } = poses;
        for &(slot, pose) in &poses {
            let slot = usize::from(slot.0);
            let Some(Some(mut object)) = self.objects.cards.get(slot).copied() else {
                continue;
            };
            let drawn = self.tray_drawn(slot, object);
            self.tray.starts(slot, drawn);
            if let Some(zone) = self.tray.zone.get_mut(slot) {
                *zone = Some((object.pose, object.visible));
            }
            object.pose = pose;
            object.visible = true;
            self.objects.cards[slot] = Some(object);
        }
        self.held = Some(held);
        self.tray.poses = poses;
        self.tray.library = library;
        self.tray.over = Some(over);
        self.tray.waiting = None;
        self.tray.curve = curve;
        self.tray.leaving = false;
        self.tray.handed.clear();
    }

    /// Where each card held up rests in its zone in `view`, the view the table shows, for
    /// `let_go`: its pose and whether it is drawn there.
    #[must_use]
    pub fn zone_rests(&self, view: &PlayerView) -> Vec<(ObjectId, Pose, bool)> {
        let Some(held) = &self.held else {
            return Vec::new();
        };
        let placement = self.placement(view, &self.objects.numbers);
        (held.piles.iter().flatten())
            .filter_map(|&slot| {
                let rest = placement.get(slot)?;
                Some((slot, rest.pose, rest.visible))
            })
            .collect()
    }

    /// Lets the cards held up go back to their places in `rests` (from `zone_rests`) over
    /// `length` nanoseconds, eased by `curve`: each rests there from now on and is drawn on its
    /// way. The move begins at the next frame, or, where `script` names the script that answers
    /// the closed prompt, when that script's start places it (`start_let_go`); until then each
    /// card is drawn where the tray held it. A card a script moves meanwhile starts from the
    /// tray instead (`Show::hand_over_tray`). Allocates nothing once the tray has let go of as
    /// many cards before.
    pub fn let_go(
        &mut self,
        rests: &[(ObjectId, Pose, bool)],
        (length, script): (u64, Option<usize>),
        curve: Curve,
    ) {
        if self.held.take().is_none() {
            return;
        }
        self.tray.poses.clear();
        self.tray.library.clear();
        self.tray.letting.clear();
        (self.tray.letting).extend(rests.iter().map(|&(slot, ..)| usize::from(slot.0)));
        for &(slot, pose, visible) in rests {
            let index = usize::from(slot.0);
            let Some(Some(mut object)) = self.objects.cards.get(index).copied() else {
                continue;
            };
            let drawn = self.tray_drawn(index, object);
            self.tray.starts(index, drawn);
            object.pose = pose;
            object.visible = visible;
            self.objects.cards[index] = Some(object);
        }
        self.tray.over = None;
        self.tray.waiting = Some(Wait { length, script });
        self.tray.curve = curve;
        self.tray.leaving = true;
    }

    /// The number of the script whose start places the let-go of the tray's cards, while one
    /// waits for it, and the cards it lets go, by slot.
    pub(crate) fn let_go_waits_for(&self) -> Option<(usize, &[usize])> {
        let script = self.tray.waiting?.script?;
        Some((script, &self.tray.letting))
    }

    /// Begins the let-go that waits for a script at `at`, in nanoseconds since the client
    /// started, and returns how long it takes.
    pub(crate) fn start_let_go(&mut self, at: u64) -> u64 {
        let Some(Wait { length, .. }) = self.tray.waiting.take() else {
            return 0;
        };
        self.tray.over = Some((at, at + length));
        length
    }

    /// Takes `now`, the time of the frame being drawn, and begins a move of the tray's cards
    /// that waits for it.
    pub(crate) fn advance_tray(&mut self, now: u64) {
        self.tray.now = now;
        if let Some(Wait {
            length,
            script: None,
        }) = self.tray.waiting
        {
            self.tray.waiting = None;
            self.tray.over = Some((now, now + length));
        }
    }

    /// Where the card in `slot`, resting as `object` says, is drawn now: part of the way through
    /// a move of the tray's, or where it rests.
    fn tray_drawn(&self, slot: usize, mut object: SceneObject) -> Pose {
        self.tray_look(slot, &mut object, self.tray.now);
        object.pose
    }

    /// Where the card in `slot` is drawn at `now`, given `pose`, where the playing script's track
    /// or its rest puts it: moved as `write` moves it, by the slide of a row that scrolled, the
    /// tray's move into or out of the tray, and the box that shows it. `pose` itself for a slot
    /// the table holds no card in.
    #[must_use]
    pub fn drawn_pose(&self, slot: usize, pose: Pose, now: u64) -> Pose {
        let Some(mut object) = self.objects.cards.get(slot).copied().flatten() else {
            return pose;
        };
        object.pose = pose;
        self.slide_look(slot, &mut object, now);
        self.tray_look(slot, &mut object, now);
        self.shown_look(slot, &mut object, now);
        object.pose
    }

    /// Changes `object`, the card in `slot` as it rests, for the move of the tray's cards under
    /// way at `now`: it is drawn part of the way from where it was as the move began. Returns
    /// whether it moves.
    pub(super) fn tray_look(&self, slot: usize, object: &mut SceneObject, now: u64) -> bool {
        let (Some(share), Some(Some(from))) =
            (self.tray.share(now), self.tray.from.get(slot).copied())
        else {
            return false;
        };
        object.pose = between(&from, &object.pose, share);
        true
    }

    /// Whether a card of the tray moves at `now`. A let-go that waits for its script holds
    /// still until the script places it.
    #[must_use]
    pub fn tray_moving(&self, now: u64) -> bool {
        self.tray.over.is_some_and(|(_, to)| now < to)
            || self.tray.waiting.is_some_and(|wait| wait.script.is_none())
    }

    /// Whether the tray has let its cards go and no script has started since: the script that
    /// starts next moves each card it moves from where the tray let it go
    /// (`Show::hand_over_tray`).
    pub(crate) fn tray_leaving(&self) -> bool {
        self.tray.leaving
    }

    /// A script has taken over the cards the tray let go: those in `handed` wait where the tray
    /// held them up until the script's tracks move them.
    pub(crate) fn handed_over_tray(&mut self, handed: &[usize]) {
        self.tray.leaving = false;
        self.tray.handed.clear();
        self.tray.handed.extend_from_slice(handed);
    }

    /// Where the card in `slot` was drawn as the tray began to let it go, while no script has
    /// taken the tray's cards over yet.
    pub(crate) fn letting_go(&self, slot: usize) -> Option<Pose> {
        if !self.tray.leaving {
            return None;
        }
        self.tray.from.get(slot).copied().flatten()
    }

    /// `placement` with the cards held up in the tray, where this window lays it out.
    pub(super) fn with_held(&mut self, mut placement: Placement) -> Placement {
        if let Some(held) = &self.held {
            placement.hold(held, self.layout().tray, self.camera());
            let TrayPoses { cards, library } = self.tray_poses(held);
            self.tray.poses = cards;
            self.tray.library = library;
        }
        placement
    }

    /// Puts each card held up back at its place in the tray, after the scene objects were
    /// exchanged for a script's, which are played from the cards' zones. Allocates nothing.
    pub(super) fn rehold(&mut self) {
        for &(slot, pose) in &self.tray.poses {
            let index = usize::from(slot.0);
            if let Some(Some(object)) = self.objects.cards.get_mut(index) {
                if let Some(zone) = self.tray.zone.get_mut(index) {
                    *zone = Some((object.pose, object.visible));
                }
                object.pose = pose;
                object.visible = true;
            }
        }
    }

    /// Gives each card held up its place in its zone in `placement`, the table's resting
    /// places, for the scripts played from them: the tray is the table's to draw, not theirs.
    pub(super) fn unhold(&self, placement: &mut Placement) {
        for &(slot, _) in &self.tray.poses {
            let index = usize::from(slot.0);
            let (Some(Some(rest)), Some(Some((pose, visible)))) = (
                placement.rests.get_mut(index),
                self.tray.zone.get(index).copied(),
            ) else {
                continue;
            };
            rest.pose = pose;
            rest.visible = visible;
        }
    }

    /// Writes the card backs a put-back's library is drawn with, under the cards of the tray and
    /// like them over the tray's shade, each showing the card back's layer `back` on its face.
    /// Allocates nothing.
    pub(super) fn write_tray_library(&self, back: u16, builder: &mut FrameBuilder) {
        for pose in &self.tray.library {
            let draw = Draw::card(
                pose.transform(),
                [255; 4],
                FaceSize::Board,
                back,
                back,
                0.0,
                0.0,
            );
            builder.draw(Band::LayerCards, draw);
        }
    }
}

/// Whether `place` is the viewer's hand.
fn is_your_hand(place: crate::frame::Place) -> bool {
    matches!(
        place,
        crate::frame::Place::Card {
            zone: beryl_protocol::protocol::ZoneName::Hand,
            side: beryl_protocol::protocol::Side::You,
            ..
        }
    )
}
