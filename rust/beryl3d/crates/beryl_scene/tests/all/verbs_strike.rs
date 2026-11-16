//! The verb `strike`: a creature lunges at what it strikes and comes back to its place
//! (docs/beryl-3d.md, "Verbs"), for each case of the verb in data/ui-conventions.json.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{CardType, ObjectId, PowerToughness, Side, ZoneName};
use beryl_protocol::script::{Act, Phase, StepTiming, Subject};
use beryl_scene::agent::steps::{no_jumps, rest_equals_placement, step_times};
use beryl_scene::input::{GlyphCell, GlyphKey};
use beryl_scene::play::Show;
use beryl_scene::play::record::ScriptRecord;
use beryl_scene::table::badges::STYLE;
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, timing};
use beryl_scene::text::{Glyphs, Text, glyph_key};
use glam::{Vec2, Vec3};

/// A strike's duration in both cases, in milliseconds, and its phases as shares of the step
/// (`strike` in data/ui-conventions.json). Neither case has a delay.
const DURATION: f64 = 1250.0;
const POISE: f64 = 0.32;
const HIT: f64 = 0.376;

/// The timing the session sends with a strike of `case`.
fn strike_timing(case: &str) -> StepTiming {
    StepTiming {
        case: Some(case.to_owned()),
        phases: vec![
            Phase {
                name: "poise".to_owned(),
                at: POISE,
            },
            Phase {
                name: "hit".to_owned(),
                at: HIT,
            },
        ],
        ..timing(DURATION)
    }
}

/// A show that has played the view with the viewer's attacking Serra Angel and the opponent's
/// Wall of Omens, then a script of one strike by the Angel at `at` (`None`: at the Wall).
struct Struck {
    show: Show,
    record: ScriptRecord,
    attacker: ObjectId,
    blocker: ObjectId,
}

fn strike(case: &str, at: Option<Subject>) -> Struck {
    strikes(case, at, false)
}

/// As `strike`, and with `back` the Wall strikes the Angel back in the same beat.
fn strikes(case: &str, at: Option<Subject>, back: bool) -> Struck {
    strikes_dealing(case, at, back, 0)
}

/// As `strikes`, with the Wall marked with `damage` in the view the script leads to.
fn strikes_dealing(case: &str, at: Option<Subject>, back: bool, damage: i32) -> Struck {
    let mut builder = ViewBuilder::new();
    let attacker = builder.card(ZoneName::Battlefield, Side::You, "Serra Angel");
    attacker.types = vec![CardType::Creature];
    attacker.tapped = true;
    let attacker = attacker.slot;
    let blocker = builder.card(ZoneName::Battlefield, Side::Opponent, "Wall of Omens");
    blocker.types = vec![CardType::Creature];
    // Its plaque shows its toughness less its damage.
    blocker.pt = Some(PowerToughness {
        power: 0,
        toughness: 4,
    });
    let blocker_record = blocker.clone();
    let blocker = blocker.slot;
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let to = at.unwrap_or(Subject::Object { slot: blocker });
    let object = matches!(to, Subject::Object { .. }).then_some(blocker_record);
    let mut acts = vec![Act::Strike {
        source: attacker,
        to,
        amount: 4,
        object,
    }];
    if back {
        acts.push(Act::Strike {
            source: blocker,
            to: Subject::Object { slot: attacker },
            amount: 0,
            object: None,
        });
    }
    let timings = acts
        .iter()
        .map(|_| StepTiming {
            with: back,
            ..strike_timing(case)
        })
        .collect();
    let mut after = start.view.clone();
    if let Some(wall) = after
        .objects
        .iter_mut()
        .find(|object| object.slot == blocker)
    {
        wall.damage = damage;
    }
    after.script = Some(ScriptBuilder::default().beat(acts).build());
    let mut show = show();
    show.record();
    shape_badges(&mut show);
    show.push(Box::new(start), Vec::new());
    show.push(
        Box::new(GameUpdate {
            session: beryl_protocol::protocol::SessionId(1),
            view: after,
        }),
        timings,
    );
    let mut drawn = empty_frame();
    frame(&mut show, &mut drawn, 0);
    let record = show
        .history()
        .last()
        .expect("the strike's script is recorded")
        .clone();
    Struck {
        show,
        record,
        attacker,
        blocker,
    }
}

