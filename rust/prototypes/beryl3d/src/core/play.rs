//! Playing a view's script: each beat's steps become segments on the objects they name, all
//! compiled when the script starts, with absolute times. `fly` arcs through the air and turns
//! the card over where its face changes, `appear` and `vanish` fade, `tap` rotates, and every
//! other verb is an instant change that still holds its beat for as long as the data says.
//! Objects a beat does not name but whose place it changes (the rest of a hand) slide.
//! When the script ends, every object is moved to where the view's own records place it.

use beryl_session::GameUpdate;
use beryl_session::protocol::{CardType, Facing, ObjectView, PlayerView, Side, ZoneName};
use beryl_session::script::{Act, Step, Subject};

use super::motion::{Segment, Timing};
use super::timing::Other;
use super::{Core, Rec, Zone};

/// How high a flight rises above the straight line, in millimetres. The doc gives this value as
/// an example of a verb's spatial data; ui-conventions.json has no spatial entries yet.
const ARC: f32 = 55.0;

#[derive(Clone, Copy)]
enum Kind {
    Fly,
    FadeIn,
    /// Fades where it is, then is at its new place at once.
    FadeOut {
        gone: bool,
    },
    Turn,
    Instant,
}

#[derive(Clone, Copy)]
pub(super) struct Anim {
    id: usize,
    kind: Kind,
    at: u64,
    timing: Timing,
}

pub(super) struct Scratch {
    pub anims: Vec<Anim>,
    pub touched: Vec<u32>,
    pub stamp: u32,
    /// Position of each object in the view's ordered lists, or `u32::MAX`.
    pub positions: Vec<u32>,
    pub seen: Vec<bool>,
}

impl Scratch {
    pub fn new(objects: usize) -> Scratch {
        Scratch {
            anims: Vec::with_capacity(4096),
            touched: vec![0; objects],
            stamp: 0,
            positions: vec![u32::MAX; objects],
            seen: vec![false; objects],
        }
    }
}

fn zone(name: ZoneName) -> Zone {
    match name {
        ZoneName::Library => Zone::Library,
        ZoneName::Hand => Zone::Hand,
        ZoneName::Battlefield => Zone::Battlefield,
        ZoneName::Graveyard => Zone::Graveyard,
        ZoneName::Exile => Zone::Exile,
        ZoneName::Stack => Zone::Stack,
        ZoneName::Nowhere | ZoneName::Sideboard => Zone::Nowhere,
    }
}

impl Core {
    /// The record an object's view gives, keeping its place in its zone unless it changed zone.
    fn rec_of(&mut self, object: &ObjectView) -> Rec {
        let id = usize::from(object.slot.0);
        let old = self.objs[id].rec;
        let zone = zone(object.zone);
        let side = if zone == Zone::Battlefield {
            object.controller
        } else {
            object.zone_side.unwrap_or(object.owner)
        };
        let order = if old.zone == zone {
            old.order
        } else {
            self.next_order += 1;
            self.next_order
        };
        Rec {
            zone,
            you: side == Side::You,
            face_up: object.facing == Facing::Up && object.name.is_some() && zone != Zone::Library,
            tapped: object.tapped,
            land: object.types.contains(&CardType::Land),
            layer: self.faces.layer(object.name.as_deref()),
            order,
        }
    }

