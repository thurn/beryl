//! The table's scene objects: one per game object, keyed by the object's slot in the view and
//! kept across updates, so that a card moving between zones is one object throughout.

use std::cell::Cell;

use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerView, Side, StackItem, ZoneName};
use beryl_protocol::script::Subject;
use std::f32::consts::FRAC_PI_4;

use glam::{Affine3A, Quat, Vec2, Vec3};

use super::badges;
use super::look::{Dress, Look};
use super::own::{self, Fights};
use super::pile::PileShown;
use super::place::{Placement, Rest, RowFits, Rows, Scroll};
use super::shadows::Floors;
use super::shown::Shown;
use super::tray::TrayMotion;
use super::tray_layout::Held;
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, DESKTOP, Layout, PlayerAnchor, Window};
use crate::effects::{self, Effect};
use crate::faces::{CARD_BACK, Faces};
use crate::frame::{
    Band, Draw, ElementId, FrameBuilder, OBJECTS, Place, Resolved, Role, StateBits,
};
use crate::input::FaceSize;
use crate::light;
use crate::motion::Track;
use crate::pointer;
use crate::pose::Pose;

mod motion;
mod objects;
mod placement;
mod scroll;

pub(crate) use motion::PileCount;
pub use motion::{Motion, Turn};

/// How large the table cloth is, in millimetres: past every edge of any window.
const CLOTH: f32 = 4000.0;
/// The cloth lies this far below the cards lying on the table, so that the two never share a
/// height.
const CLOTH_HEIGHT: f32 = -0.5;
const CLOTH_TINT: [u8; 4] = [38, 66, 58, 255];

/// One game object's presence on the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneObject {
    pub(super) pose: Pose,
    pub(super) place: Place,
    pub(super) visible: bool,
    tapped: bool,
    /// How many identical permanents it stands for (`Rest::copies`).
    copies: u16,
    /// Its index in the view's `objects`, for its name; for an ability, in the view's `stack`.
    view_index: u32,
}

impl SceneObject {
    /// Where it rests.
    pub(crate) fn pose(&self) -> Pose {
        self.pose
    }
}

/// The scene objects of one view: a card for each game object the table shows, by slot, and
/// one for each ability on the stack, by number, with the numbers of the stack's abilities.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Objects {
    pub cards: Vec<Option<SceneObject>>,
    /// Sorted by number.
    pub abilities: Vec<(u32, SceneObject)>,
    /// The numbers of the abilities on the view's stack, bottom first, which the view itself
    /// does not give.
    pub numbers: Vec<u32>,
    /// Each library, graveyard and exile pile, by `PileShown::index`.
    pub piles: [PileShown; 6],
    /// Which groups of each battlefield row are in view.
    pub rows: Rows,
    /// The cards among `cards` that lie on the table, for contact shadows.
    pub floors: Floors,
}

impl Objects {
    /// The scene object of the ability numbered `id`.
    pub(crate) fn ability(&self, id: u32) -> Option<SceneObject> {
        let at = self
            .abilities
            .binary_search_by_key(&id, |(number, _)| *number)
            .ok()?;
        Some(self.abilities[at].1)
    }
}

/// The id of the resolved element of the ability numbered `number`: past every card's and
/// every flat-layer element's. The numbers a table made from a view alone gives count down from
/// `u32::MAX`, and are folded into the same range.
pub const ABILITY_IDS: u32 = 1 << 20;

/// The track of the ability numbered `id` in `tracks`, sorted by number, if it has one.
pub(crate) fn ability_track(tracks: &[(u32, Track)], id: u32) -> Option<&Track> {
    let at = tracks
        .binary_search_by_key(&id, |(number, _)| *number)
        .ok()?;
    Some(&tracks[at].1)
}

/// The 3D table: the camera and every scene object.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    layout: Layout,
    camera: Camera,
    row_fits: RowFits,
    scroll: Scroll,
    /// The cards of the row scrolled last, sliding to their new places.
    slide: scroll::Slide,
    pub(super) objects: Objects,
    placed: Placed,
    /// The record of the face each card last showed when it was held up to be read, by slot,
    /// which the Revealed Cards pile draws (`Span::Revealed`).
    pub(super) revealed: Vec<Option<ObjectView>>,
    /// Since when each permanent has fought as its record says, by slot, for the marks of its
    /// own record.
    fights: Fights,
    /// The cards a prompt holds up in its tray, and how they lie and move (`table::tray`).
    pub(super) held: Option<Held>,
    pub(super) tray: TrayMotion,
    /// The cards shown in the flat layer's boxes, and their moves (`table::shown`).
    pub(super) shown: Shown,
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

impl Default for Table {
    fn default() -> Self {
        Self::new(DESKTOP, Window::default())
    }
}

