//! Light that the tree lists is drawn. Every arrow, line of combat and pointer arrow is an
//! element of role `arrow` whose records are quads of the glow material, and every attacker's
//! wedge and blocker's crescent is a glow record drawn with its card. The interface checks read
//! the element tree, which lists an arrow whether or not its records give any light: every
//! arrow once drew fully faded, and every check passed while none showed.
//!
//! This test draws each such light alone on black with the real renderer, and requires it to
//! light at least `LEAST_PIXELS` pixels. Drawn alone, a light that other cards cover still
//! counts as drawn: a blocker's crescent lies under its attacker while the two fight. It
//! looks at every recording from both seats in the 1600x900 window, and draws only the check
//! time (the moments the interface tests check) at which each kind of light first shows in that
//! recording, so it draws a few frames per recording rather than every tick.
//!
//! The sweep of the recordings is split into `PARTS` tests, and a separate test requires one
//! recording that shows every kind of light to draw each of them, so that the sweep cannot pass
//! by finding nothing to draw.

use std::collections::{BTreeMap, BTreeSet};

use beryl_inspect::stress::Board;
use beryl_inspect::{pages, recordings, runner};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use beryl_render::{Limits, Renderer, Target};
use game_probe::driver::{Duration, Moment};
use kit_base::data::profiles::SizeClass;
use kit_frame::frame::{Draw, Frame, Material, Role, Sprite, sprites};
use kit_ui::input::Size;

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// The least opacity, of 255, at which a light's brightest record must show. A wedge or a
/// crescent fades in from nothing over its first frames, and a fading arrow is nearly clear,
/// so lights under it are not held to showing.
const LEAST_ALPHA: u8 = 64;

/// The least number of pixels a light must change. The thinnest light, a line of combat between
/// two cards beside each other, changes hundreds.
const LEAST_PIXELS: usize = 20;

/// How many levels above black a channel of a pixel must be for the pixel to count as lit, so
/// that a faint fringe of rounding counts as dark.
const LEVELS: u8 = 2;

/// How many tests the sweep of the recordings is split into, each part taking every `PARTS`th
/// recording of the list. Finding where each kind of light first shows plays every recording
/// from both seats: 2590 of them took about 90 s of thread time, 23 s on a 10-core Mac. As one
/// test, the sweep ran for minutes on the few cores a busy tip round left it, after every other
/// test of its batch had finished. The parts run beside the other tests instead.
const PARTS: usize = 8;

/// How many recordings each kind of light is drawn in, in each part. Drawing every recording
/// where a light first shows drew 1346 recordings in 33 s on every core of a Mac; the light is
/// drawn by one writer whatever the card, so 24 recordings of each kind over all the parts,
/// spread over the list, keep each kind's variety of ends at a fraction of the cost.
const PER_KIND: usize = 3;

/// The recording that shows every kind of light in `KINDS`, from your seat: a combat with
/// attackers, blockers and a targeted spell.
const EVERY_KIND: &str = "verbs-combat";

/// The kinds of light `EVERY_KIND` must draw, so that the sweep cannot pass by finding nothing
/// to draw.
const KINDS: [&str; 6] = [
    "source",
    "hostile-target",
    "attack",
    "block",
    "attack mark",
    "block mark",
];

/// One light of a frame: its kind (an arrow's name, or a fight mark), the element its records
/// belong to, and the records, by index in the frame's `draws`.
#[derive(Debug)]
struct Light {
    kind: String,
    owner: u32,
    records: Vec<usize>,
}

/// The kind of fight mark `draw` is, if it is one.
fn fight_mark(draw: &Draw) -> Option<&'static str> {
    let is = |sprite: Sprite| draw.uv == sprite.rect && draw.layer == sprite.page;
    if draw.material != Material::Glow as u8 {
        None
    } else if is(sprites::WEDGE) || is(sprites::WEDGE_HOT) {
        Some("attack mark")
    } else if is(sprites::CRESCENT) {
        Some("block mark")
    } else {
        None
    }
}

