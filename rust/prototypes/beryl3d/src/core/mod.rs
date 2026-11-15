//! The client core of the prototype: it turns session updates, pointer input and a time into a
//! list of 64-byte draw records. Nothing here reads a clock, starts a thread, takes a lock, opens
//! a file or names a wgpu type, which is the split docs/beryl-3d.md puts on trial. Time is the
//! `now` argument of [`Core::frame`], in nanoseconds.

pub mod camera;
pub mod draw;
pub mod motion;
pub mod place;
pub mod play;
pub mod stress;
pub mod timing;

use std::collections::{BTreeMap, VecDeque};

use beryl_session::GameUpdate;
use beryl_session::protocol::Status;
use glam::Vec2;

use camera::Camera;
pub use draw::Draw;
use motion::{Pose, Track};
use place::Placement;
use stress::Stress;
use timing::Timings;

/// The most objects the core makes room for: the engine's large form holds 1,024.
pub const MAX_OBJECTS: usize = 1100;
/// Texture array layers with a fixed meaning: the card back, and a face with no scan.
pub const LAYER_BACK: u16 = 0;
pub const LAYER_UNKNOWN: u16 = 1;

/// Which texture layer shows each card name.
#[derive(Clone, Default)]
pub struct Faces {
    pub by_name: BTreeMap<String, u16>,
    /// Face layers after the two fixed ones.
    pub layers: u16,
}

impl Faces {
    fn layer(&self, name: Option<&str>) -> u16 {
        name.and_then(|n| self.by_name.get(n).copied())
            .unwrap_or(LAYER_UNKNOWN)
    }
}

/// The zones the prototype places, a subset of the session's `ZoneName`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Exile,
    Stack,
    Nowhere,
}

/// The part of an object's view the prototype draws from. Kept small and `Copy` so that a
/// script can be played without cloning the session's records, whose vectors would allocate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rec {
    pub zone: Zone,
    /// The viewer's side (true) or the opponent's: the controller on the battlefield, the owner
    /// elsewhere.
    pub you: bool,
    pub face_up: bool,
    pub tapped: bool,
    pub land: bool,
    pub layer: u16,
    /// Position within its zone: objects are laid out in increasing order.
    pub order: u32,
}

impl Rec {
    const NONE: Rec = Rec {
        zone: Zone::Nowhere,
        you: true,
        face_up: false,
        tapped: false,
        land: false,
        layer: LAYER_BACK,
        order: 0,
    };
}

pub struct Obj {
    pub rec: Rec,
    pub track: Track,
}

pub enum Input {
    Update(Box<GameUpdate>),
    /// The window's size in physical pixels.
    Surface {
        width: u32,
        height: u32,
    },
    /// The pointer in normalized device coordinates, or none when it left the window.
    Pointer(Option<Vec2>),
}

/// When the core next needs a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    Now,
    At(u64),
    Never,
}

pub struct Core {
    pub camera: Camera,
    timing: Timings,
    base_timing: Timings,
    faces: Faces,
    objs: Vec<Obj>,
    placement: Placement,
    scratch: play::Scratch,
    pending: VecDeque<Box<GameUpdate>>,
    /// Updates that have arrived but not yet started; dropped at the next frame, which only
    /// frees memory.
    finished: Vec<Box<GameUpdate>>,
    /// When the script being played ends.
    busy_until: u64,
    /// When the last segment of any object ends.
    motion_until: u64,
    next_order: u32,
    stress: Option<Stress>,
    pointer: Option<Vec2>,
    pointer_moved: bool,
    resized: bool,
    hover: Option<u16>,
    last_now: u64,
    /// The latest update received says the AI is searching.
    pub thinking: bool,
    /// The core has drawn at least one update of a game.
    pub has_game: bool,
    pub draw: draw::DrawList,
}

impl Core {
    pub fn new(width: u32, height: u32, timing: Timings, faces: Faces) -> Core {
        let mut objs = Vec::with_capacity(MAX_OBJECTS);
        for _ in 0..MAX_OBJECTS {
            objs.push(Obj {
                rec: Rec::NONE,
                track: Track::default(),
            });
        }
        Core {
            camera: Camera::new(width, height),
            timing,
            base_timing: timing,
            faces,
            objs,
            placement: Placement::new(),
            scratch: play::Scratch::new(MAX_OBJECTS),
            pending: VecDeque::with_capacity(4096),
            finished: Vec::with_capacity(4096),
            busy_until: 0,
            motion_until: 0,
            next_order: 1,
            stress: None,
            pointer: None,
            pointer_moved: false,
            resized: false,
            hover: None,
            last_now: 0,
            thinking: false,
            has_game: false,
            draw: draw::DrawList::new(),
        }
    }

    /// A core showing a stress board of `cards` cards instead of a game.
    pub fn stress(width: u32, height: u32, timing: Timings, faces: Faces, cards: usize) -> Core {
        let layers = faces.layers.max(1);
        let mut core = Core::new(width, height, timing, faces);
        let stress = Stress::new(cards, layers);
        stress.setup(&mut core.objs);
        core.stress = Some(stress);
        core.draw.shadow_height = 45.0;
        core
    }