/// Shapes the badges of `show`, with every glyph in the atlas, so that its plaques are drawn.
fn shape_badges(show: &mut Show) {
    const SCALE: f32 = 2.0;
    let mut wanted = Glyphs::default();
    let badges = show.badges_mut();
    badges.shape(&mut Text::new(), &mut wanted, SCALE);
    let cells: Vec<GlyphCell> = (0..2_000)
        .map(|glyph| GlyphCell {
            key: glyph_key(STYLE, SCALE, glyph),
            rect: [glyph, 0, 16, 20],
            bearing: [0, -20],
        })
        .collect();
    badges.add(1, &cells);
    let _: Vec<GlyphKey> = wanted.drain_wanted().collect();
}

impl Struck {
    /// The real time `share` of the way through the strike.
    fn at(&self, share: f64) -> u64 {
        let step = &self.record.steps[0];
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a share of a step that lasts a second"
        )]
        let offset = (share * (step.to - step.from) as f64).round() as u64;
        self.record
            .real(step.from + offset)
            .expect("the script has started")
    }

    fn position(&self, slot: ObjectId, now: u64) -> Vec3 {
        self.show
            .pose(usize::from(slot.0), now)
            .expect("the card is on the table")
            .position
    }

    /// Where the attacker's middle is on the screen at `now`.
    fn on_screen(&self, now: u64) -> Vec2 {
        self.show
            .table()
            .camera()
            .to_screen(self.position(self.attacker, now))
    }
}

/// Plays the strike to its end and checks that every object comes to rest at its placement,
/// with no jump, and that the strike moves its creature for as long as its step lasts.
fn plays_to_rest(struck: &mut Struck) {
    let end = struck.record.real(struck.record.end).expect("started");
    let mut drawn = empty_frame();
    for now in (0..=end + 100 * MS).step_by(usize::try_from(16 * MS).expect("fits")) {
        frame(&mut struck.show, &mut drawn, now);
    }
    assert!(
        struck.show.settled(),
        "the show is at rest after the strike"
    );
    let record = &struck.record;
    assert_eq!(rest_equals_placement(record), Vec::new());
    assert_eq!(no_jumps(record), Vec::new());
    assert_eq!(step_times(record), Vec::new());
    let track = record.track(usize::from(struck.attacker.0));
    assert!(track.len() >= 3, "the strike moves its creature: {track:?}");
    let last = track.last().expect("a segment").segment;
    assert_eq!(
        Some(last.to),
        record.finals[usize::from(struck.attacker.0)],
        "the last segment ends at the creature's placement"
    );
}

#[test]
fn each_case_of_strike_plays_to_its_placement() {
    for case in ["blocked attacker", "unblocked attacker"] {
        let player = Subject::Player {
            player: Side::Opponent,
        };
        let at = (case == "unblocked attacker").then_some(player);
        plays_to_rest(&mut strike(case, at));
    }
}

#[test]
fn a_strike_is_farthest_at_its_poise_and_nearest_at_its_hit() {
    let struck = strike("blocked attacker", None);
    let target = struck.position(struck.blocker, 0);
    let distance = |share: f64| {
        struck
            .position(struck.attacker, struck.at(share))
            .distance(target)
    };
    let samples: Vec<f32> = (0..=100).map(|n| distance(f64::from(n) / 100.0)).collect();
    let farthest = samples.iter().copied().fold(f32::MIN, f32::max);
    let nearest = samples.iter().copied().fold(f32::MAX, f32::min);
    assert!(
        distance(POISE) >= farthest - 0.01,
        "{} at the poise, {farthest} at most",
        distance(POISE)
    );
    assert!(
        distance(HIT) <= nearest + 0.01,
        "{} at the hit, {nearest} at least",
        distance(HIT)
    );
    assert!(
        distance(HIT) < distance(0.0),
        "the blow is nearer than the start"
    );
}

