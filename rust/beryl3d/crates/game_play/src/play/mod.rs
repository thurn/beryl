//! The script clock: the table plays each view's script before it shows the view
//! (docs/beryl-3d.md, "Verbs", "Timing a script" and "Determinism").
//!
//! - A script is compiled with the timings the session sent beside it (`compile`), and starts at
//!   the `now` of the next frame. It is compiled over the frames that play it, a few steps ahead
//!   of each (`part`, `prepare`): one that arrives with nothing playing or waiting from the frame
//!   that receives it, and one that arrives behind another from the frame in which it starts.
//!   Nothing here reads a duration, a delay or a wait from anywhere but those timings.
//! - Scripts never overlap. An update that arrives while a script plays waits, and its script
//!   starts as the one before ends.
//! - When more than `catch_up.double` updates are waiting as a script starts, it plays twice as
//!   fast, and past `catch_up.quadruple` four times as fast (`data/beryl3d/catch-up.json`), so that
//!   the table catches up with a session that runs ahead of it.
//! - When the window changes shape or a row scrolls, every resting place is worked out again
//!   and the playing script is compiled again for it. A segment under way keeps its start and
//!   duration and takes the new end pose, and its start pose is changed so that the object's
//!   pose at that instant is the same: the card does not jump, and arrives at its new place.
//! - Each script's spans and streaks reach the table's `Lit` once the script's clock reaches
//!   them, and a script with a reset drops every span first. Effects, marks, the pointer and
//!   the shake then change what the table draws.
//! - When asked to (`record`), the show keeps a record of every script it is given, for the
//!   interface checks, and a test can plant a fault in a script before it plays (`plant`).

mod affect;
pub(crate) mod compile;
mod counts;
mod history;
mod hold;
pub(crate) use compile::nanoseconds;
mod hand_order;
mod index;
mod notices;
mod part;
#[cfg(feature = "driver")]
pub mod parts;
mod playing;
pub mod record;
mod relay;
mod resize;
mod scene;
mod scroll;
mod sounds;
pub(crate) mod step;
mod tray;
mod turn;
mod wake;

pub use kit_fx::effects::Reach;
pub(crate) use scene::LightTarget;
pub use scene::Scene;
pub use tray::PlacedLetGo;
pub use turn::TurnChange;

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{Kind, PlayerView, Status};
use beryl_protocol::script::{
    Act, OwnStep, StepTiming, Subject, Table as Played, Turn, Verb, own_timings,
};
use glam::Vec2;

use self::compile::{Compiled, bit, compile};
use self::hand_order::HandOrder;
use self::playing::{Playing, Waiting, timings_of};
use self::record::{Plant, ScriptRecord};
use crate::hud::counts::Counts;
use crate::light::{self, Lit};
use crate::notice::Announced;
use crate::pointer::Pointer;
use game_table::faces::Faces;
use game_table::table::badges::{BadgeSizes, Badges};
use game_table::table::{Dress, Table, ability_track};
use kit_base::data::playback::CatchUp;
use kit_base::pose::Pose;
use kit_frame::frame::FrameBuilder;
use kit_fx::effects::{self, EffectRun};
use kit_text::text::{Glyphs, Text};

/// How many spans, streaks and effects the show holds before holding more allocates.
const LIGHTS: usize = 256;

