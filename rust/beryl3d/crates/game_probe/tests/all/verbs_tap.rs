//! The verbs `tap` and `set`, and the badges on a permanent (docs/beryl-3d.md, "Verbs" and
//! "Objects and placement"): each case of the two verbs plays to its end with every object at
//! its placement and no fault from the step checks, a permanent reads as tapped from the middle
//! of its turn, and a permanent wears its power and toughness, its counters and its pile's count
//! as badges on its face.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{
    CardType, Counter, CounterView, ObjectId, PlayerView, PowerToughness, Side, Why, ZoneName,
};
use beryl_protocol::script::{Act, Fact, Script, Table as Played, time_script};
use game_play::play::Show;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use game_probe::testing::{ScriptBuilder, ViewBuilder};
use game_table::table::badges::STYLE;
use glam::{Vec2, Vec3};
use kit_frame::frame::{Frame, Material, StateBits};
use kit_text::text::{Glyphs, Text, glyph_key};
use kit_ui::input::{GlyphCell, GlyphKey};

/// The window's physical pixels per logical point, as `common::window` sets it.
const SCALE: f32 = 2.0;

/// `view` with `script`, timed as the session times it from the table of `before`.
fn scripted(before: &PlayerView, mut view: PlayerView, script: Script) -> (GameUpdate, Vec<f64>) {
    let timings = time_script(&script, &Played::of(before));
    let durations = timings.iter().map(|timing| timing.duration).collect();
    view.script = Some(script);
    let update = GameUpdate {
        session: beryl_protocol::protocol::SessionId(1),
        view,
    };
    (update, durations)
}

/// A show that has taken `before` at rest and keeps records of the scripts after it.
fn shown(before: &GameUpdate) -> Show {
    let mut show = show();
    let mut start = before.clone();
    start.view.script = Some(ScriptBuilder::reset().build());
    show.push(Box::new(start), Vec::new());
    frame(&mut show, &mut empty_frame(), 0);
    show.record();
    show
}

/// Plays `after`, timed from `before`, from 100 ms to its end, checking that the step's case is
/// `case`, and that no step check finds anything once it has played.
fn plays_cleanly(before: &GameUpdate, after: &PlayerView, script: Script, case: &str) {
    let timings = time_script(&script, &Played::of(&before.view));
    assert_eq!(
        timings[0].case.as_deref(),
        Some(case),
        "the step is the case {case}"
    );
    let mut show = shown(before);
    let mut view = after.clone();
    view.script = Some(script);
    let update = GameUpdate {
        session: before.session,
        view,
    };
    show.push(Box::new(update), timings);
    let mut scratch = empty_frame();
    for t in (100..=1_000).step_by(10) {
        frame(&mut show, &mut scratch, t * MS);
    }
    let history = show.history();
    assert_eq!(history.len(), 1, "one script was played");
    let script = &history[0];
    assert!(script.start.is_some(), "the script started");
    assert_eq!(no_jumps(script), Vec::new(), "no jumps in {case}");
    assert_eq!(
        rest_equals_placement(script),
        Vec::new(),
        "every object rests at its placement after {case}"
    );
    assert_eq!(
        step_times(script),
        Vec::new(),
        "each step of {case} plays its time"
    );
}

/// A table of three lands and a creature of the viewer's, with a library.
fn lands() -> (ViewBuilder, ObjectId) {
    let mut builder = ViewBuilder::new();
    for _ in 0..4 {
        builder.card(ZoneName::Library, Side::You, "Island");
    }
    let mut first = None;
    for name in ["Island", "Island", "Plains"] {
        let land = builder.card(ZoneName::Battlefield, Side::You, name);
        land.types = vec![CardType::Land];
        first.get_or_insert(land.slot);
    }
    let angel = builder.card(ZoneName::Battlefield, Side::You, "Serra Angel");
    angel.types = vec![CardType::Creature];
    angel.pt = Some(PowerToughness {
        power: 4,
        toughness: 4,
    });
    (builder, first.expect("a land"))
}

fn tapping(view: &PlayerView, slot: ObjectId, tapped: bool) -> (PlayerView, Script) {
    let mut after = view.clone();
    let object = after
        .objects
        .iter_mut()
        .find(|object| object.slot == slot)
        .expect("the land is in the view");
    object.tapped = tapped;
    let object = object.clone();
    let script = ScriptBuilder::default().beat([Act::Tap { object }]).build();
    (after, script)
}