    /// Plays `update`'s script from `start`, and returns when it ends.
    pub(super) fn play(&mut self, update: &GameUpdate, start: u64, now: u64) -> u64 {
        let view = &update.view;
        if !self.has_game {
            self.reconcile(view);
            self.placement.place(&self.objs, &self.camera);
            for id in 0..self.objs.len() {
                if self.objs[id].rec.zone != Zone::Nowhere {
                    let rest = self.placement.rest[id];
                    self.push(id, Segment::instant(start, rest), now);
                }
            }
            return start;
        }
        let mut t = start;
        if let Some(script) = &view.script {
            for beat in &script.beats {
                t = self.play_beat(beat, t, now);
            }
        }
        self.reconcile(view);
        self.placement.place(&self.objs, &self.camera);
        for id in 0..self.objs.len() {
            let track = &self.objs[id].track;
            if track.is_empty() {
                if self.objs[id].rec.zone != Zone::Nowhere {
                    let rest = self.placement.rest[id];
                    self.push(id, Segment::instant(t, rest.with_opacity(0.0)), now);
                    let fade =
                        Segment::new(t, self.timing.appear, rest.with_opacity(0.0), rest, 0.0);
                    self.push(id, fade, now);
                }
                continue;
            }
            let last = track.final_pose();
            if self.objs[id].rec.zone == Zone::Nowhere {
                if last.opacity > 0.0 {
                    let fade =
                        Segment::new(t, self.timing.vanish, last, last.with_opacity(0.0), 0.0);
                    self.push(id, fade, now);
                }
                continue;
            }
            let rest = self.placement.rest[id];
            if !last.same(&rest) {
                let from = track.pose_at(t);
                self.push(id, Segment::new(t, self.timing.slide, from, rest, 0.0), now);
            }
        }
        t
    }

    /// Sets every object's record from the view, with each zone's order from the view's lists.
    fn reconcile(&mut self, view: &PlayerView) {
        let positions = &mut self.scratch.positions;
        positions.fill(u32::MAX);
        self.scratch.seen.fill(false);
        let lists = [
            &view.battlefield,
            &view.players.you.library,
            &view.players.opponent.library,
            &view.players.you.graveyard,
            &view.players.opponent.graveyard,
        ];
        for list in lists {
            for (i, id) in list.iter().enumerate() {
                positions[usize::from(id.0)] = i as u32;
            }
        }
        for object in &view.objects {
            let id = usize::from(object.slot.0);
            let mut rec = self.rec_of(object);
            if self.scratch.positions[id] != u32::MAX {
                rec.order = self.scratch.positions[id];
            }
            self.objs[id].rec = rec;
            self.scratch.seen[id] = true;
        }
        for (list, you) in [
            (&view.players.you.library, true),
            (&view.players.opponent.library, false),
        ] {
            for (i, id) in list.iter().enumerate() {
                let id = usize::from(id.0);
                if !self.scratch.seen[id] {
                    let rec = &mut self.objs[id].rec;
                    rec.zone = Zone::Library;
                    rec.you = you;
                    rec.face_up = false;
                    rec.tapped = false;
                    rec.order = i as u32;
                    self.scratch.seen[id] = true;
                }
            }
        }
        for id in 0..self.objs.len() {
            if !self.scratch.seen[id] {
                self.objs[id].rec.zone = Zone::Nowhere;
            }
        }
    }