/// The table and the scripts that lead it from view to view.
pub struct Show {
    table: Table,
    /// The match's faces and their layers, which the table draws each card from.
    faces: Faces,
    /// The view the table shows at rest.
    shown: Option<Rc<GameUpdate>>,
    /// How many views the table has shown at rest, counting `shown` (`views_shown`).
    views_shown: usize,
    playing: Option<Playing>,
    waiting: VecDeque<Waiting>,
    /// Whether a script plays faster while many updates wait (`rate_for`).
    catch_up: bool,
    /// The table the last script given ends with, which the next is played from. The scripts
    /// number the abilities on the stack and later steps name them by those numbers, which a
    /// table made from a view alone does not know.
    played: Option<Played>,
    /// How many steps of the scripts given so far the table could not take.
    unplaced: usize,
    /// How many script steps the show has compiled since `take_compiled` was last called.
    compiled: Cell<u32>,
    /// The playing script compiled again for a window of a new shape, which the next frame
    /// takes over from the old one at its own `now`.
    recompiled: Option<Compiled>,
    /// The verbs played as instant changes so far, one bit per verb as `Verb::ALL` orders them.
    unbuilt: u32,
    /// Those that `next_unbuilt` has returned.
    reported: u32,
    /// How many scripts the show has been given.
    given: usize,
    /// When the last script to end ended, or the last effect run or notice to end after it did,
    /// in nanoseconds since the client started.
    rested: u64,
    /// A record of every script given since `record` was called, in order.
    history: Option<Vec<ScriptRecord>>,
    /// How many scripts the show had been given when `record` was called.
    recorded_from: usize,
    /// The faults to plant in scripts not yet given.
    plants: Vec<Plant>,
    /// The spans and streaks the table shows.
    lit: Lit,
    /// The effects playing, in nanoseconds since the client started, until they end.
    effects: Vec<EffectRun>,
    /// The runs under way in the frame. Keeps future effects out of each card's scan.
    active_effects: Vec<EffectRun>,
    card_effects: Vec<EffectRun>,
    /// The notices shown and to be shown, in real time, until they end.
    notices: Vec<Announced>,
    /// What the pointer is doing on the table.
    pointer: Pointer,
    /// The life totals and pools the table shows.
    counts: Counts,
    /// The turn and step of the view the table last showed at rest (`turn_at`).
    turn: Option<Turn>,
    /// The numbers the badges on permanents are made of.
    badges: Badges,
    /// How many scripts were laid out again as they started (`relay`).
    relays: usize,
    /// Where the let-go of the tray's cards that waited for its script was placed, until the
    /// client takes it (`take_placed_let_go`).
    placed_let_go: Option<PlacedLetGo>,
    /// How a closed prompt's tray letting its cards go is timed (`OwnStep::TrayLetGo`), for the
    /// beat of the script it lets go in, which lasts until it is over (`relay`).
    let_go_timing: Option<StepTiming>,
    /// The time `prepare` was last called for, until a script arrives.
    prepared: Option<u64>,
    /// Whether every script is compiled whole as it arrives (`compile_whole`).
    whole: bool,
    /// Each hand's order of arrival, which every view given is rearranged into.
    hands: HandOrder,
}

impl Show {
    #[must_use]
    pub fn new(table: Table) -> Self {
        Self {
            table,
            faces: Faces::default(),
            shown: None,
            views_shown: 0,
            playing: None,
            waiting: VecDeque::new(),
            catch_up: true,
            played: None,
            unplaced: 0,
            compiled: Cell::new(0),
            recompiled: None,
            unbuilt: 0,
            reported: 0,
            given: 0,
            rested: 0,
            history: None,
            recorded_from: 0,
            plants: Vec::new(),
            lit: Lit::with_capacity(LIGHTS),
            effects: Vec::with_capacity(LIGHTS),
            active_effects: Vec::with_capacity(LIGHTS),
            card_effects: Vec::with_capacity(LIGHTS),
            notices: Vec::with_capacity(notices::NOTICES),
            pointer: Pointer::default(),
            counts: Counts::default(),
            turn: None,
            badges: Badges::default(),
            relays: 0,
            placed_let_go: None,
            let_go_timing: (own_timings().entries.into_iter())
                .find(|entry| entry.step == OwnStep::TrayLetGo)
                .map(|entry| entry.timing),
            prepared: None,
            whole: false,
            hands: HandOrder::new(),
        }
    }

    /// The effects the show holds, each in nanoseconds since the client started: those under
    /// way and those that begin later, until each ends. An effect outlives the script that
    /// began it, and a script with a reset drops them all.
    #[must_use]
    pub fn effects(&self) -> &[EffectRun] {
        &self.effects
    }

    /// The spans and streaks the table shows.
    #[must_use]
    pub fn lit(&self) -> &Lit {
        &self.lit
    }

