//! The `point` verb and the arrows `light::arrows` draws: each case plays to its placement, an
//! arrow lasts from the script that begins it to the script that ends it, its ends stay on the
//! edges of what it joins while they fly, and "lines hold" holds over a played table.

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::{Act, Arrow, Span, StepTiming, Subject};
use beryl_protocol::testing::{ScriptBuilder, timing};
use game_play::art::orb;
use game_play::light::arrows::RIBBON;
use game_play::play::Show;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use glam::{Vec2, Vec3};
use kit_base::data::profiles::SizeClass;
use kit_base::tokens::color;
use kit_frame::frame::{
    Band, Draw, ElementId, Endpoint, Frame, FrameBuilder, LineStyle, Material, Role, ShapeAnchor,
    Vfx,
};
use kit_probe::agent::checks::{KeptArrow, keep_arrows, lines_hold, relations_hold};
use kit_probe::agent::{Context, TreeOptions};
use kit_ui::input::{Insets, Size, Tier};

use crate::common::{Cards, MS, before, discard, draw, empty_frame, frame, show};

/// The time between two frames, in nanoseconds.
const TICK: u64 = 16 * MS;

const fn card(slot: ObjectId) -> Subject {
    Subject::Object { slot }
}

/// The timing the session sends with a step of `point`'s `case` (data/ui-conventions.json).
fn point_timing(case: &str) -> StepTiming {
    let (duration, delay) = match case {
        "source line" => (33.0, 450.0),
        "target arrow" | "stack arrow" => (33.0, 0.0),
        "block line" | "attack line" => (66.0, 0.0),
        "end" => (33.0, 290.0),
        "source line end" => (33.0, 170.0),
        _ => panic!("no case {case}"),
    };
    StepTiming {
        case: Some(case.to_owned()),
        delay,
        ..timing(duration)
    }
}

fn point(span: Span, arrow: Arrow, from: Subject, to: Subject) -> Act {
    Act::Point {
        span,
        arrow,
        from,
        to,
    }
}

/// `before`'s view with a script of one beat for each act, timed by its timing.
fn pointed(before: &GameUpdate, acts: Vec<(Act, StepTiming)>) -> (GameUpdate, Vec<StepTiming>) {
    let mut view = before.view.clone();
    let mut script = ScriptBuilder::default();
    let mut timings = Vec::new();
    for (act, timing) in acts {
        script.beat([act]);
        timings.push(timing);
    }
    view.script = Some(script.build());
    (
        GameUpdate {
            session: before.session,
            view,
        },
        timings,
    )
}

/// A show that keeps a record of its scripts, drawn one tick at a time.
struct Played {
    show: Show,
    drawn: Frame,
    now: u64,
}

impl Played {
    /// The sample table of `common::before`, at rest.
    fn new() -> (Self, GameUpdate, Cards) {
        let (start, cards) = before();
        let mut played = Self {
            show: show(),
            drawn: empty_frame(),
            now: 0,
        };
        played.show.record();
        played.show.push(Box::new(start.clone()), Vec::new());
        played.settle(|_, _| {});
        (played, start, cards)
    }

    /// Gives the show `update` and draws it tick by tick until it is at rest, calling `each`
    /// with every frame and its time.
    fn play(
        &mut self,
        (update, timings): (GameUpdate, Vec<StepTiming>),
        each: impl FnMut(&Frame, u64),
    ) -> GameUpdate {
        self.show.push(Box::new(update.clone()), timings);
        self.settle(each);
        update
    }

    fn settle(&mut self, mut each: impl FnMut(&Frame, u64)) {
        loop {
            let drawn = frame(&mut self.show, &mut self.drawn, self.now);
            each(&drawn, self.now);
            if self.show.settled() && self.now > 0 {
                break;
            }
            self.now += TICK;
        }
        // One more frame, so that what the last script began has finished fading in.
        self.now += TICK;
        let drawn = frame(&mut self.show, &mut self.drawn, self.now);
        each(&drawn, self.now);
    }

    fn last_frame(&mut self) -> Frame {
        frame(&mut self.show, &mut self.drawn, self.now)
    }
}

fn arrows(frame: &Frame) -> Vec<&kit_frame::frame::Resolved> {
    frame
        .resolved
        .iter()
        .filter(|element| element.role == Role::Arrow)
        .collect()
}

