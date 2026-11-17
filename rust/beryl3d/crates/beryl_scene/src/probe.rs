//! The board the app draws to choose its tier (docs/beryl-3d.md, "Choosing a tier"), which is
//! also the frame-time gates' `stress-255` and `stress-1024` boards: a battlefield at the
//! engine's capacity, built by hand with no engine. The engine knows only a few dozen cards, so
//! a board made by playing it would repeat a handful of names, stack into piles and cost less to
//! draw than a real one. Here every permanent has a name of its own. Nothing reads a clock or a
//! random number, so two calls return equal values. Only a driver of the client and the app's
//! tier choice need it (the `driver` feature).

use beryl_protocol::protocol::{
    CardType, ObjectId, ObjectView, PlayerState, PlayerView, PowerToughness, Side, Why, ZoneName,
};
use beryl_protocol::script::{Act, Curve, Reset, Room, Step, StepTiming};

use crate::testing;

/// How many permanents move in the board's second view.
const FLIGHTS: usize = 40;
/// How long each flight takes, in milliseconds.
const FLIGHT_DURATION: f64 = 420.0;
/// How long after one flight the next one starts, in milliseconds.
const FLIGHT_STAGGER: f64 = 30.0;

/// A board's views and the timings of their scripts.
#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    /// The first view is a reset with every object at rest. Each later view carries a script.
    pub views: Vec<PlayerView>,
    /// For each view after the first, one timing per step of its script, in script order:
    /// `timings[i]` belongs to `views[i + 1]`.
    pub timings: Vec<Vec<StepTiming>>,
}

/// A battlefield of `count` permanents, then the same one after `FLIGHTS` of them have moved
/// to its end.
#[must_use]
pub fn board(count: u16) -> Board {
    let mut first = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    first.objects = (0..count).map(permanent).collect();
    first.battlefield = first.objects.iter().map(|object| object.slot).collect();
    first.script = Some(testing::script(
        Some(Reset::default()),
        Vec::new(),
        Vec::new(),
    ));

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
    second.script = Some(testing::script(None, vec![beat], timings.clone()));

    Board {
        views: vec![first, second],
        timings: vec![timings],
    }
}

/// The permanent in slot `index + 1`: its own name; a creature or a land by turns, a third of
/// them tapped, and the controllers alternating every two so that each kind is split evenly.
#[must_use]
pub fn permanent(index: u16) -> ObjectView {
    let side = if (index / 2).is_multiple_of(2) {
        Side::You
    } else {
        Side::Opponent
    };
    let mut object = card(index, ZoneName::Battlefield, side);
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

/// The card in slot `index + 1`, named `Stress <slot>`, in `zone` and owned and controlled by
/// `side`.
#[must_use]
pub fn card(index: u16, zone: ZoneName, side: Side) -> ObjectView {
    ObjectView::card(
        ObjectId(index + 1),
        format!("Stress {:03}", index + 1),
        zone,
        side,
        side,
    )
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
        room: Room::Step,
        phases: Vec::new(),
        effects: Vec::new(),
    }
}