    /// The spans and streaks the table shows, for the marks and arrows a prompt begins and ends
    /// itself.
    pub fn lit_mut(&mut self) -> &mut Lit {
        &mut self.lit
    }

    /// Advances the pointer animations to the frame at `now`.
    pub fn advance_pointer(&mut self, now: u64) {
        self.pointer.advance(now, &self.table.data().pointer);
    }

    /// The pointer on the table, which takes the host's pointer events.
    pub fn pointer_mut(&mut self) -> &mut Pointer {
        &mut self.pointer
    }

    /// Where `subject` is at `now` and how large: a card where it is drawn (`drawn_pose`), an
    /// ability on the stack where `ability_pose` has it, a player at their anchor
    /// (`Table::player_pose`).
    #[must_use]
    pub fn reach(&self, subject: Subject, now: u64) -> Option<Reach> {
        let card = |pose: Pose| Reach {
            pose,
            half: self.table.camera().data.card.size() * 0.5 * pose.scale,
        };
        match subject {
            Subject::Object { slot } => self.drawn_pose(usize::from(slot.0), now).map(card),
            Subject::Player { player } => Some(Reach {
                pose: self.table.player_pose(player),
                half: Vec2::ZERO,
            }),
            Subject::Ability { id } => self.ability_pose(id, now).map(card),
        }
    }

    /// When the last script, or an effect run that outlasted it, ended, in nanoseconds since the
    /// client started: when the table came to rest, if nothing has played since. However the
    /// frames fell, it is the same, because a reply is due a fixed time after the table rests.
    #[must_use]
    pub fn rested(&self) -> u64 {
        self.rested
    }

    /// When the first notice the show holds ends, in nanoseconds since the client started, if it
    /// holds one. A notice, such as the turn banner, can outlast the script that announced it,
    /// and `wake` asks for frames until it ends without counting it in `rested`.
    #[must_use]
    pub fn notice_end(&self) -> Option<u64> {
        self.notices.iter().map(|announced| announced.over.to).min()
    }

    /// Whether every script given has played out, as of the last `frame`: the table shows the
    /// latest view at rest, and that view's prompt may open.
    #[must_use]
    pub fn settled(&self) -> bool {
        self.playing.is_none() && self.waiting.is_empty()
    }

    /// Compiles `update`'s script, the `number`th the show is given, played from `before` and
    /// `from`, the table the script before it ended with, with any fault planted in it. Returns
    /// it with the table it ends with.
    fn compile(
        &self,
        number: usize,
        before: Option<&PlayerView>,
        from: Option<&Played>,
        update: &GameUpdate,
        timings: &[StepTiming],
    ) -> (Compiled, Played) {
        let plants = self.plants.iter().filter(|plant| plant.script == number);
        let records = self.history.is_some() || plants.clone().next().is_some();
        let shown = before
            .zip(self.shown.as_deref())
            .is_some_and(|(before, shown)| std::ptr::eq(before, &raw const shown.view));
        // Each script given after the first is played from the table the one before ended with;
        // the first has no view before it either.
        let before = before.zip(from);
        let steps = u32::try_from(timings.len()).unwrap_or(u32::MAX);
        self.compiled.set(self.compiled.get().saturating_add(steps));
        let (mut compiled, played) =
            compile(before, &update.view, timings, &self.table, records, shown);
        for plant in plants {
            compiled.plant(plant);
        }
        (compiled, played)
    }

    #[must_use]
    pub fn table(&self) -> &Table {
        &self.table
    }

    /// The match's faces and their layers.
    #[must_use]
    pub fn faces(&self) -> &Faces {
        &self.faces
    }

    pub fn faces_mut(&mut self) -> &mut Faces {
        &mut self.faces
    }

    pub fn badges_mut(&mut self) -> &mut Badges {
        &mut self.badges
    }

