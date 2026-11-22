//! The game vocabulary used by frame probes.

use game_table::table::labels::{ABILITY_ROLE, COPY_NOTE, FACE_NOTE, PHASED_OUT, PICKED, TAPPED};
use kit_frame::frame::Labels;

pub const GAME_LABELS: Labels = {
    let mut labels = Labels::DEFAULT
        .with_state(TAPPED, "tapped")
        .with_state(PHASED_OUT, "phased-out")
        .with_state(PICKED, "picked");
    labels.count = Some("pile");
    labels.annotations[COPY_NOTE] = Some("copy");
    labels.annotations[FACE_NOTE] = Some("note");
    labels.roles = &[ABILITY_ROLE];
    labels
};
