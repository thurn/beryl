//! The icons of the flat layer, by id. Every part of the client names an icon by one of these
//! constants, so that the widgets track can draw them from one atlas.

/// An icon of the flat layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IconId(pub u16);

pub const GEAR: IconId = IconId(0);
pub const BUG: IconId = IconId(1);
pub const UNDO: IconId = IconId(2);
pub const LOG: IconId = IconId(3);
pub const CLOSE: IconId = IconId(4);
pub const SPEAKER: IconId = IconId(5);
pub const SPEAKER_OFF: IconId = IconId(6);
pub const FULL_CONTROL: IconId = IconId(7);
pub const PASS_ON: IconId = IconId(8);
pub const PLUS: IconId = IconId(9);
pub const MINUS: IconId = IconId(10);

/// The icon of each step stop, in the order of the web interface's `STEP_STOPS`
/// (`ui/src/components/Steps.tsx`): untap, upkeep, draw, first main, beginning of combat,
/// attackers, blockers, damage, end of combat, second main, end step and cleanup.
pub const STEPS: [IconId; 12] = [
    IconId(11),
    IconId(12),
    IconId(13),
    IconId(14),
    IconId(15),
    IconId(16),
    IconId(17),
    IconId(18),
    IconId(19),
    IconId(20),
    IconId(21),
    IconId(22),
];

/// The icon of each phase, in the order of the web interface's `PhaseTracker`
/// (`ui/src/components/Controls.tsx`): beginning, first main, combat, second main and end.
pub const PHASES: [IconId; 5] = [IconId(23), IconId(24), IconId(25), IconId(26), IconId(27)];