    /// Shapes the numbers and signs of the badges at `scale`, once for each set of badge sizes
    /// the data gives; the mana symbols of every cost the face list names that are not shaped
    /// yet, for the costs hand cards show; and the name of every named counter and the lines of
    /// text (`Badges::prepare`) in the latest view not shaped yet. Each view is the latest when
    /// it arrives, so every name a view shows is shaped before a frame shows it. New sizes drop
    /// every shaped name and line, so those of every view still to be shown are shaped again.
    pub fn shape_badges(&mut self, text: &mut Text, wanted: &mut Glyphs, scale: f32) {
        let sizes = BadgeSizes::of(self.table.data());
        let resized = !self.badges.shaped_at(&sizes);
        if resized {
            self.badges.shape(text, wanted, scale, sizes);
        }
        self.badges.shape_costs(text, wanted, self.faces.list());
        // The views the frames may still show, the latest last, with the show's fields
        // borrowed apart from the badges.
        let playing = (self.playing.as_ref()).and_then(|playing| playing.update.as_deref());
        let held = (self.shown.as_deref().into_iter())
            .chain(playing)
            .chain(self.waiting.iter().map(|waiting| &*waiting.update));
        let latest = held.clone().last();
        let mut shape = |update: &GameUpdate| {
            self.badges.shape_names(text, wanted, &update.view.objects);
            self.badges.prepare(&update.view, text, wanted);
        };
        if resized {
            held.for_each(&mut shape);
        } else if let Some(update) = latest {
            shape(update);
        }
    }

    /// The latest view: the last to arrive, which the table shows once every script before it
    /// has played.
    #[must_use]
    pub fn latest(&self) -> Option<&GameUpdate> {
        self.waiting
            .back()
            .map(|waiting| &*waiting.update)
            .or(self
                .playing
                .as_ref()
                .and_then(|playing| playing.update.as_deref()))
            .or(self.shown.as_deref())
    }

    /// How many steps of the scripts given so far the table played over could not take. Each
    /// made the step after it place the whole table, so that its objects jumped to their places
    /// instead of moving there.
    #[must_use]
    pub fn unplaced(&self) -> usize {
        self.unplaced
    }

    /// How many script steps the show has compiled since the last call, and starts counting
    /// again. A script played again for a window of a new shape counts its steps again.
    pub fn take_compiled(&self) -> u32 {
        self.compiled.take()
    }

    /// The view the table shows at rest: that of the last script to end.
    #[must_use]
    pub fn shown(&self) -> Option<&GameUpdate> {
        self.shown.as_deref()
    }

    /// How many views the table has shown at rest so far: a number that changes whenever
    /// `shown` does, so that what is drawn from it knows to be laid out again.
    #[must_use]
    pub fn views_shown(&self) -> usize {
        self.views_shown
    }

    /// Where the object in `slot` is at `now`, as the last frame left the clock: on its track
    /// while the playing script moves it, and otherwise where it rests in the view shown.
    #[must_use]
    pub fn pose(&self, slot: usize, now: u64) -> Option<Pose> {
        if let Some(playing) = &self.playing
            && playing.update.is_some()
            && let Some(track) = playing.compiled.tracks.get(slot)
            && !track.segments().is_empty()
        {
            return Some(track.pose_at(playing.clock(now)));
        }
        self.table.rest(slot)
    }

    /// Where the object in `slot` is drawn at `now`: where `pose` puts it, moved as the table
    /// draws it by a row's slide, the tray's move and the box that shows it
    /// (`Table::drawn_pose`). A mark, arrow or effect on a card goes with the card, instead of
    /// staying where its track or its rest is while the card stands in a box or flies to the
    /// tray.
    fn drawn_pose(&self, slot: usize, now: u64) -> Option<Pose> {
        Some(self.table.drawn_pose(slot, self.pose(slot, now)?, now))
    }

    /// Where the ability numbered `id` is at `now`, as `pose` gives a card's: on its track while
    /// the playing script moves it, and otherwise where it rests on the stack of the view shown.
    #[must_use]
    pub fn ability_pose(&self, id: u32, now: u64) -> Option<Pose> {
        if let Some(playing) = &self.playing
            && playing.update.is_some()
            && let Some(track) = ability_track(&playing.compiled.ability_tracks, id)
            && !track.segments().is_empty()
        {
            return Some(track.pose_at(playing.clock(now)));
        }
        self.table.ability_rest(id)
    }

    /// How many scripts the show has been given: the number the next script given will have.
    #[must_use]
    pub fn given(&self) -> usize {
        self.given
    }