/// The lights of a `looking` frame bright enough to be held to showing.
fn lights(frame: &Frame) -> Vec<Light> {
    let mut found: BTreeMap<(String, u32), Vec<usize>> = BTreeMap::new();
    for (at, draw) in frame.draws.iter().enumerate() {
        let Some(&owner) = frame.owners.get(at) else {
            continue;
        };
        let element = frame.resolved.get(owner as usize);
        let kind = match element {
            Some(element) if element.role == Role::Arrow => frame.name(element.name).to_owned(),
            _ => match fight_mark(draw) {
                Some(kind) => kind.to_owned(),
                None => continue,
            },
        };
        found.entry((kind, owner)).or_default().push(at);
    }
    found
        .into_iter()
        .filter(|(_, records)| {
            records
                .iter()
                .any(|&at| frame.draws[at].tint[3] >= LEAST_ALPHA)
        })
        .map(|((kind, owner), records)| Light {
            kind,
            owner,
            records,
        })
        .collect()
}

/// The recording `name` from `side` in the window, its frames `looking` so that each record
/// names its element.
fn page(name: &str, side: Side) -> Result<app_client::driver::Page, String> {
    // The scan visits the whole pack. Keeping every decoded recording resident put the
    // Windows verifier under memory pressure before it could finish the check.
    let mut page = pages::uncached_page(name, side, SizeClass::Regular, WINDOW)?;
    page.set_looking(true);
    Ok(page)
}

/// The check time, in nanoseconds, at which each kind of light first shows in the recording
/// `name` from `side`.
fn first_lights(name: &str, side: Side) -> Result<BTreeMap<String, u64>, String> {
    let mut page = page(name, side)?;
    let mut first: BTreeMap<String, u64> = BTreeMap::new();
    for time in page.check_times() {
        page.seek(&Moment::At(Duration(time)))
            .map_err(|error| format!("{name}: {error}"))?;
        for light in lights(page.frame()) {
            first.entry(light.kind).or_insert(time);
        }
    }
    Ok(first)
}

/// For each test, the moments to draw: for each kind of light, the moment it first shows in
/// `PER_KIND` of the tests that show it, spread evenly over them in name order.
fn chosen(first: &[BTreeMap<String, u64>]) -> Vec<BTreeSet<u64>> {
    let mut showing: BTreeMap<&str, Vec<(usize, u64)>> = BTreeMap::new();
    for (test, kinds) in first.iter().enumerate() {
        for (kind, &time) in kinds {
            showing.entry(kind).or_default().push((test, time));
        }
    }
    let mut out = vec![BTreeSet::new(); first.len()];
    for tests in showing.values() {
        let take = tests.len().min(PER_KIND);
        for n in 0..take {
            let (test, time) = tests[n * tests.len() / take];
            out[test].insert(time);
        }
    }
    out
}

/// How many pixels of an RGBA picture drawn on black are lit: brighter than `LEVELS` in a
/// channel.
fn lit(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[..3].iter().any(|&level| level > LEVELS))
        .count()
}

/// What drawing the recording `name` from `side` at `times` found: the kind of each light drawn,
/// and a line for each that lit too few pixels.
fn draw_lights(
    renderer: &mut Renderer,
    target: &mut Target,
    name: &str,
    side: Side,
    times: &BTreeSet<u64>,
) -> (Vec<String>, Vec<String>) {
    let mut page = page(name, side).expect("a page");
    let mut kinds = Vec::new();
    let mut faults = Vec::new();
    for &time in times {
        page.seek(&Moment::At(Duration(time))).expect("a moment");
        let frame = page.frame();
        for light in lights(frame) {
            let mut isolated = frame.clone();
            isolated.keep_records(|at| light.records.binary_search(&at).is_ok());
            renderer.render(&isolated, target);
            let pixels = lit(&renderer.read(target));
            if pixels < LEAST_PIXELS {
                let seat = if side == Side::You { "you" } else { "opponent" };
                faults.push(format!(
                    "{name} --seat {seat} --at {}ms: the {} of element {} lights {pixels} \
                     pixels, under {LEAST_PIXELS}",
                    time / 1_000_000,
                    light.kind,
                    light.owner,
                ));
            }
            kinds.push(light.kind);
        }
    }
    (kinds, faults)
}

