//! The table's scene objects: one per game object, keyed by the object's slot in the view and
//! kept across updates, so that a card moving between zones is one object throughout.

use std::cell::Cell;
use std::sync::Arc;

use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerView, Side, ZoneName};
use beryl_protocol::script::Subject;
use std::f32::consts::FRAC_PI_4;

use glam::{Affine3A, Quat, Vec2, Vec3};

use super::badges;
use super::hands::hand_top;
use super::look::{Dress, Look};
use super::own::{self, Fights};
use super::pile::PileShown;
use super::place::{Placement, Rest, RowFits, Rows, Scroll, Zooms};
use super::shadows::Floors;
use super::shown::Shown;
use super::tray::TrayMotion;
use super::tray_layout::Held;
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, Layout, PlayerAnchor, Window, takes_pointer};
use crate::faces::{CARD_BACK, Faces};
use crate::profiles::Profile;
use kit_base::data::Data;
use kit_base::pose::{Interval, Pose};
use kit_frame::frame::{
    Band, Draw, ElementId, FrameBuilder, OBJECTS, PaintBand, Place, Resolved, Role, StateBits,
};
use kit_fx::effects;
use kit_space::motion::Track;
use kit_ui::input::FaceSize;

mod abilities;
mod motion;
mod objects;
mod placement;
mod scroll;

pub use motion::PileCount;
pub use motion::{Motion, Turn};

/// How large the table cloth is, in millimetres: past every edge of any window.
const CLOTH: f32 = 4000.0;
/// The cloth lies this far below the cards lying on the table, so that the two never share a
/// height.
const CLOTH_HEIGHT: f32 = -0.5;

/// One game object's presence on the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SceneObject {
    pub(super) pose: Pose,
    pub(super) place: Place,
    pub(super) visible: bool,
    tapped: bool,
    /// How many identical permanents it stands for (`Rest::copies`).
    copies: u16,
    /// Its index in the view's `objects`, for its name; for an ability, in the view's `stack`.
    view_index: u32,
}

/// The table's scene at rest for one view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Objects {
    pub(super) cards: Vec<Option<SceneObject>>,
    /// Sorted by number.
    abilities: Vec<(u32, SceneObject)>,
    /// The numbers assigned to the stack's abilities, bottom first.
    pub(crate) numbers: Vec<u32>,
    /// Each library, graveyard and exile pile, by `PileShown::index`.
    pub(crate) piles: [PileShown; 6],
    rows: Rows,
    /// The cards among `cards` that lie on the table, for contact shadows.
    pub(crate) floors: Floors,
}

impl Objects {
    /// Where the card in `slot` rests, if the scene has one there.
    #[must_use]
    pub fn card_pose(&self, slot: usize) -> Option<Pose> {
        Some(self.cards.get(slot)?.as_ref()?.pose)
    }

    /// Resting card poses in slot order, including empty slots.
    pub fn card_poses(&self) -> impl Iterator<Item = Option<Pose>> + '_ {
        self.cards
            .iter()
            .map(|object| object.as_ref().map(|object| object.pose))
    }

    /// Which groups of each scrollable line of cards are in view.
    #[must_use]
    pub fn rows(&self) -> &Rows {
        &self.rows
    }

    /// The resting poses of the abilities on the stack.
    pub fn ability_poses(&self) -> impl Iterator<Item = (u32, Pose)> + '_ {
        self.abilities.iter().map(|(id, object)| (*id, object.pose))
    }

    /// The scene object of the ability numbered `id`.
    fn ability(&self, id: u32) -> Option<SceneObject> {
        let at = self
            .abilities
            .binary_search_by_key(&id, |(number, _)| *number)
            .ok()?;
        Some(self.abilities[at].1)
    }
}

pub use kit_frame::frame::ABILITY_IDS;

/// The track of the ability numbered `id` in `tracks`, sorted by number, if it has one.
pub fn ability_track(tracks: &[(u32, Track)], id: u32) -> Option<&Track> {
    let at = tracks
        .binary_search_by_key(&id, |(number, _)| *number)
        .ok()?;
    Some(&tracks[at].1)
}

