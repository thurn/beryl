//! The `point` verb and the arrows `light::arrows` draws: each case plays to its placement, an
//! arrow lasts from the script that begins it to the script that ends it, its ends stay on the
//! edges of what it joins while they fly, and "lines hold" holds over a played table.

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::{Act, Arrow, Span, StepTiming, Subject};
use beryl_protocol::testing::{ScriptBuilder, timing};
use game_play::light::arrows::RIBBON;
use game_play::play::Show;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use glam::Vec2;
use kit_base::data::space::light;
use kit_frame::frame::{Frame, Material, Role};
use kit_probe::agent::checks::{KeptArrow, keep_arrows, lines_hold};
use kit_probe::agent::{Context, Layout, TreeOptions};
use kit_ui::input::{Insets, Size};

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

fn desktop() -> Context {
    Context {
        recording: "point".to_owned(),
        seat: Side::You,
        layout: Layout::Desktop,
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
        let mut context = desktop();
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
    let before = played.last_frame();
    let mut kept = Vec::new();
    keep_arrows(&before, &mut kept);
    played.now += TICK;
    let mut after = played.last_frame();
    assert_eq!(lines_hold(&kept, &after, &desktop()), Vec::new());
    let index = after
        .resolved
        .iter()
        .position(|element| element.role == Role::Arrow)
        .expect("the arrow is drawn");
    after.resolved[index].corners[1].y += 5.0;
    assert_eq!(lines_hold(&kept, &after, &desktop()).len(), 1);
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
    let arrow = arrows(&drawn)[0];
    let table = played.show.table();
    let camera = table.camera();
    let anchor = camera.to_screen(table.player_pose(Side::Opponent).position);
    // The life orb's half-size, in pixels of the web's stage, whose 900 are the window's height.
    let half = light::PLAYER_HALF_PIXELS * (camera.window.size.height / 900.0);
    let inside = ((arrow.corners[3] - anchor) / half).length();
    assert!(
        (inside - (1.0 - light::onto::INTO)).abs() < 0.01,
        "{:?} ends {inside} of the way out from {anchor:?}",
        arrow.corners
    );
}