    fn play_beat(&mut self, beat: &[Step], t: u64, now: u64) -> u64 {
        self.scratch.anims.clear();
        let mut length = 0;
        let mut flights = 0;
        for step in beat {
            let (id, kind, timing) = match &step.act {
                Act::Fly { from, object, .. } => {
                    let rec = self.rec_of(object);
                    let timing = self.fly_timing(*from, rec);
                    let timing = Timing {
                        delay: timing.delay + flights * timing.stagger,
                        ..timing
                    };
                    flights += 1;
                    self.objs[usize::from(object.slot.0)].rec = rec;
                    (usize::from(object.slot.0), Kind::Fly, timing)
                }
                Act::Appear { object, .. } => {
                    let rec = self.rec_of(object);
                    self.objs[usize::from(object.slot.0)].rec = rec;
                    (usize::from(object.slot.0), Kind::FadeIn, self.timing.appear)
                }
                Act::Vanish {
                    subject: Subject::Object { slot },
                    object,
                    ..
                } => {
                    let id = usize::from(slot.0);
                    if let Some(object) = object {
                        let rec = self.rec_of(object);
                        self.objs[id].rec = rec;
                    } else {
                        self.objs[id].rec.zone = Zone::Nowhere;
                    }
                    (
                        id,
                        Kind::FadeOut {
                            gone: object.is_none(),
                        },
                        self.timing.vanish,
                    )
                }
                Act::Tap { object } => {
                    let rec = self.rec_of(object);
                    self.objs[usize::from(object.slot.0)].rec = rec;
                    let timing = if rec.tapped {
                        self.timing.tap
                    } else {
                        self.timing.untap
                    };
                    (usize::from(object.slot.0), Kind::Turn, timing)
                }
                Act::Flip { object } => {
                    let rec = self.rec_of(object);
                    self.objs[usize::from(object.slot.0)].rec = rec;
                    (usize::from(object.slot.0), Kind::Turn, self.timing.flip)
                }
                act => {
                    let timing = self.timing.other(other(act));
                    length = length.max(timing.delay + timing.duration);
                    let Some(object) = act.object() else {
                        continue;
                    };
                    let rec = self.rec_of(object);
                    self.objs[usize::from(object.slot.0)].rec = rec;
                    (usize::from(object.slot.0), Kind::Instant, timing)
                }
            };
            length = length.max(timing.delay + timing.duration);
            self.scratch.anims.push(Anim {
                id,
                kind,
                at: t + timing.delay,
                timing,
            });
        }
        self.placement.place(&self.objs, &self.camera);
        self.scratch.stamp += 1;
        let stamp = self.scratch.stamp;
        for i in 0..self.scratch.anims.len() {
            let anim = self.scratch.anims[i];
            self.scratch.touched[anim.id] = stamp;
            self.animate(anim, now);
        }
        for id in 0..self.objs.len() {
            let obj = &self.objs[id];
            if self.scratch.touched[id] == stamp
                || obj.rec.zone == Zone::Nowhere
                || obj.track.is_empty()
            {
                continue;
            }
            let rest = self.placement.rest[id];
            if !obj.track.final_pose().same(&rest) {
                let from = obj.track.pose_at(t);
                self.push(id, Segment::new(t, self.timing.slide, from, rest, 0.0), now);
            }
        }
        t + length
    }

    fn animate(&mut self, anim: Anim, now: u64) {
        let Anim {
            id,
            kind,
            at,
            timing,
        } = anim;
        let rest = self.placement.rest[id];
        let from = self.objs[id].track.pose_at(at);
        let end = at + timing.duration;
        match kind {
            Kind::Fly => {
                let from = if self.objs[id].track.is_empty() {
                    rest
                } else {
                    from
                };
                self.push(id, Segment::new(at, timing, from, rest, ARC), now);
            }
            Kind::Turn => self.push(id, Segment::new(at, timing, from, rest, 0.0), now),
            Kind::FadeIn => {
                self.push(
                    id,
                    Segment::new(at, timing, rest.with_opacity(0.0), rest, 0.0),
                    now,
                );
            }
            Kind::FadeOut { gone } => {
                self.push(
                    id,
                    Segment::new(at, timing, from, from.with_opacity(0.0), 0.0),
                    now,
                );
                if !gone {
                    self.push(id, Segment::instant(end, rest), now);
                }
            }
            Kind::Instant => self.push(id, Segment::instant(at, rest), now),
        }
    }

    /// The `fly` case that fits a flight from `from` to the record `to`.
    fn fly_timing(&self, from: ZoneName, to: Rec) -> Timing {
        let timing = &self.timing;
        match (from, to.zone) {
            (ZoneName::Stack, Zone::Battlefield) => timing.resolves,
            (ZoneName::Hand, Zone::Battlefield) if to.you => timing.land_played,
            (ZoneName::Hand, Zone::Battlefield) => timing.opponent_land_played,
            (ZoneName::Hand, Zone::Stack) if to.you => timing.own_spell_cast,
            (ZoneName::Hand, Zone::Stack) => timing.opponent_spell_cast,
            (ZoneName::Hand, Zone::Graveyard) => timing.discard,
            _ => timing.fly,
        }
    }
}

fn other(act: &Act) -> Other {
    match act {
        Act::Strike { .. } => Other::Strike,
        Act::Streak { .. } => Other::Streak,
        Act::Count { .. } => Other::Count,
        Act::Set { .. } => Other::Set,
        Act::Showcase { .. } => Other::Showcase,
        Act::Dwell { .. } => Other::Dwell,
        Act::Announce { .. } => Other::Announce,
        Act::Point { .. } => Other::Point,
        Act::Mark { .. } | Act::Lift { .. } => Other::Mark,
        _ => Other::Emerge,
    }
}