/// The 3D table: the camera and every scene object.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    data: Arc<Data>,
    /// The selected profile fitted to the window.
    layout: Layout,
    camera: Camera,
    row_fits: RowFits,
    scroll: Scroll,
    /// The cards of the row scrolled last, sliding to their new places.
    slide: scroll::Slide,
    pub(super) objects: Objects,
    placed: Placed,
    /// The face last held up for each card slot, used by the Revealed Cards pile.
    pub(super) revealed: Vec<Option<ObjectView>>,
    /// When each permanent's combat marks began, by slot.
    fights: Fights,
    /// The cards a prompt holds up in its tray, and how they lie and move (`table::tray`).
    pub(super) held: Option<Held>,
    pub(super) tray: TrayMotion,
    /// The cards shown in the flat layer's boxes, and their moves (`table::shown`).
    pub(super) shown: Shown,
    /// Whether the prompt covers the table and prevents presses there.
    pub(super) prompt_covers: bool,
    /// Whether the viewer's hand waits below the window (`Table::set_hand_away`).
    hand_away: bool,
}

/// How many objects a table has placed since its count was last taken, for `Work::placed`.
/// Placing reads the table without changing it, so the count is kept in a cell. It is
/// bookkeeping, not what the table shows, so two tables are equal whatever their counts.
#[derive(Clone, Debug, Default)]
struct Placed(Cell<u32>);

impl Placed {
    fn add(&self, objects: usize) {
        let objects = u32::try_from(objects).unwrap_or(u32::MAX);
        self.0.set(self.0.get().saturating_add(objects));
    }
}

impl PartialEq for Placed {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Table {
    #[must_use]
    pub fn new(data: Arc<Data>, window: Window) -> Self {
        let window = data.profiles.resolve_window(window);
        let layout = Profile::for_window(&data.profiles, window).layout();
        let camera = layout.fit(window);
        Self {
            data,
            layout,
            row_fits: RowFits::new(&layout, &camera),
            camera,
            scroll: Scroll::default(),
            slide: scroll::Slide::default(),
            objects: Objects::default(),
            placed: Placed::default(),
            revealed: Vec::new(),
            fights: Fights::with_capacity(OBJECTS),
            held: None,
            tray: TrayMotion::new(),
            shown: Shown::default(),
            prompt_covers: false,
            hand_away: false,
        }
    }

    /// The selected profile before its table geometry is fitted.
    #[must_use]
    pub fn profile(&self) -> Profile<'_> {
        Profile::for_window(&self.data.profiles, self.camera.window)
    }

    pub fn data(&self) -> &Arc<Data> {
        &self.data
    }

    pub fn set_data(&mut self, data: Arc<Data>) {
        self.data = data;
    }

    #[must_use]
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Where `side`'s player is drawn: at their life total's anchor on the screen, at the
    /// depth and card height of the viewer's hand.
    #[must_use]
    pub fn player_pose(&self, side: Side) -> Pose {
        let anchor = self.anchor(side).life;
        self.anchor_pose(anchor)
    }

    /// Where `side`'s unspent mana is drawn, which a streak of mana out of their pool leaves:
    /// at their pool's anchor on the screen, at the depth and card height of the viewer's hand.
    #[must_use]
    pub fn pool_pose(&self, side: Side) -> Pose {
        let anchor = self.anchor(side).pool;
        self.anchor_pose(anchor)
    }

    fn anchor(&self, side: Side) -> &PlayerAnchor {
        &self.layout.players[match side {
            Side::You => 0,
            Side::Opponent => 1,
        }]
    }

    /// The pose at `anchor`, a point of the screen in normalized device coordinates.
    fn anchor_pose(&self, anchor: Vec2) -> Pose {
        let hand = &self.layout.hand;
        self.camera
            .screen_pose(anchor, hand.depth, Quat::IDENTITY, hand.height)
    }

