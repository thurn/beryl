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

use std::collections::{BTreeMap, BTreeSet};

use beryl_host::{Headless, Scans};
use beryl_inspect::stress::Board;
use beryl_inspect::{pages, recordings, runner};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use game_probe::driver::{Duration, Moment};
use kit_frame::frame::{Draw, Frame, Material, Role, Sprite, sprites};
use kit_probe::agent::Layout;
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

/// How many recordings each kind of light is drawn in. Drawing every recording where a light
/// first shows drew 1346 recordings in 33 s on every core of a Mac; the light is drawn by one
/// writer whatever the card, so 24 recordings of each kind, spread over the list, keep each
/// kind's variety of ends at a fraction of the cost.
const PER_KIND: usize = 24;

/// The kinds of light at least one recording must show, so that the test cannot pass by finding
/// nothing to draw.
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
    let mut page = pages::page(name, side, Layout::Desktop, WINDOW)?;
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
fn draw_lights(name: &str, side: Side, times: &BTreeSet<u64>) -> (Vec<String>, Vec<String>) {
    let page = page(name, side).expect("a page");
    let mut head = Headless::new(page, WINDOW.width, WINDOW.height, &Scans::app()).expect("a GPU");
    // Making the host makes the client again, which forgets that it was looking.
    head.page().set_looking(true);
    let mut kinds = Vec::new();
    let mut faults = Vec::new();
    for &time in times {
        head.seek(&Moment::At(Duration(time))).expect("a moment");
        for light in lights(head.frame()) {
            let records: BTreeSet<usize> = light.records.iter().copied().collect();
            let pixels = lit(&head.pixels_keeping_records(|_, at| records.contains(&at)));
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

#[test]
fn every_arrow_line_and_fight_mark_the_tree_lists_is_drawn() {
    let tests: Vec<(String, Side)> = recordings::names()
        .expect("the recordings pack")
        .into_iter()
        .filter(|(name, _)| Board::parse(name).is_none())
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
    let moments = runner::each(&tests, |(name, side)| first_lights(name, *side));
    eprintln!(
        "scanned {} recordings in {:?}",
        tests.len(),
        start.elapsed()
    );
    let start = std::time::Instant::now();
    let first: Vec<BTreeMap<String, u64>> = tests
        .iter()
        .zip(moments)
        .map(|((name, _), first)| first.unwrap_or_else(|error| panic!("{name}: {error}")))
        .collect();
    let lit_tests: Vec<(&str, Side, BTreeSet<u64>)> = tests
        .iter()
        .zip(chosen(&first))
        .map(|((name, side), times)| (name.as_str(), *side, times))
        .filter(|(_, _, times)| !times.is_empty())
        .collect();
    let found = runner::each(&lit_tests, |(name, side, times)| {
        draw_lights(name, *side, times)
    });
    let mut drawn: BTreeMap<String, usize> = BTreeMap::new();
    let mut faults: Vec<String> = Vec::new();
    for (kinds, more) in found {
        for kind in kinds {
            *drawn.entry(kind).or_default() += 1;
        }
        faults.extend(more);
    }
    eprintln!(
        "drew {} recordings in {:?}: {drawn:?}",
        lit_tests.len(),
        start.elapsed()
    );
    let missing: Vec<&str> = KINDS
        .into_iter()
        .filter(|kind| !drawn.contains_key(*kind))
        .collect();
    assert!(
        missing.is_empty(),
        "no recording shows these kinds of light, so the test cannot tell whether they draw: \
         {missing:?} (drawn: {drawn:?})"
    );
    assert!(
        faults.is_empty(),
        "lights the tree lists drew nothing (`just b3d png <recording> --seat <seat> --at <ms>` \
         shows each):\n{}",
        faults.join("\n")
    );
}