impl Table {
    #[must_use]
    pub fn new(layout: Layout, window: Window) -> Self {
        let camera = Camera::fit(layout.camera, window);
        Self {
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
        }
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

    /// Fits the camera to a window of a new shape and recomputes every resting place.
    pub fn resize(&mut self, window: Window, view: Option<&PlayerView>) {
        self.camera = Camera::fit(self.layout.camera, window);
        self.row_fits = RowFits::new(&self.layout, &self.camera);
        if let Some(view) = view {
            self.apply(view);
        }
    }

    /// Where the object in `slot` rests in the view the table shows, if it shows it.
    #[must_use]
    pub fn rest(&self, slot: usize) -> Option<Pose> {
        Some(self.objects.cards.get(slot).copied().flatten()?.pose)
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
    pub(crate) fn take_placed(&self) -> u32 {
        self.placed.0.take()
    }

    /// Every object's resting place in the view the table shows, which it holds already: the
    /// same as `placement` of that view, for nothing but a copy.
    pub(crate) fn rests(&self) -> Placement {
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
        };
        self.unhold(&mut placement);
        placement
    }

    /// Puts every object of `view` at its resting place, and forgets the objects it no longer
    /// holds, as an update with a reset script must. Cards a prompt holds up rest in its tray.
    pub fn apply(&mut self, view: &PlayerView) {
        let numbers = std::mem::take(&mut self.objects.numbers);
        let placement = self.with_held(self.placement(view, &numbers));
        self.objects = Self::objects_for(view, numbers, &placement);
    }

    /// Exchanges the scene objects for `objects`, which `objects_for` made for the view the
    /// table now shows. Nothing is allocated, so a script can end inside a frame.
    /// The cards a prompt holds up stay in its tray.
    pub(crate) fn swap_objects(&mut self, objects: &mut Objects) {
        std::mem::swap(&mut self.objects, objects);
        self.rehold();
    }

    /// Writes a resolved element and a draw record for every visible card, with a contact
    /// shadow for each card above what lies under it, and one draw record for the cloth, which
    /// comes last among the opaque records. `view` is the view the table shows. While a script
    /// plays, `motion` gives the objects that move their poses, and the records of the view the
    /// script leads to. A permanent wears the marks its record calls for (`own`). Each card is
    /// drawn from the board-size layer of its face in `faces`, or layer 0 while that face has
    /// none or the viewer does not know the card's name, and its back from the card back's
    /// layer. How each card looks is then changed by `dress`: the effects on it, the marks on
    /// it and the pointer, in that order.
    pub fn write(
        &self,
        view: &PlayerView,
        motion: Option<Motion<'_>>,
        faces: &Faces,
        dress: &Dress<'_>,
        builder: &mut FrameBuilder,
    ) {
        let back = faces.board_layer_of(CARD_BACK).unwrap_or(0);
        builder.camera(self.camera.view_projection);
        let slots = motion
            .map_or(0, |motion| motion.tracks.len())
            .max(self.objects.cards.len());
        for slot in 0..slots {
            builder.visit();
            let Some((mut object, drawn, mut under_way)) =
                self.object_at(slot, view, motion, builder)
            else {
                self.fights.forget(slot);
                continue;
            };
            // A card at rest in a row that scrolled slides to its new place, as one does into
            // or out of a prompt's tray.
            under_way = under_way || self.slide_look(slot, &mut object, dress.now);
            under_way = under_way || self.tray_look(slot, &mut object, dress.now);
            under_way = self.shown_look(slot, &mut object, dress.now) || under_way;
            if !object.visible {
                self.fights.forget(slot);
                continue;
            }
            self.write_card(
                slot,
                (&object, drawn, under_way),
                (view, faces),
                (back, dress),
                builder,
            );
        }
        self.write_piles(builder);
        self.write_tray_library(back, builder);
        self.write_abilities(view, motion, faces, (back, dress), builder);
        self.write_revealed(faces, (back, dress), builder);
        let cloth = Affine3A::from_scale_rotation_translation(
            Vec3::new(CLOTH / CARD_WIDTH, CLOTH / CARD_HEIGHT, 1.0),
            glam::Quat::IDENTITY,
            Vec3::new(0.0, 0.0, CLOTH_HEIGHT),
        );
        builder.draw(Band::Opaque, Draw::table(cloth, CLOTH_TINT));
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
        let turn = motion.and_then(|motion| {
            let at = motion
                .turns
                .iter()
                .filter(|turn| turn.slot == slot)
                .map(|turn| turn.at)
                .min()?;
            Some((motion, at))
        });
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
        // A card a row has scrolled out of view rests undrawn at the row's end, above the last
        // card shown, as does a card under the top two of a pile. One out of view before the
        // script and after it stays undrawn while it slides, or it would cover that card. A
        // card that changes place is drawn on its way, wherever it starts or ends, and so is one
        // a step's own verb moves, such as a flight to the end of its row: it leaves from the
        // row's nearer end, where it rests undrawn. Before its
        // track first moves it, a card is drawn as where it rests is; once its track has ended,
        // as where the script leaves it is.
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
            .filter(|rest| !begun && !pointer::takes_pointer(rest.place))
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
    /// `view` shows it, moving if `under_way`. Its face comes from `faces`, its back from layer
    /// `back`, and how it looks is changed by `dress`. `shown_view` is the view the table shows
    /// before the playing script, whose face a `cover` shows until half way.
    fn write_card(
        &self,
        slot: usize,
        (object, view, under_way): (&SceneObject, &PlayerView, bool),
        (shown_view, faces): (&PlayerView, &Faces),
        (back, dress): (u16, &Dress<'_>),
        builder: &mut FrameBuilder,
    ) {
        let shown = &view.objects[object.view_index as usize];
        let name = shown.name.as_deref().unwrap_or("");
        let face = face_of(faces, shown);
        let listed = shown
            .token_printing
            .as_deref()
            .and_then(|printing| faces.face(printing))
            .or_else(|| faces.face(name));
        let mut state = 0;
        if object.tapped {
            state |= StateBits::TAPPED;
        }
        if object.pose.turn > 0.5 {
            state |= StateBits::FACE_DOWN;
        }
        if under_way {
            state |= StateBits::MOVING;
        }
        state |= u16::from(shown.phased_out) * StateBits::PHASED_OUT;
        state |= u16::from(shown.hidden_pick) * StateBits::PICKED;
        let mut look = Look {
            pose: object.pose,
            tint: [255, 255, 255],
            dissolve: 0.0,
            whiten: 0.0,
            face,
            state,
            // A copy beneath a pile's top peeks out too little to be pressed by itself; the
            // pile is pressed through its top card. Under a tray, only its cards (`pressable`).
            interactive: state & StateBits::FACE_DOWN == 0
                && object.copies > 0
                && !name.is_empty()
                && self.pressable(slot, object.place),
        };
        let on = Subject::Object {
            slot: ObjectId(u16::try_from(slot).expect("slots are u16")),
        };
        let covered = dress
            .effects
            .iter()
            .any(|run| run.effect == Effect::Cover && run.on == on);
        let earlier_face = match self.objects.cards.get(slot).copied().flatten() {
            Some(resting) if covered => {
                face_of(faces, &shown_view.objects[resting.view_index as usize])
            }
            _ => face,
        };
        effects::card::look(dress.effects, on, dress.now, earlier_face, &mut look);
        light::marks::look(dress.lit, on, dress.now, &mut look);
        pointer::look(dress.pointer, slot, (dress.now, &self.camera), &mut look);
        effects::card::write(dress.effects, on, dress.now, &look, builder);
        if shown.zone == ZoneName::Battlefield {
            own::write((slot, shown), &look, &self.fights, dress.now, builder);
        } else {
            self.fights.forget(slot);
        }
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
        if let Some(really) = shown.really.as_deref() {
            builder.really(at, really);
        }
        let on = badges::On {
            pose: look.pose,
            band: if under_way {
                Band::RaisedMarks
            } else {
                Band::TableMarks
            },
            opacity: look.pose.opacity,
        };
        dress
            .badges
            .write((shown, listed, object.copies), &on, builder);
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
        let at = builder.resolved(
            name,
            Resolved {
                id,
                parent: u32::MAX,
                role,
                name: crate::frame::NameRef::default(),
                place,
                corners: self.camera.card_corners(transform),
                depth: self.camera.depth(look.pose.position),
                opacity: look.pose.opacity,
                clip: None,
                interactive: look.interactive,
                state: StateBits(look.state),
                face: look.face,
                copies,
                really: crate::frame::NameRef::default(),
            },
        );
        let opacity = (look.pose.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let [red, green, blue] = look.tint;
        // A card that is fading, breaking up or washed with light is drawn with the blended
        // records, after the opaque ones, among the marks of cards at its height. The opaque
        // pass neither blends nor shows what is behind a record, so a fading or broken card
        // drawn there would darken toward black and hide the cards beneath it. It takes the
        // band its own marks take (`light::marks::band`), or a mark drawn before it would lie
        // under it.
        // A card a box of the flat layer shows is drawn over the flat layer (`table::shown`).
        let band = if role == Role::Card && self.shown.holds(id.0 as usize) {
            Band::LayerCards
        } else if opacity == 255 && look.dissolve <= 0.0 && look.whiten <= 0.0 {
            Band::Opaque
        } else if look.pose.position.z < crate::data::space::light::ON_TABLE_BELOW {
            Band::TableMarks
        } else {
            Band::RaisedMarks
        };
        builder.draw(
            band,
            Draw::card(
                transform,
                [red, green, blue, opacity],
                FaceSize::Board,
                look.face.unwrap_or(0),
                back,
                look.dissolve,
                look.whiten,
            ),
        );
        at
    }

    /// Writes each ability on the stack as a card: those of the view the table shows, and
    /// while a script plays, those it leads to and those it moves on their way.
    fn write_abilities(
        &self,
        view: &PlayerView,
        motion: Option<Motion<'_>>,
        faces: &Faces,
        (back, dress): (u16, &Dress<'_>),
        builder: &mut FrameBuilder,
    ) {
        let shown = self.objects.abilities.iter().map(|(id, _)| *id);
        let finals = motion.map_or(&[][..], |motion| &motion.finals.abilities[..]);
        let moved = motion.map_or(&[][..], |motion| motion.ability_tracks);
        // Each ability once: those shown, then those only the script's view holds, then those
        // only a track holds.
        let finals = finals
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| self.objects.ability(*id).is_none());
        let moved = moved.iter().map(|(id, _)| *id).filter(|id| {
            self.objects.ability(*id).is_none()
                && motion.is_none_or(|motion| motion.finals.ability(*id).is_none())
        });
        for id in shown.chain(finals).chain(moved) {
            builder.visit();
            let Some((object, view, under_way)) = self.ability_at(id, view, motion, builder) else {
                continue;
            };
            if object.visible {
                self.write_ability(
                    id,
                    (&object, view, under_way),
                    faces,
                    (back, dress),
                    builder,
                );
            }
        }
    }

    /// The ability numbered `id` as this frame draws it, as `object_at` gives a card.
    fn ability_at<'v>(
        &self,
        id: u32,
        view: &'v PlayerView,
        motion: Option<Motion<'v>>,
        builder: &mut FrameBuilder,
    ) -> Option<(SceneObject, &'v PlayerView, bool)> {
        let resting = self.objects.ability(id);
        let track = motion.and_then(|motion| {
            let track = ability_track(motion.ability_tracks, id)?;
            (!track.segments().is_empty()).then_some((motion, track))
        });
        let Some((motion, track)) = track else {
            return resting.map(|object| (object, view, false));
        };
        builder.segment();
        let pose = track.pose_at(motion.t);
        let (object, view) = match motion.finals.ability(id) {
            Some(object) => (object, motion.view),
            None => (resting?, view),
        };
        let object = SceneObject {
            pose,
            visible: pose.opacity > 0.0,
            ..object
        };
        Some((object, view, track.moving(motion.t)))
    }

    /// Writes the resolved element and the draw record of the ability numbered `id`: `object`
    /// as `view`'s stack shows it, moving if `under_way`, drawn as a card with the board-size
    /// plate face of the ability (its source's picture wearing a plate that says it), or its
    /// source's face while it has none, or the blank layer 0 when it has no source.
    fn write_ability(
        &self,
        id: u32,
        (object, view, under_way): (&SceneObject, &PlayerView, bool),
        faces: &Faces,
        (back, dress): (u16, &Dress<'_>),
        builder: &mut FrameBuilder,
    ) {
        let StackItem::Ability {
            source,
            name,
            triggered,
            nth,
            text,
            ..
        } = &view.stack[object.view_index as usize]
        else {
            return;
        };
        // The plate face says the ability over its source's picture. An ability whose words are
        // not a paragraph of its source's printed text (`text` is given) has no plate face, and
        // shows its source's face alone.
        let plate = text
            .is_none()
            .then(|| faces.plate_layer(name, *triggered, *nth))
            .flatten();
        let face = source.and_then(|_| plate.or_else(|| faces.board_layer(name)));
        let mut look = Look {
            pose: object.pose,
            tint: [255, 255, 255],
            dissolve: 0.0,
            whiten: 0.0,
            face,
            state: if under_way { StateBits::MOVING } else { 0 },
            interactive: false,
        };
        let on = Subject::Ability { id };
        effects::card::look(dress.effects, on, dress.now, face, &mut look);
        light::marks::look(dress.lit, on, dress.now, &mut look);
        effects::card::write(dress.effects, on, dress.now, &look, builder);
        self.write_shadow((None, object.place), &look.pose, under_way, builder);
        self.write_look(
            name,
            (ElementId(ABILITY_IDS + id % ABILITY_IDS), Role::Ability, 1),
            object.place,
            &look,
            back,
            builder,
        );
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