    /// How many updates wait for the playing script to end.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Takes an update and the timings of its script's steps, one per step in the order they are
    /// played. It is compiled now, against the view the table will show when its turn comes, and
    /// plays after every script that arrived before it.
    pub fn push(&mut self, update: Box<GameUpdate>, timings: Vec<StepTiming>) {
        self.push_timed(update, Some(timings));
    }

    /// Takes a session update using the timings it carries beside the script.
    pub fn push_update(&mut self, update: Box<GameUpdate>) {
        if let Some(script) = &update.view.script {
            debug_assert_eq!(
                script.timing.len(),
                script.steps(),
                "the session sends one timing per step"
            );
        }
        self.push_timed(update, None);
    }

    fn push_timed(&mut self, mut update: Box<GameUpdate>, timings: Option<Vec<StepTiming>>) {
        self.hands.arrange(&mut update.view);
        self.prepared = None;
        let number = self.given;
        self.given += 1;
        // The faces the script holds up to be read, for the Revealed Cards pile to draw once
        // their steps play.
        let scripted = update.view.script.iter().flat_map(|script| &script.beats);
        for step in scripted.flatten() {
            if let Act::Showcase { object } = &step.act {
                self.table.reveal(object);
            }
        }
        let update: Rc<GameUpdate> = Rc::from(update);
        let timings: Option<Rc<[StepTiming]>> = timings.map(Rc::from);
        let from = self.played.take().map(Rc::new);
        // A script that arrives behind another is compiled as it starts (`part`).
        if !self.whole && !self.settled() {
            let waiting = self.defer(number, update, timings, from);
            self.waiting.push_back(waiting);
            return;
        }
        if let Some(waiting) = self.in_parts(number, &update, timings.as_ref(), from.as_ref()) {
            self.waiting.push_back(waiting);
            return;
        }
        let (compiled, played) = self.compile(
            number,
            self.before_next(),
            from.as_deref(),
            &update,
            timings_of(&update, timings.as_deref()),
        );
        self.played = Some(played);
        self.unplaced += compiled.unplaced;
        self.write_record(number, &compiled);
        self.waiting.push_back(Waiting {
            number,
            update,
            timings,
            from,
            compiled,
            part: None,
            pending: false,
        });
    }

    /// The view the next update to arrive is played from: the last one waiting, or playing, or
    /// shown.
    fn before_next(&self) -> Option<&PlayerView> {
        self.latest().map(|update| &update.view)
    }

    /// Drops the effect runs and notices that have ended by `now`. A run is under way from its
    /// start until, not at, its end, as on the web stage. The table is not at rest while a run
    /// or a notice is under way (`wake`), so one that outlasts its script moves `rested` to its
    /// end: a reply due after the rest is then given at the same time however often frames are
    /// drawn.
    fn forget_ended(&mut self, now: u64) {
        let mut rested = self.rested;
        self.effects.retain(|run| {
            let end = run.start + run.duration;
            if end <= now {
                rested = rested.max(end);
            }
            end > now
        });
        self.notices.retain(|announced| {
            if announced.over.to <= now {
                rested = rested.max(announced.over.to);
            }
            announced.over.to > now
        });
        self.rested = rested;
    }