#[test]
fn a_strike_stops_outside_the_card_it_strikes() {
    let struck = strike("blocked attacker", None);
    let camera = struck.show.table().camera();
    let pose = struck
        .show
        .pose(usize::from(struck.blocker.0), struck.at(HIT))
        .expect("the blocker is on the table");
    let corners: Vec<Vec2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .into_iter()
        .map(|(x, y)| {
            let corner = Vec3::new(x * CARD_WIDTH / 2.0, y * CARD_HEIGHT / 2.0, 0.0);
            camera.to_screen(pose.transform().transform_point3(corner))
        })
        .collect();
    let blow = struck.on_screen(struck.at(HIT));
    // Inside a convex quad, the point is on the same side of every edge.
    let sides: Vec<f32> = (0..4)
        .map(|n| {
            let (a, b) = (corners[n], corners[(n + 1) % 4]);
            (b - a).perp_dot(blow - a)
        })
        .collect();
    let inside = sides.iter().all(|side| *side > 0.0) || sides.iter().all(|side| *side < 0.0);
    assert!(!inside, "{blow} is outside the corners {corners:?}");
}

#[test]
fn a_strike_at_a_player_dashes_toward_their_anchor() {
    let struck = strike(
        "unblocked attacker",
        Some(Subject::Player {
            player: Side::Opponent,
        }),
    );
    let table = struck.show.table();
    let anchor = table
        .camera()
        .to_screen(table.player_pose(Side::Opponent).position);
    let began = struck.on_screen(struck.at(0.0)).distance(anchor);
    let blow = struck.on_screen(struck.at(HIT)).distance(anchor);
    assert!(blow < began, "{blow} at the blow, {began} at the start");
}

#[test]
fn two_creatures_that_strike_each_other_meet_half_way() {
    let struck = strikes("blocked attacker", None, true);
    let hit = struck.at(HIT);
    let (attacker, blocker) = (
        struck.position(struck.attacker, hit),
        struck.position(struck.blocker, hit),
    );
    let (began, wall) = (
        struck.position(struck.attacker, 0),
        struck.position(struck.blocker, 0),
    );
    let middle = began.lerp(wall, 0.5);
    assert!(
        attacker.y < middle.y && blocker.y > middle.y,
        "the Angel at {attacker} and the Wall at {blocker} stop short of the middle {middle}"
    );
    assert!(blocker.y < wall.y, "the Wall strikes too");
}

/// What a blow does shows as it lands, as the web stage's `cover` has it: until the hit the
/// creature it strikes is drawn from the record the table showed before the script, and from
/// the hit from the record the script leads to, whether it strikes back (and so moves) or
/// stands still.
#[test]
fn the_damage_of_a_blow_shows_as_it_lands() {
    for back in [false, true] {
        let mut unhurt = strikes_dealing("blocked attacker", None, back, 0);
        let mut hurt = strikes_dealing("blocked attacker", None, back, 3);
        let mut scratch = empty_frame();
        let mut draws = |struck: &mut Struck, share: f64| {
            let now = struck.at(share);
            frame(&mut struck.show, &mut scratch, now).draws
        };
        for share in [0.1, HIT - 0.05] {
            assert!(
                draws(&mut unhurt, share) == draws(&mut hurt, share),
                "before its blow lands (at {share} of the strike, back {back}) the Wall shows no damage"
            );
        }
        assert!(
            draws(&mut unhurt, HIT + 0.05) != draws(&mut hurt, HIT + 0.05),
            "once the blow has landed (back {back}) the Wall shows its damage"
        );
    }
}