/// The box round the corners of the card named `name` on the screen.
fn card_box(frame: &Frame, name: &str) -> Option<(Vec2, Vec2)> {
    let element = frame
        .resolved
        .iter()
        .find(|element| element.role == Role::Card && frame.name(element.name) == name)?;
    let low = element
        .corners
        .iter()
        .fold(Vec2::INFINITY, |a, &b| a.min(b));
    let high = element
        .corners
        .iter()
        .fold(Vec2::NEG_INFINITY, |a, &b| a.max(b));
    Some((low, high))
}

/// Whether `point` lies on the edge of the box from `low` to `high`, within half a point.
fn on_edge(point: Vec2, (low, high): (Vec2, Vec2)) -> bool {
    let near = 0.5;
    let inside = point.cmpge(low - near).all() && point.cmple(high + near).all();
    let side = (point.x - low.x).abs() < near
        || (point.x - high.x).abs() < near
        || (point.y - low.y).abs() < near
        || (point.y - high.y).abs() < near;
    inside && side
}

fn regular_context() -> Context {
    Context {
        size_class: SizeClass::Regular,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(SizeClass::Regular)
            .checks,
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        safe_area: Insets::default(),
        at: 0,
    }
}

#[test]
fn each_case_of_point_plays_to_its_placement() {
    let cases = [
        ("source line", Arrow::Source),
        ("target arrow", Arrow::FriendlyTarget),
        ("stack arrow", Arrow::HostileTarget),
        ("block line", Arrow::Block),
        ("attack line", Arrow::Attack),
        ("end", Arrow::FriendlyTarget),
        ("source line end", Arrow::Source),
    ];
    for (case, arrow) in cases {
        let (mut played, start, cards) = Played::new();
        let (from, to) = (card(cards.angel), card(cards.island));
        let mut acts = Vec::new();
        if case.ends_with("end") {
            let begins = if arrow == Arrow::Source {
                "source line"
            } else {
                "target arrow"
            };
            acts.push((point(Span::Begin, arrow, from, to), point_timing(begins)));
            acts.push((point(Span::End, arrow, from, to), point_timing(case)));
        } else {
            acts.push((point(Span::Begin, arrow, from, to), point_timing(case)));
        }
        played.play(pointed(&start, acts), |_, _| {});
        let record = played.show.history().last().expect("recorded").clone();
        assert_eq!(rest_equals_placement(&record), Vec::new(), "{case}");
        assert_eq!(no_jumps(&record), Vec::new(), "{case}");
        assert_eq!(step_times(&record), Vec::new(), "{case}");
        let last = played.last_frame();
        let shown = arrows(&last).len();
        let expected = usize::from(!case.ends_with("end"));
        assert_eq!(shown, expected, "{case}: arrows shown at its end");
        // The element alone does not show that the arrow is seen: its ribbon must give light.
        // A glow `progress` 1 of the way through gives none, and arrows were once drawn so.
        if expected == 1 {
            assert!(
                lit_glows(&last) >= RIBBON,
                "{case}: the arrow's ribbon gives light"
            );
        }
    }
}

/// How many glow records of `frame` give any light: some opacity, not faded out by their
/// progress (`Draw::glow`).
fn lit_glows(frame: &Frame) -> usize {
    frame
        .draws
        .iter()
        .filter(|draw| draw.material == Material::Glow as u8)
        .filter(|draw| draw.tint[3] > 0 && draw.params[0] < 1.0)
        .count()
}

#[test]
fn an_arrow_lasts_across_scripts_until_the_script_that_ends_it() {
    let (mut played, start, cards) = Played::new();
    let arrow = point(
        Span::Begin,
        Arrow::HostileTarget,
        card(cards.angel),
        card(cards.island),
    );
    let begun = played.play(
        pointed(&start, vec![(arrow, point_timing("target arrow"))]),
        |_, _| {},
    );
    let drawn = played.play(draw(&begun, cards), |_, _| {});
    let discarded = played.play(discard(&drawn, cards), |_, _| {});
    let tree = kit_probe::agent::tree(&played.last_frame(), &TreeOptions::default())
        .expect("the whole tree");
    assert!(
        tree.lines()
            .any(|line| line.trim_start().starts_with("arrow \"hostile-target\"")),
        "the arrow is still shown after two more scripts:\n{tree}"
    );
    let end = point(
        Span::End,
        Arrow::HostileTarget,
        card(cards.angel),
        card(cards.island),
    );
    played.play(
        pointed(&discarded, vec![(end, point_timing("end"))]),
        |_, _| {},
    );
    assert_eq!(arrows(&played.last_frame()).len(), 0, "the arrow is gone");
}