    /// Advances the clock to `now`, ending each script whose time is up and starting the next,
    /// and writes the table as it is at `now`. Allocates nothing.
    pub fn frame(&mut self, now: u64, builder: &mut FrameBuilder) {
        self.prepare(now);
        self.take_recompiled(now);
        // When the script before ended, so that the next starts then and not at a later frame.
        let mut ended = None;
        loop {
            if let Some(playing) = &self.playing {
                let end = playing.compiled.end;
                if playing.clock(now) < end {
                    break;
                }
                let real = playing.real(end);
                ended = Some(real);
                self.rested = real;
                if let Some(playing) = &mut self.playing {
                    playing.give(real, (&mut self.lit, &mut self.counts), builder);
                    self.lit.script_ended(end, real - playing.start);
                }
                self.finish();
            }
            let start = ended.unwrap_or(now);
            let rate = if self.catch_up {
                rate_for(self.waiting.len(), self.table.data().catch_up)
            } else {
                1
            };
            // Begun in `prepare` unless it did not foresee this start; this may allocate.
            self.begin(0, now, (start, rate));
            let Some(mut waiting) = self.waiting.pop_front() else {
                break;
            };
            self.unbuilt |= waiting.compiled.unbuilt;
            let moves =
                !waiting.compiled.tracks.is_empty() || !waiting.compiled.ability_tracks.is_empty();
            if waiting.compiled.reset {
                self.lit.clear();
                self.effects.clear();
                self.notices.clear();
            }
            self.relay(&mut waiting, start, rate);
            self.record_start(waiting.number, start, rate);
            self.playing = Some(Playing::started(waiting, start, rate));
            self.hand_over(now);
            self.hand_over_tray();
            self.place_let_go(now);
            if let Some(playing) = &mut self.playing {
                playing.affect(&mut self.effects);
                playing.announce(&mut self.notices, self.table.data().notices);
            }
            if !moves {
                self.show_playing();
            }
        }
        self.place_let_go(now);
        if let Some(playing) = &mut self.playing {
            playing.give(now, (&mut self.lit, &mut self.counts), builder);
        }
        self.lit.advance(now);
        self.forget_ended(now);
        let Some(shown) = self.shown.as_ref().map(|update| &update.view) else {
            return;
        };
        let motion = self
            .playing
            .as_ref()
            .and_then(|playing| playing.motion(now));
        let active = &mut self.active_effects;
        active.clear();
        active.extend(
            self.effects
                .iter()
                .filter(|run| run.start <= now && now < run.start + run.duration)
                .cloned(),
        );
        self.card_effects.clear();
        self.card_effects.extend(
            active
                .iter()
                .filter(|run| run.definition.on_card())
                .cloned(),
        );
        let data = self.table.data();
        let (pointer, tuning) = (&self.pointer, &data.pointer);
        let dress = Dress {
            effects: &self.card_effects,
            badges: &self.badges,
            marks: &|on, look| light::marks::look(&data.light, &self.lit, on, now, look),
            pointer: &|slot, camera, look| {
                crate::pointer::look(pointer, slot, (now, camera), tuning, look);
            },
            now,
        };
        let revealed = self.lit.open().iter().filter_map(|open| match open.span {
            light::Span::Revealed { slot, .. } if open.ends.is_none() => Some((slot, open.begins)),
            _ => None,
        });
        self.table
            .write(shown, motion, &self.faces, &dress, revealed, builder);
        // The view shown, its scripts played out, asks something other than priority, as the
        // web stage's `#prompt` says: the line from an ability to its source is hidden then.
        let prompt = self.settled()
            && matches!(&shown.status, Status::Deciding { decision }
                if decision.kind != Kind::Priority);
        self.write_light_and_effects(now, prompt, builder);
    }

    /// Draws the script's arrows, marks and effects over the written table.
    fn write_light_and_effects(&self, now: u64, prompt: bool, builder: &mut FrameBuilder) {
        let scene = Scene::new(self, now);
        scene.register_light_targets(builder);
        light::arrows::write(&self.lit, now, prompt, &scene, builder);
        light::marks::write(&self.lit, now, &scene, builder);
        builder.clear_owner();
        light::streaks::write(&self.lit, now, &scene, builder);
        let light_target = |target, ago| {
            let subject = game_table::effect::subject(target)?;
            let mut reach = scene.reach_before(subject, ago)?;
            if ago == 0 && matches!(subject, Subject::Player { .. }) {
                let table = scene.table();
                let card = table.camera().data.card;
                let window = reach.pose.scale * card.height / table.layout().hand.height;
                reach.half = table.data().light.player_half_pixels * (window / 900.0);
            }
            Some(reach)
        };
        let camera = self.table.camera();
        effects::write(
            &self.table.data().effects,
            &self.active_effects,
            now,
            light_target,
            (camera.eye, camera.window.scale),
            builder,
        );
    }

