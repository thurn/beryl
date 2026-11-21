//! The Enhanced tier's particles for `vanish` (docs/beryl-3d.md, "The Enhanced tier"): a
//! destroyed permanent falls to ash as it dissolves and an exiled one is drawn into a vortex as
//! it whitens. Only Enhanced writes them, each frame's are a pure function of its time, and they
//! move nothing the checks see.

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{CardType, ObjectId, Side, Why, ZoneName};
use beryl_protocol::script::{Act, Subject, Table as Played, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_play::play::Show;
use kit_frame::frame::{Draw, Frame, FrameBuilder, Material, PARTICLES};
use kit_fx::effects::Effect;
use kit_ui::input::Tier;

use crate::common::{MS, empty_frame, show};

/// How long after the vanish is pushed the tests look: past the end of every vanish.
const UNTIL: u64 = 3_000 * MS;

/// The sample table, with a reset script.
fn table() -> GameUpdate {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    builder.update()
}

/// The slot of the first card named `name` that `side` has on the battlefield.
fn permanent(update: &GameUpdate, side: Side, name: &str) -> ObjectId {
    update
        .view
        .objects
        .iter()
        .find(|o| {
            o.zone == ZoneName::Battlefield
                && o.controller == side
                && o.name.as_deref() == Some(name)
        })
        .unwrap_or_else(|| panic!("{side:?} has a {name} on the battlefield"))
        .slot
}

/// The view after each permanent in `slots` leaves the battlefield together, for its owner's
/// graveyard when destroyed and for exile when exiled, in one beat.
fn vanish(before: &GameUpdate, slots: &[ObjectId], why: Why) -> GameUpdate {
    let to = if why == Why::Exile {
        ZoneName::Exile
    } else {
        ZoneName::Graveyard
    };
    let mut view = before.view.clone();
    let mut acts = Vec::new();
    for &slot in slots {
        let object = view
            .objects
            .iter_mut()
            .find(|o| o.slot == slot)
            .expect("the card is in the view");
        object.zone = to;
        let record = object.clone();
        view.battlefield.retain(|&s| s != slot);
        if to == ZoneName::Graveyard {
            match record.owner {
                Side::You => view.players.you.graveyard.push(slot),
                Side::Opponent => view.players.opponent.graveyard.push(slot),
            }
        }
        acts.push(Act::Vanish {
            subject: Subject::Object { slot },
            from: ZoneName::Battlefield,
            why,
            object: Some(record),
            at: None,
        });
    }
    view.script = Some(ScriptBuilder::default().beat(acts).build());
    GameUpdate {
        session: before.session,
        view,
    }
}

/// The frame `show` draws at `now` in `tier`.
fn draw(show: &mut Show, buffer: &mut Frame, now: u64, tier: Tier) -> Frame {
    let mut builder = FrameBuilder::new(buffer, tier);
    show.frame(now, &mut builder);
    builder.finish().clone()
}

/// A show that has drawn `start` at 0 and has then been handed `vanished`, timed as the
/// session times it, to start on its next frame.
fn handed(start: &GameUpdate, vanished: &GameUpdate, tier: Tier) -> (Show, Frame) {
    let mut show = show();
    let mut buffer = empty_frame();
    show.push(Box::new(start.clone()), Vec::new());
    draw(&mut show, &mut buffer, 0, tier);
    let script = vanished.view.script.as_ref().expect("a script");
    let timings = time_script(script, &Played::of(&start.view));
    show.push(Box::new(vanished.clone()), timings);
    (show, buffer)
}

/// Each frame of `vanished` after `start`, every 10 ms from 10 ms to `UNTIL`, in `tier`.
fn frames(start: &GameUpdate, vanished: &GameUpdate, tier: Tier) -> Vec<(u64, Frame)> {
    let (mut show, mut buffer) = handed(start, vanished, tier);
    (1..=UNTIL / (10 * MS))
        .map(|tick| {
            let now = tick * 10 * MS;
            (now, draw(&mut show, &mut buffer, now, tier))
        })
        .collect()
}

/// The particle records of `frame`.
fn particles(frame: &Frame) -> Vec<Draw> {
    frame
        .draws
        .iter()
        .filter(|draw| draw.material == Material::Particle as u8)
        .copied()
        .collect()
}

/// The one run of `effect` that `vanished` plays after `start`: its start and duration.
fn run_of(start: &GameUpdate, vanished: &GameUpdate, effect: Effect) -> (u64, u64) {
    let (mut show, mut buffer) = handed(start, vanished, Tier::Enhanced);
    draw(&mut show, &mut buffer, 10 * MS, Tier::Enhanced);
    let runs: Vec<_> = show
        .effects()
        .iter()
        .filter(|run| run.effect == effect)
        .map(|run| (run.start, run.duration))
        .collect();
    assert_eq!(runs.len(), 1, "one {effect:?} in {:?}", show.effects());
    runs[0]
}

/// The sample table and the view after its viewer's Serra Angel leaves as `why` says.
fn angel_leaves(why: Why) -> (GameUpdate, GameUpdate) {
    let start = table();
    let angel = permanent(&start, Side::You, "Serra Angel");
    let vanished = vanish(&start, &[angel], why);
    (start, vanished)
}

#[test]
fn baseline_writes_no_particles() {
    for why in [Why::Destroy, Why::Exile] {
        let (start, vanished) = angel_leaves(why);
        for (now, frame) in frames(&start, &vanished, Tier::Baseline) {
            assert!(particles(&frame).is_empty(), "{why:?} at {} ms", now / MS);
            assert_eq!(frame.load.particles, 0);
        }
    }
}

#[test]
fn a_destroyed_card_falls_to_ash_after_it_darkens_and_an_exiled_one_to_a_vortex() {
    const DARKENED: f32 = 0.3;
    for (why, effect, most) in [
        (Why::Destroy, Effect::Dissolve, 96),
        (Why::Exile, Effect::Blanch, 128),
    ] {
        let (start, vanished) = angel_leaves(why);
        let (from, duration) = run_of(&start, &vanished, effect);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a share of a run of about a second, in nanoseconds"
        )]
        let darkened = from + (f64::from(DARKENED) * duration as f64) as u64;
        let mut seen = 0;
        for (now, frame) in frames(&start, &vanished, Tier::Enhanced) {
            let written = particles(&frame);
            assert_eq!(written.len(), frame.load.particles as usize);
            assert!(written.len() <= most, "{why:?} at {} ms", now / MS);
            let outside = now < from || now >= from + duration;
            if outside || (effect == Effect::Dissolve && now <= darkened) {
                assert!(
                    written.is_empty(),
                    "{why:?} writes {} particles at {} ms, its run {} to {} ms",
                    written.len(),
                    now / MS,
                    from / MS,
                    (from + duration) / MS
                );
            }
            seen = seen.max(written.len());
        }
        assert!(seen > most / 4, "{why:?} writes at most {seen} particles");
    }
}