#[test]
fn each_case_of_tap_and_set_plays_to_its_end_at_its_placement() {
    let (builder, island) = lands();
    let untapped = builder.update();
    let (tapped_view, script) = tapping(&untapped.view, island, true);
    plays_cleanly(&untapped, &tapped_view, script, "tap");

    let tapped = GameUpdate {
        session: untapped.session,
        view: tapped_view.clone(),
    };
    let (_, script) = tapping(&tapped_view, island, false);
    plays_cleanly(&tapped, &untapped.view, script, "untap");

    let turn = &untapped.view.turn;
    let script = ScriptBuilder::default()
        .beat([Act::Set {
            fact: Fact::Turn {
                number: turn.number + 1,
                own_number: turn.own_number,
                active: turn.active,
                step: turn.step,
            },
        }])
        .build();
    plays_cleanly(&untapped, &untapped.view, script, "step begins");

    let mut order = untapped.view.players.you.library.clone();
    order.reverse();
    let mut shuffled = untapped.view.clone();
    shuffled.players.you.library.clone_from(&order);
    let script = ScriptBuilder::default()
        .beat([Act::Set {
            fact: Fact::Library {
                player: Side::You,
                order,
            },
        }])
        .build();
    plays_cleanly(&untapped, &shuffled, script, "library shuffled");
}

#[test]
fn a_fact_library_leaves_every_card_at_its_placement() {
    let (builder, _) = lands();
    let before = builder.update();
    let mut order = before.view.players.you.library.clone();
    order.rotate_left(1);
    let mut after = before.view.clone();
    after.players.you.library.clone_from(&order);
    let script = ScriptBuilder::default()
        .beat([Act::Set {
            fact: Fact::Library {
                player: Side::You,
                order,
            },
        }])
        .build();
    let (update, _) = scripted(&before.view, after.clone(), script);
    let timings = time_script(
        update.view.script.as_ref().expect("a script"),
        &Played::of(&before.view),
    );
    let mut show = shown(&before);
    show.push(Box::new(update), timings);
    let mut scratch = empty_frame();
    frame(&mut show, &mut scratch, 100 * MS);
    frame(&mut show, &mut scratch, 200 * MS);
    let placement = show.table().placement(&after, &[]);
    for object in &after.objects {
        let slot = usize::from(object.slot.0);
        assert_eq!(
            show.pose(slot, 200 * MS),
            placement.get(object.slot).map(|rest| rest.pose),
            "card #{slot} is at its placement"
        );
    }
}

/// Whether the card in `slot` is drawn tapped in `frame`.
fn drawn_tapped(frame: &Frame, slot: u16) -> bool {
    frame
        .resolved
        .iter()
        .find(|element| element.id.0 == u32::from(slot))
        .expect("the card is drawn")
        .state
        .has(StateBits::TAPPED)
}

#[test]
fn a_permanent_reads_as_tapped_from_the_middle_of_its_turn() {
    let (builder, island) = lands();
    let before = builder.update();
    let (after, script) = tapping(&before.view, island, true);
    let timings = time_script(&script, &Played::of(&before.view));
    let duration = timings[0].duration;
    let mut show = shown(&before);
    let mut view = after;
    view.script = Some(script);
    show.push(
        Box::new(GameUpdate {
            session: before.session,
            view,
        }),
        timings,
    );
    let mut scratch = empty_frame();
    let start = 100 * MS;
    frame(&mut show, &mut scratch, start);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a step of milliseconds"
    )]
    let at = |share: f64| start + (duration * share * 1e6) as u64;
    let early = frame(&mut show, &mut scratch, at(0.05));
    assert!(
        !drawn_tapped(&early, island.0),
        "the land has hardly begun to turn"
    );
    let middle = frame(&mut show, &mut scratch, at(0.5));
    assert!(drawn_tapped(&middle, island.0), "the land has turned");
    let end = frame(&mut show, &mut scratch, at(1.5));
    assert!(drawn_tapped(&end, island.0), "the land is tapped");
}

/// A glyph cell for every glyph a badge can use, each at its own place on page 1 so that a
/// glyph record says which glyph it draws: its rectangle's x is the glyph's number.
fn every_glyph() -> Vec<GlyphCell> {
    (0..2_000)
        .map(|glyph| GlyphCell {
            key: glyph_key(STYLE, SCALE, glyph),
            rect: [glyph, 0, 16, 20],
            bearing: [0, -20],
        })
        .collect()
}

/// A show whose badges are shaped and whose glyphs are all in the atlas.
fn badged_show(before: &GameUpdate) -> Show {
    let mut show = shown(before);
    let mut wanted = Glyphs::default();
    let badges = show.badges_mut();
    badges.shape(&mut Text::new(), &mut wanted, SCALE);
    let cells = every_glyph();
    badges.add(1, &cells);
    let _: Vec<GlyphKey> = wanted.drain_wanted().collect();
    show
}