    /// Ends the playing script: the table shows its view, at rest, with its life totals and
    /// pools. A script compiled over frames is whole by its end.
    fn finish(&mut self) {
        debug_assert!(
            self.playing
                .as_ref()
                .is_none_or(|playing| playing.part.is_none()),
            "a script compiled over frames does not end before it is whole"
        );
        self.show_playing();
        self.playing = None;
        if let Some(shown) = &self.shown {
            self.counts.take_view(&shown.view);
            self.turn = Some(playing::turn_of(&shown.view));
        }
    }

    /// The table shows the playing script's view at rest, if it does not already.
    fn show_playing(&mut self) {
        if let Some(playing) = &mut self.playing
            && let Some(update) = playing.update.take()
        {
            self.table.swap_objects(&mut playing.compiled.finals);
            // A script the table takes at once shows its view's step as it starts.
            if playing.compiled.immediate {
                self.turn = Some(playing::turn_of(&update.view));
            }
            self.shown = Some(update);
            self.views_shown += 1;
        }
    }

    /// Takes over the playing script compiled again for a new window: each segment under way at
    /// `now` keeps the object where the old one had it.
    fn take_recompiled(&mut self, now: u64) {
        let Some(mut compiled) = self.recompiled.take() else {
            return;
        };
        let Some(playing) = &mut self.playing else {
            return;
        };
        relay::follow(&mut compiled, &playing.compiled);
        let t = playing.clock(now);
        let old = &playing.compiled.tracks;
        for (slot, track) in compiled.tracks.iter_mut().enumerate() {
            let Some(was) = old.get(slot).filter(|old| !old.segments().is_empty()) else {
                continue;
            };
            let pose = was.pose_at(t);
            let begun = track
                .segments()
                .partition_point(|segment| segment.start <= t);
            if let Some(latest) = begun.checked_sub(1) {
                track.segments_mut()[latest].through(t, pose);
            }
            // The same segment in the record of the script's segments: the last added of those
            // that begin latest by `t`.
            let latest = compiled
                .segments
                .iter_mut()
                .filter(|record| record.slot == slot && record.segment.start <= t)
                .reduce(|kept, next| {
                    if next.segment.start >= kept.segment.start {
                        next
                    } else {
                        kept
                    }
                });
            if let Some(record) = latest {
                record.segment.through(t, pose);
            }
        }
        for (id, track) in &mut compiled.ability_tracks {
            let Some(was) = ability_track(&playing.compiled.ability_tracks, *id)
                .filter(|old| !old.segments().is_empty())
            else {
                continue;
            };
            let pose = was.pose_at(t);
            let begun = track
                .segments()
                .partition_point(|segment| segment.start <= t);
            if let Some(latest) = begun.checked_sub(1) {
                track.segments_mut()[latest].through(t, pose);
            }
        }
        let number = playing.number;
        playing.compiled = compiled;
        if let Some(playing) = self.playing.take() {
            self.write_record(number, &playing.compiled);
            self.playing = Some(playing);
        }
    }

    /// The verbs of the script under way, one bit per verb in the order of `Verb::ALL`, or 0
    /// when none plays.
    #[must_use]
    pub fn playing_verbs(&self) -> u32 {
        self.playing
            .as_ref()
            .map_or(0, |playing| playing.compiled.verbs)
    }

    /// The next verb that a script played as an instant change because it is not built yet,
    /// each verb returned once, for the host to log.
    pub fn next_unbuilt(&mut self) -> Option<Verb> {
        let verb = Verb::ALL
            .iter()
            .copied()
            .find(|verb| self.unbuilt & bit(*verb) != 0 && self.reported & bit(*verb) == 0)?;
        self.reported |= bit(verb);
        Some(verb)
    }
}

/// How many times faster than real time a script plays as it starts with `waiting` scripts,
/// itself among them, waiting to play: twice past `catch_up.double` and four times past
/// `catch_up.quadruple`, so that the table catches up with a session that runs ahead of it.
fn rate_for(waiting: usize, catch_up: CatchUp) -> u64 {
    match waiting {
        n if n > catch_up.quadruple => 4,
        n if n > catch_up.double => 2,
        _ => 1,
    }
}