#[test]
fn an_arrows_ends_stay_on_its_cards_edges_while_one_flies() {
    let (mut played, start, cards) = Played::new();
    // The line from the Counterspell in hand to the Angel, then the Counterspell flies to the
    // graveyard under it.
    let line = point(
        Span::Begin,
        Arrow::Source,
        card(cards.discard),
        card(cards.angel),
    );
    let begun = played.play(
        pointed(&start, vec![(line, point_timing("source line"))]),
        |_, _| {},
    );
    let mut checked = 0;
    let mut kept: Vec<KeptArrow> = Vec::new();
    let mut held = Vec::new();
    played.play(discard(&begun, cards), |frame, now| {
        let mut context = regular_context();
        context.at = now;
        held.extend(lines_hold(&kept, frame, &context));
        keep_arrows(frame, &mut kept);
        let Some(arrow) = arrows(frame).first().copied() else {
            return;
        };
        let from = card_box(frame, "Counterspell").expect("the Counterspell is drawn");
        let to = card_box(frame, "Serra Angel").expect("the Angel is drawn");
        assert!(
            on_edge(arrow.corners[0], from),
            "at {now}: {:?} is on the edge of {from:?}",
            arrow.corners[0]
        );
        assert!(
            on_edge(arrow.corners[3], to),
            "at {now}: {:?} is on the edge of {to:?}",
            arrow.corners[3]
        );
        checked += 1;
    });
    assert!(
        checked > 10,
        "the line was drawn through the flight: {checked}"
    );
    assert_eq!(held, Vec::new(), "lines hold over the flight");
}

#[test]
fn lines_hold_finds_a_planted_reshape() {
    let (mut played, start, cards) = Played::new();
    let arrow = point(
        Span::Begin,
        Arrow::FriendlyTarget,
        card(cards.island),
        card(cards.angel),
    );
    played.play(
        pointed(&start, vec![(arrow, point_timing("target arrow"))]),
        |_, _| {},
    );
    let context = regular_context();
    let before = played.last_frame();
    let mut kept = Vec::new();
    keep_arrows(&before, &mut kept);
    played.now += TICK;
    let mut after = played.last_frame();
    assert_eq!(lines_hold(&kept, &after, &context), Vec::new());
    let index = after
        .resolved
        .iter()
        .position(|element| element.role == Role::Arrow)
        .expect("the arrow is drawn");
    after.resolved[index].corners[1].y += 5.0;
    assert_eq!(lines_hold(&kept, &after, &context).len(), 1);
    assert_eq!(relations_hold(&after, &context), Vec::new());
    after.resolved[index].corners[3].y += 5.0;
    assert_eq!(relations_hold(&after, &context).len(), 1);
}

/// An arrow at a player comes onto their life total, an oval, as the web's `reachOfLife` makes
/// it: its tip lies `light.onto.into` of the way in from the oval's rim toward its middle, on the
/// way the arrow comes in.
#[test]
fn an_arrow_at_a_player_ends_inside_their_life_total() {
    let (mut played, start, cards) = Played::new();
    let opponent = Subject::Player {
        player: Side::Opponent,
    };
    let arrow = point(
        Span::Begin,
        Arrow::HostileTarget,
        card(cards.angel),
        opponent,
    );
    played.play(
        pointed(&start, vec![(arrow, point_timing("target arrow"))]),
        |_, _| {},
    );
    let drawn = played.last_frame();
    let index = drawn
        .resolved
        .iter()
        .position(|element| element.role == Role::Arrow)
        .expect("the arrow is drawn");
    let arrow = &drawn.resolved[index];
    let Some(ShapeAnchor::Line([from, to])) = drawn.decoration_of(u32::try_from(index).unwrap())
    else {
        panic!("the arrow has two anchors");
    };
    assert_eq!(
        from.target,
        Endpoint::Node(ElementId(u32::from(cards.angel.0)))
    );
    assert_eq!(to.target, Endpoint::Node(orb::target_id(Side::Opponent)));
    let table = played.show.table();
    let light = &table.data().light;
    let camera = table.camera();
    let anchor = table.life_centre(Side::Opponent)
        + Vec2::Y * orb::face_offset(Side::Opponent) * table.layout().play.life.orb_scale;
    // The life orb's half-size, in pixels of the web's stage, whose 900 are the window's height.
    let half = light.player_half_pixels * (camera.window.size.height / 900.0);
    let inside = ((arrow.corners[3] - anchor) / half).length();
    assert!(
        (inside - (1.0 - light.onto.into)).abs() < 0.01,
        "{:?} ends {inside} of the way out from {anchor:?}",
        arrow.corners
    );
}