    /// Applies an input. Inputs have no time of their own: what they start begins at the `now`
    /// of the next [`Core::frame`].
    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Update(update) => {
                self.thinking = matches!(update.view.status, Status::Thinking);
                self.pending.push_back(update);
            }
            Input::Surface { width, height } => {
                self.camera.fit(width, height);
                self.resized = true;
            }
            Input::Pointer(pointer) => {
                self.pointer = pointer;
                self.pointer_moved = true;
            }
        }
    }

    /// Updates waiting for the script before them to end.
    pub fn queued(&self) -> usize {
        self.pending.len()
    }

    /// Builds the frame at `now`.
    pub fn frame(&mut self, now: u64) -> &draw::DrawList {
        self.finished.clear();
        while now >= self.busy_until {
            let Some(update) = self.pending.pop_front() else {
                break;
            };
            let start = self.busy_until.max(self.last_now);
            // The stand-in answers the human seat at once, so the session can run far ahead
            // of the animation. A backlog plays faster until it has caught up.
            let speed = match self.pending.len() {
                0..10 => 1,
                10..40 => 2,
                _ => 4,
            };
            self.timing = self.base_timing.scaled(speed);
            self.busy_until = self.play(&update, start, now);
            self.has_game = true;
            self.finished.push(update);
        }
        if let Some(stress) = &mut self.stress {
            let end = stress.step(now, &mut self.objs);
            self.motion_until = self.motion_until.max(end);
        }
        if self.resized {
            self.resized = false;
            self.settle(now);
        }
        if self.pointer_moved {
            self.pointer_moved = false;
            self.update_hover(now);
        }
        self.last_now = now;
        self.draw.build(&self.objs, &self.camera, now);
        &self.draw
    }

    /// The same frame with every record blended back to front and no depth buffer, to compare
    /// with the frame as built.
    pub fn frame_naive(&mut self, now: u64) -> &[Draw] {
        self.draw.build_naive(&self.objs, &self.camera, now);
        &self.draw.naive
    }

    pub fn wake(&self, now: u64) -> Wake {
        if self.stress.is_some()
            || now < self.motion_until
            || (!self.pending.is_empty() && now >= self.busy_until)
        {
            Wake::Now
        } else if now < self.busy_until {
            Wake::At(self.busy_until)
        } else {
            Wake::Never
        }
    }

    /// Whether anything moves at `now`.
    pub fn animating(&self, now: u64) -> bool {
        self.stress.is_some() || now < self.motion_until
    }

    /// Adds a segment to object `id`'s track.
    fn push(&mut self, id: usize, segment: motion::Segment, now: u64) {
        self.motion_until = self.motion_until.max(segment.end());
        self.objs[id].track.push(segment, now);
    }

    /// Moves every object that has nothing scheduled to its place for the current camera,
    /// after the window changed shape.
    fn settle(&mut self, now: u64) {
        self.placement.place(&self.objs, &self.camera);
        for id in 0..self.objs.len() {
            let obj = &self.objs[id];
            if obj.rec.zone == Zone::Nowhere || obj.track.last_start() > now {
                continue;
            }
            let rest = self.placement.rest[id];
            self.push(id, motion::Segment::instant(now, rest), now);
        }
    }

    fn update_hover(&mut self, now: u64) {
        let picked = self.pointer.and_then(|p| self.pick(p, now));
        if picked == self.hover {
            return;
        }
        let previous = self.hover;
        self.hover = picked;
        self.placement.hover = picked;
        self.placement.place(&self.objs, &self.camera);
        for id in [previous, picked].into_iter().flatten() {
            let id = usize::from(id);
            // A card that a script will move later keeps that motion. Any other card goes to
            // its new pose at once, even if its last lift has not finished: waiting for it to
            // finish left cards enlarged when the pointer crossed the hand quickly.
            if self.objs[id].track.last_start() > now {
                continue;
            }
            let from = self.objs[id].track.pose_at(now);
            let to = self.placement.rest[id];
            let segment = motion::Segment::new(now, self.timing.hover, from, to, 0.0);
            self.push(id, segment, now);
        }
    }

    /// The viewer's hand card under the pointer, nearest the camera, by a ray against each
    /// card's plane.
    fn pick(&self, pointer: Vec2, now: u64) -> Option<u16> {
        let (origin, direction) = self.camera.ray(pointer);
        let mut best: Option<(f32, u16)> = None;
        for (id, obj) in self.objs.iter().enumerate() {
            if obj.rec.zone != Zone::Hand || !obj.rec.you {
                continue;
            }
            let pose: Pose = obj.track.pose_at(now);
            let inverse = pose.matrix().inverse();
            let o = inverse.transform_point3(origin);
            let d = inverse.transform_vector3(direction);
            if d.z.abs() < 1e-6 {
                continue;
            }
            let t = -o.z / d.z;
            let hit = o + d * t;
            if t > 0.0
                && hit.x.abs() <= draw::CARD_W / 2.0
                && hit.y.abs() <= draw::CARD_H / 2.0
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, id as u16));
            }
        }
        best.map(|(_, id)| id)
    }
}