    /// Fits the camera to a window of a new shape, in the layout that shape takes, and
    /// recomputes every resting place.
    pub fn resize(&mut self, window: Window, view: Option<&PlayerView>) {
        let window = self.data.profiles.resolve_window(window);
        self.layout = Profile::for_window(&self.data.profiles, window).layout();
        if self.hand_away {
            // The stage fills the height of the window, so two normalized device units span
            // its full height in stage points.
            self.layout.hand.edge -=
                2.0 * self.layout.style.hand_away / self.layout.tray.stage_height;
        }
        self.camera = self.layout.fit(window);
        self.row_fits = RowFits::new(&self.layout, &self.camera);
        if let Some(view) = view {
            self.apply(view);
        }
    }

    /// Whether the viewer's hand waits below the window (`set_hand_away`).
    #[must_use]
    pub fn hand_away(&self) -> bool {
        self.hand_away
    }

    /// Holds the viewer's hand below the window on the desktop layout, or brings it back to
    /// the window's bottom edge, as the web's `handPlacements` holds it `away` while the opening
    /// shows the hand itself (`ui/src/game/handLayout.ts`), so that it is not seen twice and is
    /// dealt out of sight. Takes effect at the next `resize`.
    pub fn set_hand_away(&mut self, away: bool) {
        self.hand_away = away;
    }

    /// The table's current resting objects and abilities.
    #[must_use]
    pub fn objects(&self) -> &Objects {
        &self.objects
    }

    /// Where the object in `slot` rests in the view the table shows, if it shows it.
    #[must_use]
    pub fn rest(&self, slot: usize) -> Option<Pose> {
        self.objects.card_pose(slot)
    }

    /// Where the ability numbered `id` rests in the view the table shows, if it is on its stack.
    #[must_use]
    pub fn ability_rest(&self, id: u32) -> Option<Pose> {
        Some(self.objects.ability(id)?.pose)
    }

    /// The numbers of the abilities on the stack of the view the table shows, bottom first.
    #[must_use]
    pub fn abilities(&self) -> &[u32] {
        &self.objects.numbers
    }

    /// How many objects the table has placed since the last call, and starts counting again.
    pub fn take_placed(&self) -> u32 {
        self.placed.0.take()
    }

    /// Copies the resting places that placement stored for the view the table shows.
    pub fn rests(&self) -> Placement {
        let mut placement = Placement {
            piles: self.objects.piles,
            rows: self.objects.rows,
            abilities: self
                .objects
                .abilities
                .iter()
                .map(|(id, object)| {
                    let rest = Rest {
                        pose: object.pose,
                        place: object.place,
                        visible: object.visible,
                        copies: 1,
                    };
                    (*id, rest)
                })
                .collect(),
            rests: self
                .objects
                .cards
                .iter()
                .map(|object| {
                    object.map(|object| Rest {
                        pose: object.pose,
                        place: object.place,
                        visible: object.visible,
                        copies: object.copies,
                    })
                })
                .collect(),
            // Only placing a group reads the zooms (`Motion::place` keeps its own).
            zooms: Zooms::default(),
        };
        self.unhold(&mut placement);
        placement
    }

    /// Puts every object of `view` at its resting place, and forgets the objects it no longer
    /// holds, as an update with a reset script must. Cards a prompt holds up rest in its tray.
    pub fn apply(&mut self, view: &PlayerView) {
        let numbers = std::mem::take(&mut self.objects.numbers);
        let placement = self.placement(view, &numbers);
        if let Some(dock) = self.layout.life_dock {
            let rests = placement.rests.iter().flatten();
            let top = hand_top(
                &self.camera,
                rests.map(|rest| (rest.place, rest.pose, rest.visible)),
            );
            self.layout.players[0] = dock.anchor(self.camera.window, top);
        }
        let placement = self.with_held(placement);
        self.objects = Self::objects_for(view, numbers, &placement);
    }