/// The glyph records of the table in `frame`, in order: which glyph each draws and where its
/// middle is in the table's space.
fn table_glyphs(frame: &Frame) -> Vec<(u16, Vec3)> {
    frame.draws[..frame.layer as usize]
        .iter()
        .filter(|draw| draw.material == Material::Glyph as u8)
        .map(|draw| {
            let model = draw.model;
            (draw.uv[0], Vec3::new(model[3], model[7], model[11]))
        })
        .collect()
}

/// The glyphs `text` is shaped into in the badges' style.
fn glyphs_of(text: &str) -> Vec<u16> {
    let mut shaper = Text::new();
    text.chars()
        .flat_map(|piece| {
            let block = shaper.block(&piece.to_string(), STYLE, &[], None);
            block
                .glyphs
                .iter()
                .map(|glyph| glyph.glyph)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether `point` lies inside the convex quadrilateral `corners`.
fn inside(corners: [Vec2; 4], point: Vec2) -> bool {
    let sides: Vec<f32> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            (b - a).perp_dot(point - a)
        })
        .collect();
    sides.iter().all(|side| *side >= 0.0) || sides.iter().all(|side| *side <= 0.0)
}

#[test]
fn a_creature_wears_its_power_and_toughness_inside_its_corners_through_a_flight() {
    let mut builder = ViewBuilder::new();
    let bear = builder.card(ZoneName::Hand, Side::You, "Grizzly Bears");
    bear.types = vec![CardType::Creature];
    let slot = bear.slot;
    let before = builder.update();
    let mut after = before.view.clone();
    after.battlefield.push(slot);
    let landed = after
        .objects
        .iter_mut()
        .find(|object| object.slot == slot)
        .expect("the bear is in the view");
    landed.zone = ZoneName::Battlefield;
    landed.pt = Some(PowerToughness {
        power: 2,
        toughness: 3,
    });
    let object = landed.clone();
    let script = ScriptBuilder::default()
        .beat([Act::Fly {
            from: ZoneName::Hand,
            why: Why::Play,
            object,
            at: None,
            item: None,
            origin: None,
        }])
        .build();
    let timings = time_script(&script, &Played::of(&before.view));
    let mut show = badged_show(&before);
    let mut view = after;
    view.script = Some(script);
    show.push(
        Box::new(GameUpdate {
            session: before.session,
            view,
        }),
        timings,
    );
    let spelled = glyphs_of("2/3");
    let mut scratch = empty_frame();
    for t in (100..=1_200).step_by(25) {
        let drawn = frame(&mut show, &mut scratch, t * MS);
        let mut glyphs = table_glyphs(&drawn);
        // Records are drawn farthest first, so a turned card's glyphs are read left to right on
        // the screen.
        let camera = show.table().camera();
        glyphs.sort_by(|a, b| camera.to_screen(a.1).x.total_cmp(&camera.to_screen(b.1).x));
        let ids: Vec<u16> = glyphs.iter().map(|(glyph, _)| *glyph).collect();
        assert_eq!(ids, spelled, "the plaque spells 2/3 at {t} ms");
        let card = drawn
            .resolved
            .iter()
            .find(|element| element.id.0 == u32::from(slot.0))
            .expect("the bear is drawn");
        for (_, middle) in glyphs {
            let point = camera.to_screen(middle);
            assert!(
                inside(card.corners, point),
                "a glyph at {point} lies inside the card's corners {:?} at {t} ms",
                card.corners
            );
        }
    }
}

#[test]
fn a_permanent_with_two_kinds_of_counters_wears_two_badges() {
    let mut builder = ViewBuilder::new();
    let relic = builder.card(ZoneName::Battlefield, Side::You, "Relic");
    relic.types = vec![CardType::Artifact];
    relic.counters = ["time", "ice"]
        .into_iter()
        .map(|name| CounterView {
            kind: Counter::named(name).expect("a kind of counter"),
            count: 3,
        })
        .collect();
    let before = builder.update();
    let mut show = badged_show(&before);
    let drawn = frame(&mut show, &mut empty_frame(), 100 * MS);
    let panels = drawn.draws[..drawn.layer as usize]
        .iter()
        .filter(|draw| draw.material == Material::Panel as u8)
        .count();
    assert_eq!(panels, 2, "a badge for each kind of counter");
    let glyphs = table_glyphs(&drawn);
    let threes = glyphs_of("33");
    let ids: Vec<u16> = glyphs.iter().map(|(glyph, _)| *glyph).collect();
    assert_eq!(ids, threes, "each badge says 3");
}