/// The player marker sits on the HUD. A ribbon to it can use the same screen-space
/// width limit as a ribbon to a card. It ends in its own tapered tip.
#[test]
fn a_target_ribbon_has_screen_sized_glow_and_one_tapered_tip() {
    let (mut played, start, cards) = Played::new();
    let from = card(cards.angel);
    for to in [card(cards.island), Subject::Player { player: Side::You }] {
        let target = |span| {
            (
                point(span, Arrow::FriendlyTarget, from, to),
                point_timing(if span == Span::Begin {
                    "target arrow"
                } else {
                    "end"
                }),
            )
        };
        let begun = played.play(pointed(&start, vec![target(Span::Begin)]), |_, _| {});
        let drawn = played.last_frame();
        let camera = played.show.table().camera();
        let shown = arrows(&drawn);
        let [arrow] = shown.as_slice() else {
            panic!("expected one arrow to {to:?}, found {}", shown.len());
        };
        let project = |model: [f32; 12], local: Vec3| {
            let column = |index: usize| Vec3::new(model[index], model[index + 4], model[index + 8]);
            camera.to_screen(
                column(3) + column(0) * local.x + column(1) * local.y + column(2) * local.z,
            )
        };
        let halo: Vec<&Draw> = drawn
            .draws
            .iter()
            .filter(|draw| {
                draw.material == Material::Glow as u8
                    && draw.tint[..3] == color::FRIENDLY_TARGET[..3]
                    && draw.tint[3] > 0
            })
            .collect();
        assert_eq!(halo.len(), RIBBON, "one strip, with no separate arrowhead");
        let tip = arrow.corners[3];
        let mut tip_widths = Vec::new();
        for draw in halo {
            let cross = project(draw.model, Vec3::Y).distance(project(draw.model, Vec3::ZERO));
            // At a 900-point window the Web's 7.2-point body can flare by at most
            // 1.5 and grow with the endpoint by at most 1.4. Its halo is 2.6 body
            // widths plus 16 points to either side (less than 112 points across).
            assert!(cross < 112.0, "glow to {to:?} spans {cross:.1} points");
            for (local_x, taper) in [(-0.5, draw.params[1]), (0.5, draw.params[2])] {
                let at = project(draw.model, Vec3::X * local_x);
                if at.distance(tip) < 0.01 {
                    tip_widths.push(cross * taper);
                }
            }
        }
        assert!(
            matches!(tip_widths.as_slice(), [width] if *width < 0.7),
            "ribbon to {to:?} should have one narrow tip at {tip}, found {tip_widths:?}"
        );
        played.play(pointed(&begun, vec![target(Span::End)]), |_, _| {});
        assert!(arrows(&played.last_frame()).is_empty());
    }
}

