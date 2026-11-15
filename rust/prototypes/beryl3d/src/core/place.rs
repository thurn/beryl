//! Placement: for the objects' records and the camera, the pose every object rests in. One
//! function per zone. Units are millimetres, the table is z = 0, x runs to the viewer's right and
//! y away from the viewer. Positions follow the web interface's table (ui/src/game/battlefield.ts
//! and handLayout.ts) and Arena's recordings by eye: the viewer's library at the bottom left,
//! the opponent's at the top right, the stack at the right.

use std::f32::consts::PI;

use glam::{Quat, Vec2, Vec3};

use super::draw::{CARD_H, CARD_W};
use super::motion::Pose;
use super::{MAX_OBJECTS, Obj, Zone};

/// Height between two cards lying on each other.
pub const LAYER: f32 = 0.3;
/// The width a battlefield row may fill, and the gap between cards that fit.
const ROW_WIDTH: f32 = 760.0;
const ROW_LEFT: f32 = -380.0;
const GAP: f32 = 4.0;
/// Each row's centre line: the viewer's lands, the viewer's other permanents, then the
/// opponent's other permanents and lands.
const ROWS: [f32; 4] = [-158.0, -60.0, 60.0, 158.0];
const LIBRARY: Vec2 = Vec2::new(-440.0, -160.0);
const GRAVEYARD: Vec2 = Vec2::new(-440.0, -62.0);
const EXILE: Vec2 = Vec2::new(-440.0, 36.0);
const STACK: Vec3 = Vec3::new(380.0, -20.0, 40.0);
/// The viewer's hand: depth in front of the eye, card height as a fraction of the screen.
const HAND_DEPTH: f32 = 300.0;
const HAND_HEIGHT: f32 = 0.24;
const OPPONENT_HAND_HEIGHT: f32 = 0.16;
/// How much larger the hand card under the pointer is drawn.
const HOVER_SCALE: f32 = 1.35;

#[derive(Clone, Copy)]
enum Group {
    Library,
    Hand,
    Lands,
    Others,
    Graveyard,
    Exile,
}

const GROUPS: usize = 6;

pub struct Placement {
    pub rest: Vec<Pose>,
    /// The viewer's hand card under the pointer, which lifts and enlarges.
    pub hover: Option<u16>,
    groups: [[Vec<u16>; GROUPS]; 2],
    stack: Vec<u16>,
}

impl Placement {
    pub fn new() -> Placement {
        let group = || Vec::with_capacity(MAX_OBJECTS);
        Placement {
            rest: vec![Pose::HIDDEN; MAX_OBJECTS],
            hover: None,
            groups: [
                std::array::from_fn(|_| group()),
                std::array::from_fn(|_| group()),
            ],
            stack: group(),
        }
    }

    /// Fills [`Placement::rest`] for every object. Allocates nothing.
    pub fn place(&mut self, objs: &[Obj], camera: &super::camera::Camera) {
        for side in &mut self.groups {
            for group in side.iter_mut() {
                group.clear();
            }
        }
        self.stack.clear();
        for (id, obj) in objs.iter().enumerate() {
            let rec = obj.rec;
            let side = usize::from(!rec.you);
            let group = match rec.zone {
                Zone::Library => Group::Library,
                Zone::Hand => Group::Hand,
                Zone::Battlefield if rec.land => Group::Lands,
                Zone::Battlefield => Group::Others,
                Zone::Graveyard => Group::Graveyard,
                Zone::Exile => Group::Exile,
                Zone::Stack => {
                    self.stack.push(id as u16);
                    continue;
                }
                Zone::Nowhere => {
                    self.rest[id] = Pose::HIDDEN;
                    continue;
                }
            };
            self.groups[side][group as usize].push(id as u16);
        }
        let order = |id: &u16| objs[usize::from(*id)].rec.order;
        for side in &mut self.groups {
            for group in side.iter_mut() {
                group.sort_unstable_by_key(order);
            }
        }
        self.stack.sort_unstable_by_key(order);
        for side in 0..2 {
            let you = side == 0;
            let mirror = if you { 1.0 } else { -1.0 };
            for (index, group) in self.groups[side].iter().enumerate() {
                let ids = group.as_slice();
                match index {
                    0 => pile(&mut self.rest, objs, ids, LIBRARY * mirror),
                    1 if you => hand(&mut self.rest, ids, camera, self.hover),
                    1 => opponent_hand(&mut self.rest, ids, camera),
                    2 => row(&mut self.rest, objs, ids, ROWS[if you { 0 } else { 3 }]),
                    3 => row(&mut self.rest, objs, ids, ROWS[if you { 1 } else { 2 }]),
                    4 => pile(&mut self.rest, objs, ids, GRAVEYARD * mirror),
                    _ => pile(&mut self.rest, objs, ids, EXILE * mirror),
                }
            }
        }
        for (i, &id) in self.stack.iter().enumerate() {
            let step = i as f32;
            let position = STACK + Vec3::new(-step * 12.0, -step * 16.0, step * 4.0);
            self.rest[usize::from(id)] =
                Pose::at(position, facing(objs[usize::from(id)].rec.face_up));
        }
    }
}

