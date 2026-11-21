//! `tap`: a permanent turns a quarter as it taps or untaps, about its own middle where it lies,
//! and only then goes to where it now rests (the web stage's `turned` in
//! `ui/src/stage/tracks.ts`). The turn takes the share of the step the step's `Room::From`
//! gives, the share its row waits before making room, so that nothing crosses the card while
//! it turns. Where it rests within `verbs.tap.near` of a card's width of where it lay
//! (`data/beryl3d/space.json`), it turns straight onto its place, in one motion. The turn takes
//! the shortest way, as `Segment::pose_at` turns a rotation.

use beryl_protocol::script::Room;

use super::{Interval, StepIn, StepOut};
use game_table::table::CARD_WIDTH;
use kit_base::pose::{Pose, Segment};

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(from), Some(to)) = (step.subject, step.from, step.to) else {
        return;
    };
    // Where it lay, turned as it now rests.
    let home = Pose {
        rotation: to.rotation,
        ..from
    };
    let onto = if from.position.truncate().distance(to.position.truncate())
        < step.verbs.tap.near * CARD_WIDTH
    {
        to
    } else {
        home
    };
    let share = match step.timing.room {
        Room::From { share } => share,
        Room::Step | Room::For { .. } => 1.0,
    };
    let own = step.own;
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a step lasts under a second, and the share is between 0 and 1"
    )]
    let turned = own.from + (own.duration() as f64 * share).round() as u64;
    let turn = Interval {
        from: own.from,
        to: turned,
    };
    out.segment(subject, Segment::between(from, onto, turn, step.curve));
    if onto != to {
        let go = Interval {
            from: turned,
            to: own.to,
        };
        out.segment(subject, Segment::between(onto, to, go, step.curve));
    }
}