#[test]
fn a_moment_drawn_directly_writes_what_every_tick_before_it_led_to() {
    for why in [Why::Destroy, Why::Exile] {
        let (start, vanished) = angel_leaves(why);
        let stepped = frames(&start, &vanished, Tier::Enhanced);
        for (now, frame) in stepped.iter().step_by(7) {
            let (mut show, mut buffer) = handed(&start, &vanished, Tier::Enhanced);
            // The script starts on its first frame; after that, the frame at `now` directly.
            draw(&mut show, &mut buffer, 10 * MS, Tier::Enhanced);
            let direct = draw(&mut show, &mut buffer, *now, Tier::Enhanced);
            assert_eq!(
                particles(&direct),
                particles(frame),
                "{why:?} at {} ms",
                now / MS
            );
        }
    }
}

#[test]
fn particles_are_seeded_by_their_card() {
    let (start, vanished) = angel_leaves(Why::Destroy);
    let once = frames(&start, &vanished, Tier::Enhanced);
    let again = frames(&start, &vanished, Tier::Enhanced);
    for ((now, a), (_, b)) in once.iter().zip(&again) {
        assert_eq!(particles(a), particles(b), "two clients at {} ms", now / MS);
    }
    // The opponent's Serra Angel, destroyed the same way, at the same moment of its run.
    let other = permanent(&start, Side::Opponent, "Serra Angel");
    let theirs = frames(
        &start,
        &vanish(&start, &[other], Why::Destroy),
        Tier::Enhanced,
    );
    let seeds = |frame: &Frame| {
        particles(frame)
            .iter()
            .map(|draw| draw.params[1].to_bits())
            .collect::<Vec<_>>()
    };
    let busiest = once
        .iter()
        .zip(&theirs)
        .max_by_key(|((_, a), _)| particles(a).len())
        .expect("frames");
    let (mine, theirs) = (seeds(&busiest.0.1), seeds(&busiest.1.1));
    assert!(!mine.is_empty() && !theirs.is_empty());
    assert_ne!(mine, theirs, "another card's ash has other seeds");
}

#[test]
fn forty_cards_dissolving_at_once_write_at_most_the_frames_particles() {
    let mut builder = ViewBuilder::new();
    let slots: Vec<ObjectId> = (0..40)
        .map(|_| {
            let card = builder.card(ZoneName::Battlefield, Side::You, "Serra Angel");
            card.types = vec![CardType::Creature];
            card.slot
        })
        .collect();
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let vanished = vanish(&start, &slots, Why::Destroy);
    let most = frames(&start, &vanished, Tier::Enhanced)
        .iter()
        .map(|(_, frame)| frame.load.particles as usize)
        .max()
        .unwrap_or(0);
    assert!(most <= PARTICLES, "{most} particles in one frame");
    assert!(
        most > PARTICLES / 4,
        "forty cards write {most} particles at most"
    );
}

#[test]
fn particles_move_nothing_the_checks_see() {
    for why in [Why::Destroy, Why::Exile] {
        let (start, vanished) = angel_leaves(why);
        let baseline = frames(&start, &vanished, Tier::Baseline);
        let enhanced = frames(&start, &vanished, Tier::Enhanced);
        for ((now, a), (_, b)) in baseline.iter().zip(&enhanced) {
            assert_eq!(a.resolved, b.resolved, "{why:?} at {} ms", now / MS);
        }
    }
}