    /// Exchanges the resting scene at the end of a script without allocating.
    /// The cards a prompt holds up stay in its tray.
    pub fn swap_objects(&mut self, objects: &mut Objects) {
        std::mem::swap(&mut self.objects, objects);
        if let Some(dock) = self.layout.life_dock {
            let rests = self.objects.cards.iter().flatten();
            let top = hand_top(
                &self.camera,
                rests.map(|object| (object.place, object.pose, object.visible)),
            );
            self.layout.players[0] = dock.anchor(self.camera.window, top);
        }
        self.rehold();
    }

    /// Writes a resolved element and a draw record for every visible card, with a contact
    /// shadow for each card above what lies under it, and one draw record for the cloth, which
    /// comes last among the opaque records and carries the shade along the table's far edge
    /// (`far_shade`). `view` is the view the table shows. While a script
    /// plays, `motion` gives the objects that move their poses, and the records of the view the
    /// script leads to. A permanent wears the marks its record calls for (`own`). Each card is
    /// drawn from the board-size layer of its face in `faces`, or layer 0 while that face has
    /// none or the viewer does not know the card's name, and its back from the card back's
    /// layer. How each card looks is then changed by `dress`: the effects on it, the marks on
    /// it and the pointer, in that order. `revealed` lists the visible faces in draw order
    /// with the interval over which each grows into place.
    pub fn write(
        &self,
        view: &PlayerView,
        motion: Option<Motion<'_>>,
        faces: &Faces,
        dress: &Dress<'_>,
        revealed: impl Iterator<Item = (ObjectId, Interval)> + Clone,
        builder: &mut FrameBuilder,
    ) {
        let back = faces.board_layer_of(CARD_BACK).unwrap_or(0);
        builder.camera(self.camera.view_projection);
        let slots = motion
            .map_or(0, |motion| motion.tracks.len())
            .max(self.objects.cards.len());
        for slot in 0..slots {
            builder.visit();
            // A box shows the card as the table holds it at rest, in the view the table shows,
            // which the screen laying the box out was drawn from: while a script plays, the
            // card it moves may already have left for another zone, or its slot hold another
            // card, as a mulligan's new hand reuses the old hand's slots.
            let boxed = (self.in_box(slot))
                .then(|| self.objects.cards.get(slot).copied().flatten())
                .flatten();
            let Some((mut object, mut drawn, mut under_way)) = (self
                .object_at(slot, view, motion, builder))
            .or_else(|| boxed.map(|resting| (resting, view, false))) else {
                self.fights.forget(slot);
                continue;
            };
            let track = motion.and_then(|motion| Some((motion.tracks.get(slot)?, motion.t)));
            let paused = track.is_some_and(|(track, time)| track.paused(time));
            let layout_motion =
                under_way && !paused && !track.is_some_and(|(track, time)| track.crossing(time));
            // A card at rest in a row that scrolled slides to its new place, as one does into
            // or out of a prompt's tray.
            under_way = under_way || self.slide_look(slot, &mut object, dress.now);
            under_way = under_way || self.tray_look(slot, &mut object, dress.now);
            // A card a box shows takes no effect there: its effects play where the table holds
            // it, as the web's opening and viewer draw their own copy of the card while the
            // stage's card plays the script (`write_card`).
            let held_at = boxed.map(|_| object.pose);
            if let Some(resting) = boxed {
                (object, drawn) = (resting, view);
            }
            under_way = self.shown_look(slot, &mut object, dress.now) || under_way;
            if !object.visible {
                self.fights.forget(slot);
                continue;
            }
            let mut state = u32::from(under_way || paused) * StateBits::MOVING;
            if layout_motion && !self.in_tray(slot) && boxed.is_none() {
                state |= StateBits::LAYOUT_MOTION;
            }
            self.write_card(
                slot,
                (&object, drawn, (under_way, state), held_at),
                (view, faces),
                (back, dress, self.handed_waiting(motion)),
                builder,
            );
        }
        self.write_piles(builder);
        self.write_tray_library(back, builder);
        self.write_abilities(view, motion, faces, (back, dress), builder);
        self.write_revealed(faces, (back, dress.now), revealed, builder);
        let cloth = Affine3A::from_scale_rotation_translation(
            Vec3::new(CLOTH / CARD_WIDTH, CLOTH / CARD_HEIGHT, 1.0),
            glam::Quat::IDENTITY,
            Vec3::new(0.0, 0.0, CLOTH_HEIGHT),
        );
        let mut tabletop = Draw::tabletop(cloth, [255; 4]);
        if self.layout.style.far_shade {
            let (centre, radius) = super::far_shade::place(self.camera.window);
            tabletop = tabletop.with_far_shade(centre, radius);
        }
        builder.draw(Band::Opaque, tabletop);
    }

