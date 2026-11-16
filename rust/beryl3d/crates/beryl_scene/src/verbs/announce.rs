//! `announce`: a banner or a pill is shown over the step, and nothing moves. The step writes a
//! `NoticeRun`; the show keeps it in real time from when its script starts, and the client shows
//! it as a float of the flat layer (`hud::notices`). A notice whose step `passes` goes on being
//! shown after its beat, while the table plays on under it.

use beryl_protocol::script::Act;

use super::{StepIn, StepOut};
use crate::notice::NoticeRun;

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let Act::Announce { notice } = step.act else {
        return;
    };
    out.notice(NoticeRun {
        notice: notice.clone(),
        over: step.own,
        passes: step.timing.passes,
        step: step.number,
    });
}
