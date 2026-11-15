//! Stress boards: positions at the engine's capacity, built by hand with no engine, that the
//! frame-time gates draw. The engine knows only a few dozen cards, so a board made by playing
//! it would repeat a handful of names, stack into piles and cost less to draw than a real one.
//! Here every permanent has a name of its own. Nothing reads a clock or a random number, so
//! two calls return equal values.

use beryl_session::PlayerView;
use beryl_session::protocol::{
    CardType, ObjectId, ObjectView, PlayerState, PowerToughness, Side, Why, ZoneName,
};
use beryl_session::script::{Act, Curve, Reset, Script, Step, StepTiming};

/// How many permanents move in a stress board's second view.
const FLIGHTS: usize = 40;
/// How long each flight takes, in milliseconds.
const FLIGHT_DURATION: f64 = 420.0;
/// How long after one flight the next one starts, in milliseconds.
const FLIGHT_STAGGER: f64 = 30.0;

/// A stress board: a view to start from and the view that follows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stress {
    /// The first view is a reset with every permanent at rest; each later view moves
    /// permanents with a script.
    pub views: Vec<PlayerView>,
    /// For each view after the first, one timing per step of its script, in script order:
    /// `timings[i]` belongs to `views[i + 1]`, whose script carries the same timings as a
    /// session's does.
    pub timings: Vec<Vec<StepTiming>>,
}

/// The name of every stress board.
pub const NAMES: [&str; 2] = ["stress-255", "stress-1024"];

/// The stress board called `name`: `stress-255` (a battlefield of 255 objects) or
/// `stress-1024` (1,024), or none for any other name.
#[must_use]
pub fn stress(name: &str) -> Option<Stress> {
    let count = match name {
        "stress-255" => 255,
        "stress-1024" => 1024,
        _ => return None,
    };
    Some(board(count))
}

/// A battlefield of `count` permanents, then the same one after `FLIGHTS` of them have moved
/// to its end.
fn board(count: u16) -> Stress {
    let mut first = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    first.objects = (0..count).map(permanent).collect();
    first.battlefield = first.objects.iter().map(|object| object.slot).collect();
    first.script = Some(Script {
        reset: Some(Reset::default()),
        beats: Vec::new(),
        timing: Vec::new(),
    });

    // Each moved permanent joins the end of the battlefield, after the ones that stay.
    let mut second = first.clone();
    let moved: Vec<ObjectView> = second.objects[..FLIGHTS].to_vec();
    second.battlefield.rotate_left(FLIGHTS);
    second.objects.rotate_left(FLIGHTS);
    let end = count - 1;
    let beat: Vec<Step> = moved
        .into_iter()
        .map(|object| Step {
            act: Act::Fly {
                from: ZoneName::Battlefield,
                why: Why::Effect,
                object,
                at: Some(end),
                item: None,
                origin: None,
            },
            cause: None,
        })
        .collect();
    let timings: Vec<StepTiming> = beat.iter().map(|_| flight_timing()).collect();
    second.script = Some(Script {
        reset: None,
        beats: vec![beat],
        timing: timings.clone(),
    });

    Stress {
        views: vec![first, second],
        timings: vec![timings],
    }
}

/// The permanent in slot `index + 1`: its own name; a creature or a land by turns, a third of
/// them tapped, and the controllers alternating every two so that each kind is split evenly.
fn permanent(index: u16) -> ObjectView {
    let side = if (index / 2).is_multiple_of(2) {
        Side::You
    } else {
        Side::Opponent
    };
    let mut object = ObjectView::card(
        ObjectId(index + 1),
        format!("Stress {:03}", index + 1),
        ZoneName::Battlefield,
        side,
        side,
    );
    object.tapped = index.is_multiple_of(3);
    if index.is_multiple_of(2) {
        object.types = vec![CardType::Creature];
        object.pt = Some(PowerToughness {
            power: 2,
            toughness: 2,
        });
    } else {
        object.types = vec![CardType::Land];
    }
    object
}

/// The hand-built timing of one flight: every step the same, and the next begins after
/// `FLIGHT_STAGGER`.
fn flight_timing() -> StepTiming {
    StepTiming {
        case: None,
        duration: FLIGHT_DURATION,
        delay: 0.0,
        stagger: FLIGHT_STAGGER,
        curve: Curve::EaseInOut,
        each: None,
        with: false,
        after: Vec::new(),
        hold: None,
        leads: false,
        passes: false,
        alone: false,
        opens: false,
        ahead: false,
        closes: None,
        effects: Vec::new(),
    }
}