    /// The object in `slot` as this frame draws it, the view that names it, and whether it is
    /// moving: at rest in `view`, or at its pose on its track while a script plays. `None` for a
    /// slot with no object.
    fn object_at<'v>(
        &self,
        slot: usize,
        view: &'v PlayerView,
        motion: Option<Motion<'v>>,
        builder: &mut FrameBuilder,
    ) -> Option<(SceneObject, &'v PlayerView, bool)> {
        let resting = self.objects.cards.get(slot).copied().flatten();
        let copies = motion
            .and_then(|motion| motion.copies(slot))
            .unwrap_or_else(|| resting.map_or(1, |object| object.copies));
        let turn = motion.and_then(|motion| motion.turn(slot).map(|at| (motion, at)));
        let track = motion.and_then(|motion| {
            let track = motion.tracks.get(slot)?;
            (!track.segments().is_empty()).then_some((motion, track))
        });
        let Some((motion, track)) = track else {
            let resting = resting?;
            // A card that stands still shows its new record from its turn on, where it stands.
            if let Some((motion, at)) = turn
                && motion.t >= at
                && let Some(last) = motion.finals.cards.get(slot).copied().flatten()
            {
                let object = SceneObject {
                    pose: resting.pose,
                    visible: resting.visible,
                    tapped: resting.tapped,
                    copies,
                    ..last
                };
                return Some((object, motion.view, false));
            }
            return Some((SceneObject { copies, ..resting }, view, false));
        };
        let last = motion.finals.cards.get(slot).copied().flatten();
        // A card a row has scrolled out of view rests undrawn at the row's nearer end
        // (`rows::beyond_right` for the right end), as does a card under the top two of a pile.
        // One out of view before the script and after it stays undrawn while it slides, or only
        // drops a layer because a card before it left, or it would cover the cards there. A card
        // that changes place is drawn on its way, wherever it starts or ends, and so is one a
        // step's own verb moves across the table, such as a flight to the end of its row: it
        // leaves from where it rests undrawn. Before its track first moves it, a card is drawn as
        // where it rests is; once its track has ended, as where the script leaves it is.
        let moves = resting.zip(last).is_some_and(|(a, b)| a.place != b.place);
        if !moves
            && !resting.is_some_and(|object| object.visible)
            && !last.is_some_and(|object| object.visible)
            && !track.carried(motion.t)
        {
            return None;
        }
        let begun = track.begun(motion.t);
        match (resting, last) {
            (Some(resting), _) if !begun => {
                if !resting.visible {
                    return None;
                }
            }
            (_, Some(last)) if !last.visible && track.settled(motion.t) => return None,
            _ => {}
        }
        builder.segment();
        let pose = track.pose_at(motion.t);
        let veiled = turn.is_some_and(|(_, at)| motion.t < at);
        let (object, view) = match (last, resting) {
            (Some(_), Some(object)) if veiled => (object, view),
            (Some(object), _) => (object, motion.view),
            (None, _) => (resting?, view),
        };
        // A permanent that taps or untaps on its track reads as tapped once it has turned
        // more than half of its quarter turn, from the middle of its step.
        let tapped = [resting, last].iter().flatten().any(|object| object.tapped)
            && pose.rotation.angle_between(Quat::IDENTITY) > FRAC_PI_4;
        let place = resting
            .filter(|rest| !begun && !takes_pointer(rest.place))
            .map_or(object.place, |rest| rest.place);
        let object = SceneObject {
            pose,
            visible: pose.opacity > 0.0,
            tapped,
            place,
            copies,
            ..object
        };
        Some((object, view, track.moving(motion.t)))
    }

    /// Writes the resolved element and the draw record of the card in `slot`: `object` as
    /// `view` shows it, with its motion in `state`. `under_way` controls its shadow. Its face
    /// comes from `faces`, its back from layer
    /// `back`, and how it looks is changed by `dress`. `shown_view` is the view the table shows
    /// before the script, whose face a `cover` shows until half way; `waiting` is `pressable`'s.
    fn write_card(
        &self,
        slot: usize,
        (object, view, (under_way, mut state), held_at): (
            &SceneObject,
            &PlayerView,
            (bool, u32),
            Option<Pose>,
        ),
        (shown_view, faces): (&PlayerView, &Faces),
        (back, dress, waiting): (u16, &Dress<'_>, bool),
        builder: &mut FrameBuilder,
    ) {
        let shown = &view.objects[object.view_index as usize];
        let name = shown.name.as_deref().unwrap_or("");
        let (printed, layer) = shown
            .token_printing
            .as_deref()
            .map_or((None, None), |printing| faces.listed_board(printing));
        let (listed, face) = if printed.is_some() && layer.is_some() {
            (printed, layer)
        } else {
            let (listed, face) = faces.listed_board(name);
            (printed.or(listed), layer.or(face))
        };
        if object.tapped {
            state |= StateBits::TAPPED;
        }
        if object.pose.turn > 0.5 {
            state |= StateBits::FACE_DOWN;
        }
        state |= u32::from(shown.phased_out) * StateBits::PHASED_OUT;
        state |= u32::from(shown.hidden_pick) * StateBits::PICKED;
        // The desktop hand and stack show the full frame. The phone uses the board frame
        // (`scanOrFrame` in `ui/src/mobile/faces.tsx`).
        let full = (matches!(shown.zone, ZoneName::Hand | ZoneName::Stack)
            && self.layout.style.full_card_faces)
            .then(|| faces.full_layers_of_object(shown))
            .flatten();
        // A copy beneath the top card peeks out too little to be pressed by itself.
        // A pile is pressed through its top card. The tray makes only its own cards pressable.
        let interactive = state & StateBits::FACE_DOWN == 0
            && object.copies > 0
            && !name.is_empty()
            && self.pressable(slot, object.place, waiting);
        let mut look = Look::new(object.pose, face, full, state, interactive);
        let on = Subject::Object {
            slot: ObjectId(u16::try_from(slot).expect("slots are u16")),
        };
        let earlier_face = || {
            self.objects
                .cards
                .get(slot)
                .copied()
                .flatten()
                .map_or(face, |resting| {
                    face_of(faces, &shown_view.objects[resting.view_index as usize])
                })
        };
        // A card shown in a box (`held_at`) is drawn there, and its effects at the place
        // the table holds it.
        let effects_on_card = if held_at.is_some() {
            &[]
        } else {
            dress.effects
        };
        let effects = &self.data.effects;
        effects::card::look_with_face(
            (effects, builder.tier()),
            effects_on_card,
            on,
            dress.now,
            earlier_face,
            &mut look,
        );
        (dress.marks)(on, &mut look);
        (dress.pointer)(slot, &self.camera, &mut look);
        let held = held_at.map_or(look, |pose| Look { pose, ..look });
        effects::card::write(effects, dress.effects, on, dress.now, &held, builder);
        self.write_shadow((Some(slot), object.place), &look.pose, under_way, builder);
        let at = self.write_look(
            name,
            (
                ElementId(u32::try_from(slot).expect("slots are u16")),
                Role::Card,
                object.copies,
            ),
            object.place,
            &look,
            back,
            builder,
        );
        let band = builder.node_band(at);
        if shown.zone == ZoneName::Battlefield {
            own::write(
                (slot, shown),
                &look,
                &self.fights,
                (dress.now, band),
                builder,
            );
        } else {
            self.fights.forget(slot);
        }
        if let Some(really) = shown.really.as_deref() {
            builder.really(at, really);
        }
        let on = self.badges_on(&look, band);
        let note = (dress.badges).write((shown, listed, object.copies), &on, builder);
        if let Some(note) = note {
            builder.note(at, note);
        }
        // A hand card's cost stands over the card where the table holds it, not over its box:
        // the web's opening draws its copies of the hand without costs.
        let cost = (shown, listed, at, object.place);
        let held = held_at.map_or(on, |pose| badges::On { pose, ..on });
        (dress.badges).write_cost(cost, (&held, &self.camera), builder);
    }

    /// The badges share the card's paint band while it moves or is dragged.
    fn badges_on(&self, look: &Look, band: PaintBand) -> badges::On {
        badges::On {
            pose: look.pose,
            band: match band {
                PaintBand::Moving | PaintBand::Dragged => band.records(Band::TableMarks),
                _ => Band::TableMarks,
            },
            opacity: look.pose.opacity,
            show_copy_label: self.layout.style.show_copy_labels,
        }
    }

    /// Writes the resolved element and the draw record of a card named `name` with this `id`
    /// and `role`, resting at `place` and drawn as `look` says, with its back from layer `back`.
    /// Returns the element's index in `Frame::resolved`.
    pub(super) fn write_look(
        &self,
        name: &str,
        (id, role, copies): (ElementId, Role, u16),
        place: Place,
        look: &Look,
        back: u16,
        builder: &mut FrameBuilder,
    ) -> u32 {
        let transform = look.pose.transform();
        let shown =
            role == Role::Card && (self.shown.holds(id.0 as usize) || self.in_tray(id.0 as usize));
        let clip = shown.then(|| self.shown_clip(id)).flatten();
        let at = builder.resolved(
            name,
            Resolved {
                id,
                parent: u32::MAX,
                role,
                name: kit_frame::frame::NameRef::default(),
                place,
                pose: Some(look.pose),
                corners: self.camera.card_corners(transform),
                depth: self.camera.depth(look.pose.position),
                opacity: look.pose.opacity,
                clip,
                interactive: look.interactive,
                state: StateBits(look.state | if shown { StateBits::SHOWN } else { 0 }),
                face: look.face,
                copies,
                really: kit_frame::frame::NameRef::default(),
                note: kit_frame::frame::NameRef::default(),
            },
        );
        let opacity = (look.pose.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let [red, green, blue] = look.tint;
        // The full-size face, once it has landed, or the board-size one enlarged.
        let (size, layer, back) = match (look.face, look.full) {
            (Some(_), Some([full, full_back])) => (FaceSize::Full, full, full_back),
            (face, _) => (FaceSize::Board, face.unwrap_or(0), back),
        };
        // A fading, dissolving or whitening card blends among its own marks so they stay
        // visible and the records beneath it show through.
        // Cards held in a box (`table::shown`) or tray (`prompts::screen`) draw over the
        // flat layer.
        let resting = if opacity == 255 && look.dissolve <= 0.0 && look.whiten <= 0.0 {
            Band::Opaque
        } else if look.pose.position.z < kit_base::data::space::light::ON_TABLE_BELOW {
            Band::TableMarks
        } else {
            Band::RaisedMarks
        };
        let scissor = clip
            .and_then(|rect| {
                let scale = self.camera.window.scale;
                builder.scissor(kit_base::geometry::zoom_rect(
                    rect,
                    (glam::Vec2::ZERO, scale),
                ))
            })
            .unwrap_or(0);
        builder.draw_card(
            at,
            resting,
            Draw::card(
                transform,
                [red, green, blue, opacity],
                size,
                layer,
                back,
                look.dissolve,
                look.whiten,
            )
            .with_clip(scissor),
        );
        at
    }
}

/// The board-size layer of the face of the card `shown` records, if it has one. A token's
/// printing tells apart tokens that share a name, so its face is found by that first.
pub(super) fn face_of(faces: &Faces, shown: &ObjectView) -> Option<u16> {
    shown
        .token_printing
        .as_deref()
        .and_then(|printing| faces.board_layer(printing))
        .or_else(|| faces.board_layer(shown.name.as_deref().unwrap_or("")))
}
