//! The verb `streak`: mana or damage streams from one thing to another, drawn by
//! `light::streaks` while its step plays (docs/beryl-3d.md, "Verbs" and "Effects"), for each
//! case of the verb in data/ui-conventions.json.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{
    CardType, Mana, ObjectId, ObjectView, Side, StackItem, Why, ZoneName,
};
use beryl_protocol::script::{Act, Curve, StepTiming, Stream, Subject};
use game_play::light::streaks::RECORDS;
use game_play::play::Show;
use game_play::play::record::ScriptRecord;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use game_probe::testing::{ScriptBuilder, ViewBuilder, timing};
use glam::Vec3;
use kit_frame::frame::{Frame, Material};

/// The timing the session sends with a streak of `case`: its duration and delay in
/// milliseconds (`streak` in data/ui-conventions.json), easing in.
fn streak_timing(case: &str) -> StepTiming {
    let (duration, delay) = match case {
        "mana" => (275.0, 0.0),
        _ => (250.0, 100.0),
    };
    StepTiming {
        case: Some(case.to_owned()),
        delay,
        curve: Curve::EaseIn,
        ..timing(duration)
    }
}

/// A show that has played a view with the viewer's Island and Shock on the stack and the
/// opponent's Wall of Omens, then a script whose one beat is `acts`.
struct Streaked {
    show: Show,
    record: ScriptRecord,
    island: ObjectId,
    spell: ObjectId,
    wall: ObjectId,
}

/// The table every test starts from, and the slots of its cards.
fn table() -> (ViewBuilder, ObjectId, ObjectId, ObjectId, ObjectView) {
    let mut builder = ViewBuilder::new();
    let island = builder
        .card(ZoneName::Battlefield, Side::You, "Island")
        .slot;
    let spell = builder.card(ZoneName::Stack, Side::You, "Shock").slot;
    let wall = builder.card(ZoneName::Battlefield, Side::Opponent, "Wall of Omens");
    wall.types = vec![CardType::Creature];
    let wall = wall.slot;
    let held = builder
        .card(ZoneName::Hand, Side::You, "Counterspell")
        .clone();
    builder.script(ScriptBuilder::reset().build());
    (builder, island, spell, wall, held)
}

/// Plays the start of `table` and then a script of one beat of `acts` with their timings, in a
/// view `change` makes from the start's.
fn play(
    acts: Vec<Act>,
    timings: Vec<StepTiming>,
    change: impl FnOnce(&mut beryl_protocol::PlayerView),
) -> Streaked {
    let (builder, island, spell, wall, _) = table();
    let start = builder.update();
    let mut after = start.view.clone();
    change(&mut after);
    after.script = Some(ScriptBuilder::default().beat(acts).build());
    let mut show = show();
    show.record();
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
        .expect("the script is recorded")
        .clone();
    Streaked {
        show,
        record,
        island,
        spell,
        wall,
    }
}

/// Blue mana from the Island into the Shock.
fn mana(from: Option<Subject>) -> Streaked {
    let (_, island, spell, ..) = table();
    let act = Act::Streak {
        from: Some(from.unwrap_or(Subject::Object { slot: island })),
        to: Subject::Object { slot: spell },
        stream: Stream::Mana { color: Mana::Blue },
        amount: 1,
        object: None,
    };
    play(vec![act], vec![streak_timing("mana")], |_| {})
}

/// Two damage from the Shock to the Wall, which carries its new record.
fn damage() -> Streaked {
    let (builder, _, spell, wall, _) = table();
    let mut record = builder.build().objects[usize::from(wall.0)].clone();
    record.damage = 2;
    let marked = record.clone();
    let act = Act::Streak {
        from: Some(Subject::Object { slot: spell }),
        to: Subject::Object { slot: wall },
        stream: Stream::Damage,
        amount: 2,
        object: Some(record),
    };
    play(vec![act], vec![streak_timing("damage")], move |view| {
        let slot = usize::from(marked.slot.0);
        view.objects[slot] = marked;
    })
}

impl Streaked {
    /// The real start and end of the script's step `number`.
    fn step(&self, number: usize) -> (u64, u64) {
        let step = &self.record.steps[number];
        (
            self.record.real(step.from).expect("started"),
            self.record.real(step.to).expect("started"),
        )
    }

    fn frame(&mut self, now: u64) -> Frame {
        let mut drawn = empty_frame();
        frame(&mut self.show, &mut drawn, now)
    }

    fn position(&self, slot: ObjectId, now: u64) -> Vec3 {
        self.show
            .pose(usize::from(slot.0), now)
            .expect("the card is on the table")
            .position
    }
}

/// Where each streak record of `frame` is, in the order they were written.
fn streaks(frame: &Frame) -> Vec<Vec3> {
    frame
        .draws
        .iter()
        .filter(|draw| draw.material == Material::Streak as u8)
        .map(|draw| Vec3::new(draw.model[3], draw.model[7], draw.model[11]))
        .collect()
}

/// Plays the script to its end and checks that every object comes to rest at its placement,
/// with no jump, and that each step starts and ends on time.
fn plays_to_rest(streaked: &mut Streaked) {
    let end = streaked.record.real(streaked.record.end).expect("started");
    for now in (0..=end + 100 * MS).step_by(usize::try_from(16 * MS).expect("fits")) {
        streaked.frame(now);
    }
    assert!(streaked.show.settled(), "the show is at rest");
    let record = &streaked.record;
    assert_eq!(rest_equals_placement(record), Vec::new());
    assert_eq!(no_jumps(record), Vec::new());
    assert_eq!(step_times(record), Vec::new());
}

