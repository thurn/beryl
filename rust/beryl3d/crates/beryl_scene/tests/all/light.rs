//! The contract milestone 2's tracks share: every verb has a file of its own, spans reach the
//! table's `Lit` at their real time, and the effects are the data file's.

use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::{Arrow, Mark, Subject, Verb};
use beryl_scene::effects::Effect;
use beryl_scene::light::{ArrowSpan, Lit, MarkSpan, Span};
use beryl_scene::play::record::{Plant, Planted};
use beryl_scene::verbs::{self, Interval};

use crate::common::{MS, before, empty_frame, frame, show, three_beats};

const fn card(slot: u16) -> Subject {
    Subject::Object {
        slot: ObjectId(slot),
    }
}

const fn mark(slot: u16) -> Span {
    Span::Mark(MarkSpan {
        mark: Mark::Selected,
        on: card(slot),
    })
}

const fn over(from: u64, to: u64) -> Interval {
    Interval {
        from: from * MS,
        to: to * MS,
    }
}

#[test]
fn every_verb_has_a_play_function_and_says_whether_it_is_built() {
    let built: Vec<Verb> = Verb::ALL
        .iter()
        .copied()
        .filter(|verb| {
            // Taking the function is the check: the match over the verb files covers each.
            let _ = verbs::play(*verb);
            verbs::built(*verb)
        })
        .collect();
    assert_eq!(
        built,
        [
            Verb::Fly,
            Verb::Appear,
            Verb::Vanish,
            Verb::Emerge,
            Verb::Flip,
            Verb::Strike,
            Verb::Count,
            Verb::Set,
            Verb::Showcase,
            Verb::Lift
        ]
    );
}

#[test]
fn a_span_stands_whole_until_it_ends_and_is_forgotten_once_gone() {
    let mut lit = Lit::with_capacity(4);
    let arrow = Span::Arrow(ArrowSpan {
        arrow: Arrow::HostileTarget,
        from: card(1),
        to: Subject::Player {
            player: Side::Opponent,
        },
    });
    lit.begin(mark(1), over(0, 100));
    lit.begin(arrow, over(50, 150));
    assert_eq!(lit.whole_since(&mark(1)), Some(100 * MS));
    assert_eq!(lit.whole_since(&arrow), Some(150 * MS));
    assert_eq!(lit.whole_since(&mark(2)), None, "a span never begun");

    lit.end(&mark(1), over(200, 300));
    assert_eq!(lit.whole_since(&mark(1)), None, "a span going is not whole");
    lit.advance(300 * MS);
    assert_eq!(lit.open().len(), 2, "still going at its last instant");
    lit.advance(301 * MS);
    assert_eq!(
        lit.open().iter().map(|open| open.span).collect::<Vec<_>>(),
        [arrow],
        "the mark is gone"
    );
    lit.clear();
    assert!(lit.open().is_empty());
}

#[test]
fn a_span_a_step_begins_reaches_the_table_at_the_steps_real_time() {
    let (start, cards) = before();
    let (after, timings) = three_beats(&start, cards);
    let mut show = show();
    let mut buffer = empty_frame();
    // The token appears in the second beat, from 420 ms to 720 ms of the script.
    show.plant(Plant {
        kind: Planted::Mark,
        script: 1,
        t: 500 * MS,
    });
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after), timings);
    let token = mark(cards.token.0);
    frame(&mut show, &mut buffer, 0);
    frame(&mut show, &mut buffer, 419 * MS);
    assert!(show.lit().open().is_empty(), "nothing before the step");
    frame(&mut show, &mut buffer, 420 * MS);
    let open = show.lit().open();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].span, token);
    assert_eq!(open[0].begins, over(420, 720));
    assert_eq!(show.lit().whole_since(&token), Some(720 * MS));
    frame(&mut show, &mut buffer, 2000 * MS);
    assert_eq!(
        show.lit().open().len(),
        1,
        "a span outlives the script that began it"
    );
}

#[test]
fn the_effects_are_those_of_the_data_file() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../../data/ui-conventions.json"))
            .expect("the data file parses");
    let names: Vec<&str> = data["effects"]
        .as_array()
        .expect("a list of effects")
        .iter()
        .map(|effect| effect["name"].as_str().expect("each effect has a name"))
        .collect();
    let ours: Vec<&str> = Effect::ALL.iter().map(|effect| effect.name()).collect();
    assert_eq!(ours, names);
    for effect in Effect::ALL {
        assert_eq!(Effect::from_name(effect.name()), Some(effect));
    }
}