/// Windows runs the replay scan with up to three workers. At ten
/// workers on the shared verifier, memory pressure drove the same check past its hang limit.
fn each_replay<T: Sync, R: Send>(items: &[T], check: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if !cfg!(windows) {
        return runner::each(items, check);
    }
    let per_worker = items.len().div_ceil(3).max(1);
    let groups: Vec<_> = items.chunks(per_worker).collect();
    runner::each(&groups, |group| {
        group.iter().map(&check).collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect()
}

/// What drawing `lit` found: how many lights of each kind were drawn, and a line for each that
/// lit too few pixels.
fn draw_all(lit: &[(&str, Side, BTreeSet<u64>)]) -> (BTreeMap<String, usize>, Vec<String>) {
    // The isolated records sample the same light atlas as the host. They can all share one
    // device and target. Opening a device and loading card art for each replay stalled on Windows.
    let tuning = beryl_inspect::data::checkout().expect("tuning");
    let mut renderer = Renderer::headless(Limits::Default, tuning.lighting).expect("a GPU");
    let mut target = Target::offscreen(&renderer, WINDOW.width as u32, WINDOW.height as u32);
    assert!(
        renderer.upload_page(&beryl_assets::light::prepared(&tuning.light, &tuning.own)),
        "the shapes of light are uploaded"
    );
    let mut drawn: BTreeMap<String, usize> = BTreeMap::new();
    let mut faults: Vec<String> = Vec::new();
    for (name, side, times) in lit {
        let (kinds, more) = draw_lights(&mut renderer, &mut target, name, *side, times);
        for kind in kinds {
            *drawn.entry(kind).or_default() += 1;
        }
        faults.extend(more);
    }
    (drawn, faults)
}

/// Fails with every light in `faults`.
fn assert_no_faults(faults: &[String]) {
    assert!(
        faults.is_empty(),
        "lights the tree lists drew nothing (`just client png <recording> --seat <seat> --at <ms>` \
         shows each):\n{}",
        faults.join("\n")
    );
}

/// Draws the lights of every `PARTS`th recording, starting at the one numbered `part`.
fn sweep(part: usize) {
    let tests: Vec<(String, Side)> = recordings::names()
        .expect("the recordings pack")
        .into_iter()
        .filter(|(name, _)| Board::parse(name).is_none())
        .skip(part)
        .step_by(PARTS)
        .map(|(name, seat)| {
            let side = if seat == Seat::P0 {
                Side::You
            } else {
                Side::Opponent
            };
            (name, side)
        })
        .collect();
    let start = std::time::Instant::now();
    // A replay that panics fails with the recording's name and seat, which the panic's own
    // message does not give.
    let moments = each_replay(&tests, |(name, side)| {
        runner::caught(|| first_lights(name, *side)).and_then(|first| first)
    });
    eprintln!(
        "scanned {} recordings in {:?}",
        tests.len(),
        start.elapsed()
    );
    let start = std::time::Instant::now();
    let first: Vec<BTreeMap<String, u64>> = tests
        .iter()
        .zip(moments)
        .map(|((name, side), first)| {
            first.unwrap_or_else(|error| panic!("{name} from {side:?}'s seat: {error}"))
        })
        .collect();
    let lit: Vec<(&str, Side, BTreeSet<u64>)> = tests
        .iter()
        .zip(chosen(&first))
        .map(|((name, side), times)| (name.as_str(), *side, times))
        .filter(|(_, _, times)| !times.is_empty())
        .collect();
    let (drawn, faults) = draw_all(&lit);
    eprintln!(
        "drew {} recordings in {:?}: {drawn:?}",
        lit.len(),
        start.elapsed()
    );
    assert!(
        !drawn.is_empty(),
        "no recording of this part shows any light, so the part tests nothing"
    );
    assert_no_faults(&faults);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_0() {
    sweep(0);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_1() {
    sweep(1);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_2() {
    sweep(2);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_3() {
    sweep(3);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_4() {
    sweep(4);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_5() {
    sweep(5);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_6() {
    sweep(6);
}

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn_7() {
    sweep(7);
}

#[test]
fn every_kind_of_light_is_drawn() {
    let first = first_lights(EVERY_KIND, Side::You).unwrap_or_else(|error| panic!("{error}"));
    let times: BTreeSet<u64> = first.into_values().collect();
    let (drawn, faults) = draw_all(&[(EVERY_KIND, Side::You, times)]);
    let missing: Vec<&str> = KINDS
        .into_iter()
        .filter(|kind| !drawn.contains_key(*kind))
        .collect();
    assert!(
        missing.is_empty(),
        "{EVERY_KIND} from your seat no longer draws these kinds of light, so the sweep cannot \
         tell whether they draw: {missing:?} (drawn: {drawn:?}). Name a recording that shows \
         every kind in `EVERY_KIND`."
    );
    assert_no_faults(&faults);
}