#[test]
fn each_case_of_streak_plays_to_its_placement() {
    plays_to_rest(&mut mana(None));
    plays_to_rest(&mut damage());
}

#[test]
fn a_streak_is_drawn_while_its_step_plays_and_at_no_other_time() {
    for mut streaked in [mana(None), damage()] {
        let (from, to) = streaked.step(0);
        assert!(to > from, "the step takes time");
        let mut drawn = 0;
        for now in (0..to + 50 * MS).step_by(usize::try_from(5 * MS).expect("fits")) {
            let records = streaks(&streaked.frame(now));
            if (from..to).contains(&now) {
                assert_eq!(records.len(), RECORDS, "one streak at {now}");
                drawn += 1;
            } else {
                assert_eq!(records, Vec::new(), "no streak at {now}");
            }
        }
        assert!(drawn > 10, "the streak is drawn in {drawn} frames");
    }
}

#[test]
fn a_streaks_head_nears_its_target_as_it_goes() {
    let mut streaked = damage();
    let (from, to) = streaked.step(0);
    let target = streaked.position(streaked.wall, from);
    let mut last = f32::INFINITY;
    for share in 1..10 {
        let now = from + (to - from) * share / 10;
        let head = *streaks(&streaked.frame(now)).last().expect("a head");
        let far = head.distance(target);
        assert!(far < last, "{far} at {share} tenths, {last} before");
        last = far;
    }
    assert!(
        streaked.show.table().camera().depth(target) > 0.0,
        "the Wall is before the eye"
    );
}

#[test]
fn a_streak_of_damage_leaves_its_target_with_the_record_it_carries() {
    let mut streaked = damage();
    let (_, to) = streaked.step(0);
    streaked.frame(to + 50 * MS);
    let wall = streaked.wall;
    assert_eq!(
        streaked.show.table().rest(usize::from(wall.0)),
        streaked.show.pose(usize::from(wall.0), to + 50 * MS),
        "the Wall rests where it lay"
    );
    assert_eq!(
        streaked.position(wall, 0),
        streaked.position(wall, to + 50 * MS),
        "nothing moves the Wall"
    );
}

#[test]
fn mana_from_a_pool_leaves_the_players_pool() {
    let mut streaked = mana(Some(Subject::Player { player: Side::You }));
    let (from, to) = streaked.step(0);
    let pool = streaked.show.table().pool_pose(Side::You).position;
    let island = streaked.position(streaked.island, from);
    // Early in the step the tail has not left its source: its first record is by the pool.
    let tail = streaks(&streaked.frame(from + (to - from) / 10))[0];
    assert!(
        tail.distance(pool) < tail.distance(island),
        "{tail} is nearer the pool at {pool} than the Island at {island}"
    );
    let spell = streaked.show.reach(
        Subject::Object {
            slot: streaked.spell,
        },
        from,
    );
    let way = pool.distance(spell.expect("the Shock is on the stack").pose.position);
    assert!(
        tail.distance(pool) < 0.05 * way,
        "{tail} is near the pool at {pool}, on a way {way} long"
    );
}

#[test]
fn a_streak_with_no_source_on_the_table_is_not_drawn() {
    let (_, _, spell, ..) = table();
    let act = Act::Streak {
        from: None,
        to: Subject::Object { slot: spell },
        stream: Stream::Mana { color: Mana::Red },
        amount: 1,
        object: None,
    };
    let mut streaked = play(vec![act], vec![streak_timing("mana")], |_| {});
    let (from, to) = streaked.step(0);
    assert_eq!(
        streaks(&streaked.frame(u64::midpoint(from, to))),
        Vec::new()
    );
}

#[test]
fn a_streak_to_a_card_in_flight_lands_on_it_where_it_is() {
    // The held Counterspell is cast, flying from the hand to the stack, while the Island's mana
    // streams into it in the same beat.
    let (_, island, _, _, in_hand) = table();
    let slot = in_hand.slot;
    let mut cast = in_hand.clone();
    cast.zone = ZoneName::Stack;
    let item = StackItem::Spell {
        slot,
        targets: Vec::new(),
        x: 0,
        paid: Vec::new(),
    };
    let acts = vec![
        Act::Fly {
            from: ZoneName::Hand,
            why: Why::Cast,
            object: cast.clone(),
            at: None,
            item: Some(item.clone()),
            origin: None,
        },
        Act::Streak {
            from: Some(Subject::Object { slot: island }),
            to: Subject::Object { slot },
            stream: Stream::Mana { color: Mana::Blue },
            amount: 1,
            object: None,
        },
    ];
    let timings = vec![
        StepTiming {
            case: Some("cast".to_owned()),
            ..timing(400.0)
        },
        StepTiming {
            with: true,
            ..streak_timing("mana")
        },
    ];
    let mut streaked = play(acts, timings, move |view| {
        view.objects[usize::from(slot.0)] = cast;
        view.stack.push(item);
    });
    let (from, to) = streaked.step(1);
    let mut moved = false;
    for share in [50, 80, 999] {
        let now = from + (to - from) * share / 1000;
        let target = streaked.position(slot, now);
        moved |= target != streaked.position(slot, from);
        let head = *streaks(&streaked.frame(now)).last().expect("a head");
        if share == 999 {
            let way = streaked.position(island, now).distance(target);
            assert!(
                head.distance(target) < 0.01 * way,
                "the head at {head} lands on the card at {target}, on a way {way} long"
            );
        }
    }
    assert!(moved, "the card is in flight while the streak runs");
}
