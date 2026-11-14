//! Stress boards: a battlefield of `n` cards in a grid, in which 40 cards at a time fly between
//! random places. The randomness is a fixed hash of the lane and the flight's number, so a
//! board plays the same on every run, and a flight starts at a time fixed by its number, not by
//! when frames happen to be drawn.

use std::f32::consts::PI;

use glam::{Quat, Vec3};

use super::motion::{Curve, Pose, Segment, Timing};
use super::{LAYER_UNKNOWN, Obj, Rec, Zone};

const LANES: usize = 40;
const FLIGHT: u64 = 475_000_000;
const ARC: f32 = 55.0;
const HALF_WIDTH: f32 = 440.0;
const HALF_DEPTH: f32 = 190.0;
/// Height between neighbouring cards of the grid, which overlap: each is above every card
/// before it, so which is on top is a matter of position.
const STEP: f32 = 0.04;

pub struct Stress {
    cards: usize,
    layers: u16,
    columns: usize,
    rows: usize,
    lanes: [Lane; LANES],
}

#[derive(Clone, Copy)]
struct Lane {
    next: u64,
    flights: u64,
}

fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

impl Stress {
    pub fn new(cards: usize, layers: u16) -> Stress {
        let columns = ((cards as f32 * 2.2).sqrt().ceil() as usize).max(1);
        let rows = cards.div_ceil(columns);
        Stress {
            cards,
            layers,
            columns,
            rows,
            lanes: std::array::from_fn(|i| Lane {
                next: FLIGHT * i as u64 / LANES as u64,
                flights: 0,
            }),
        }
    }

    fn slot(&self, index: usize) -> Pose {
        let column = index % self.columns;
        let row = index / self.columns;
        let x = -HALF_WIDTH + 2.0 * HALF_WIDTH * (column as f32 + 0.5) / self.columns as f32;
        let y = HALF_DEPTH - 2.0 * HALF_DEPTH * (row as f32 + 0.5) / self.rows as f32;
        Pose::at(Vec3::new(x, y, STEP * index as f32), Quat::IDENTITY)
    }

    pub fn setup(&self, objs: &mut [Obj]) {
        for (i, obj) in objs.iter_mut().enumerate().take(self.cards) {
            obj.rec = Rec {
                zone: Zone::Battlefield,
                you: true,
                face_up: true,
                tapped: false,
                land: false,
                layer: LAYER_UNKNOWN + 1 + (hash(i as u64) % u64::from(self.layers)) as u16,
                order: i as u32,
            };
            obj.track.push(Segment::instant(0, self.slot(i)), 0);
        }
    }

    /// Starts every flight due by `now`; returns when the last one started ends.
    pub fn step(&mut self, now: u64, objs: &mut [Obj]) -> u64 {
        let mut end = 0;
        for lane_index in 0..LANES {
            while self.lanes[lane_index].next <= now {
                let lane = self.lanes[lane_index];
                let seed = hash((lane_index as u64) << 32 | lane.flights);
                let id = (seed % self.cards as u64) as usize;
                let target = (hash(seed) % self.cards as u64) as usize;
                let mut to = self.slot(target);
                to.position.z += STEP * 0.5;
                if hash(seed ^ 0x55) % 4 == 0 {
                    to.rotation = Quat::from_rotation_z(-PI / 2.0);
                }
                let from = objs[id].track.pose_at(lane.next);
                let timing = Timing {
                    delay: 0,
                    duration: FLIGHT,
                    stagger: 0,
                    curve: Curve::EASE_OUT,
                };
                let segment = Segment::new(lane.next, timing, from, to, ARC);
                end = end.max(segment.end());
                objs[id].track.push(segment, now);
                self.lanes[lane_index] = Lane {
                    next: lane.next + FLIGHT,
                    flights: lane.flights + 1,
                };
            }
        }
        end
    }
}