/// A card lying on the table face up, or face down with its back toward the camera.
fn facing(face_up: bool) -> Quat {
    if face_up {
        Quat::IDENTITY
    } else {
        Quat::from_rotation_y(PI)
    }
}

fn pile(rest: &mut [Pose], objs: &[Obj], ids: &[u16], at: Vec2) {
    for (i, &id) in ids.iter().enumerate() {
        let rec = objs[usize::from(id)].rec;
        let position = at.extend(LAYER * (i as f32 + 0.5));
        rest[usize::from(id)] = Pose::at(position, facing(rec.face_up));
    }
}

/// A battlefield row at full size while it fits, then overlapping, each card a layer above the
/// one it covers so that which is on top is a matter of position.
fn row(rest: &mut [Pose], objs: &[Obj], ids: &[u16], y: f32) {
    let n = ids.len();
    if n == 0 {
        return;
    }
    let full = CARD_W + GAP;
    let step = if n == 1 {
        0.0
    } else {
        full.min((ROW_WIDTH - CARD_H) / (n - 1) as f32)
    };
    let span = step * (n - 1) as f32;
    let left = ROW_LEFT + (ROW_WIDTH - span) / 2.0;
    for (i, &id) in ids.iter().enumerate() {
        let rec = objs[usize::from(id)].rec;
        // Tapped cards are wider than the step, so every card lies a layer above the one
        // before it whether or not the row overlaps.
        let z = LAYER * i as f32;
        let mut rotation = facing(rec.face_up);
        if rec.tapped {
            rotation = Quat::from_rotation_z(-PI / 2.0) * rotation;
        }
        rest[usize::from(id)] = Pose::at(Vec3::new(left + step * i as f32, y, z), rotation);
    }
}

/// The viewer's hand, fanned along the bottom of the screen and facing the camera, tilted back a
/// little; the card under the pointer stands up straight, higher and larger.
fn hand(rest: &mut [Pose], ids: &[u16], camera: &super::camera::Camera, hover: Option<u16>) {
    let n = ids.len();
    // A card's width on screen, in normalized device coordinates.
    let card = HAND_HEIGHT * 2.0 * (CARD_W / CARD_H) / camera.aspect;
    let spacing = (card * 0.8).min(1.3 / n.max(1) as f32);
    for (i, &id) in ids.iter().enumerate() {
        let offset = i as f32 - (n as f32 - 1.0) / 2.0;
        let x = offset * spacing;
        if hover == Some(id) {
            let ndc = Vec2::new(x, -1.0 + HAND_HEIGHT * HOVER_SCALE * 1.02);
            rest[usize::from(id)] = camera.screen_pose(
                ndc,
                HAND_DEPTH - 40.0,
                Quat::IDENTITY,
                HAND_HEIGHT * HOVER_SCALE,
            );
            continue;
        }
        let y = -1.0 + HAND_HEIGHT * 0.5 - offset * offset * 0.003;
        // Turned only about the line of sight, so every card lies in a plane parallel to the
        // screen and a card nearer the camera covers its neighbour cleanly. A tilt toward or
        // away from the camera made neighbouring cards, a millimetre or two apart, pass
        // through each other, showing torn slivers of both.
        let rotation = Quat::from_rotation_z(-offset * 0.05);
        let depth = HAND_DEPTH - i as f32 * 1.5;
        rest[usize::from(id)] = camera.screen_pose(Vec2::new(x, y), depth, rotation, HAND_HEIGHT);
    }
}

/// The opponent's hand: card backs along the top edge of the screen, mostly out of view.
fn opponent_hand(rest: &mut [Pose], ids: &[u16], camera: &super::camera::Camera) {
    let n = ids.len();
    let spacing = (0.5 / n.max(1) as f32).min(0.06) * 1.6 / camera.aspect;
    for (i, &id) in ids.iter().enumerate() {
        let offset = i as f32 - (n as f32 - 1.0) / 2.0;
        let rotation = Quat::from_rotation_z(offset * 0.04) * Quat::from_rotation_y(PI);
        let ndc = Vec2::new(offset * spacing, 1.02 + offset * offset * 0.002);
        let depth = HAND_DEPTH + 20.0 + i as f32 * 1.5;
        rest[usize::from(id)] = camera.screen_pose(ndc, depth, rotation, OPPONENT_HAND_HEIGHT);
    }
}