#[test]
fn enhanced_arrows_emit_vfx_ribbons_and_spearheads_in_layer_cover() {
    let cases = [
        ("source line", Arrow::Source, LineStyle::Source),
        (
            "target arrow",
            Arrow::FriendlyTarget,
            LineStyle::FriendlyTarget,
        ),
        (
            "stack arrow",
            Arrow::HostileTarget,
            LineStyle::HostileTarget,
        ),
        ("block line", Arrow::Block, LineStyle::Block),
        ("attack line", Arrow::Attack, LineStyle::Attack),
    ];
    for (case, arrow, expected_style) in cases {
        let (mut played, start, cards) = Played::new();
        let act = point(Span::Begin, arrow, card(cards.angel), card(cards.island));
        played.play(pointed(&start, vec![(act, point_timing(case))]), |_, _| {});

        let mut enhanced_buf = empty_frame();
        let mut builder = FrameBuilder::new(
            &mut enhanced_buf,
            Tier::Enhanced,
            crate::test_data::data().card.size(),
        );
        played.show.frame(played.now, &mut builder);
        let enhanced = builder.finish();
        assert!(
            enhanced.ambient_motion,
            "{case}: enhanced arrows request continuous animation"
        );

        let band = Band::LayerCover as usize;
        let cover_draws =
            &enhanced.draws[enhanced.bands[band] as usize..enhanced.bands[band + 1] as usize];
        let ribbons = cover_draws
            .iter()
            .filter(|draw| {
                draw.material == Material::Vfx as u8
                    && draw.uv[0] == Vfx::LineRibbon as u16
                    && draw.tint[3] > 0
                    && draw.uv[3] == u16::from(expected_style as u8)
            })
            .count();
        let heads = cover_draws
            .iter()
            .filter(|draw| {
                draw.material == Material::Vfx as u8
                    && draw.uv[0] == Vfx::LineHead as u16
                    && draw.tint[3] > 0
                    && draw.uv[3] == u16::from(expected_style as u8)
            })
            .count();
        assert_eq!(
            ribbons, RIBBON,
            "{case}: expected {RIBBON} Vfx::LineRibbon segments in LayerCover"
        );
        assert_eq!(
            heads, 1,
            "{case}: expected one Vfx::LineHead spearhead in LayerCover"
        );
    }
}

/// Focused check on Enhanced-tier spearhead orientation against the last ribbon segment.
/// No generic `kit_probe` check can express this property because generic checks inspect
/// `Frame::resolved` (`Role::Arrow` Bezier control points), whereas the procedural ribbon segments
/// (`Vfx::LineRibbon`) and spearhead (`Vfx::LineHead`) are GPU `Draw` records in
/// `Band::LayerCover` whose screen-space orientations live only in `Draw::model`.
#[test]
fn graveyard_target_arrow_spearhead_points_along_last_ribbon_segment() {
    let (mut played, start, cards) = Played::new();
    let (discarded, timings) = discard(&start, cards);
    played.play((discarded.clone(), timings), |_, _| {});

    let act = point(
        Span::Begin,
        Arrow::FriendlyTarget,
        card(cards.angel),
        card(cards.discard),
    );
    played.play(
        pointed(&discarded, vec![(act, point_timing("target arrow"))]),
        |_, _| {},
    );

    let mut enhanced_buf = empty_frame();
    let mut builder = FrameBuilder::new(
        &mut enhanced_buf,
        Tier::Enhanced,
        crate::test_data::data().card.size(),
    );
    played.show.frame(played.now, &mut builder);
    let enhanced = builder.finish();
    let camera = played.show.table().camera();

    let band = Band::LayerCover as usize;
    let cover_draws =
        &enhanced.draws[enhanced.bands[band] as usize..enhanced.bands[band + 1] as usize];
    let last_ribbon = cover_draws
        .iter()
        .rfind(|draw| {
            draw.material == Material::Vfx as u8
                && draw.uv[0] == Vfx::LineRibbon as u16
                && draw.tint[3] > 0
                && draw.uv[3] == u16::from(LineStyle::FriendlyTarget as u8)
        })
        .expect("graveyard target arrow should emit Vfx::LineRibbon segments");
    let head = cover_draws
        .iter()
        .find(|draw| {
            draw.material == Material::Vfx as u8
                && draw.uv[0] == Vfx::LineHead as u16
                && draw.tint[3] > 0
                && draw.uv[3] == u16::from(LineStyle::FriendlyTarget as u8)
        })
        .expect("graveyard target arrow should emit one Vfx::LineHead spearhead");

    let forward_screen = |model: [f32; 12]| {
        let column = |index: usize| Vec3::new(model[index], model[index + 4], model[index + 8]);
        let start = camera.to_screen(column(3) - column(0) * 0.5);
        let end = camera.to_screen(column(3) + column(0) * 0.5);
        (end - start).normalize()
    };
    let ribbon_dir = forward_screen(last_ribbon.model);
    let head_dir = forward_screen(head.model);
    let alignment = ribbon_dir.dot(head_dir);
    assert!(
        alignment > 0.99,
        "spearhead direction {head_dir} must align with last ribbon segment {ribbon_dir} (dot = {alignment:.4})"
    );
}
