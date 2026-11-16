//! The icons of the flat layer, by id. Every part of the client names an icon by one of these
//! constants, so that the widgets track can draw them from one atlas.

use std::collections::BTreeMap;

use crate::frame::Sprite;
use crate::input::IconCell;

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

/// The icon of each step stop, filled, in the order of the web interface's `STEP_STOPS`
/// (`ui/src/components/Steps.tsx`): untap, upkeep, draw, first main, beginning of combat,
/// attackers, blockers, damage, end of combat, second main, end step and cleanup. The web draws
/// a step's icon filled where the player stops in it.
pub const STEPS: [IconId; 12] = steps(0);

/// The same icons outlined, each with the id after its filled one, which the web draws where
/// the player does not stop.
pub const STEPS_OUTLINED: [IconId; 12] = steps(1);

/// The icon of each phase, in the order of the web interface's `PhaseTracker`
/// (`ui/src/components/Controls.tsx`): beginning, first main, combat, second main and end.
pub const PHASES: [IconId; 5] = [IconId(35), IconId(36), IconId(37), IconId(38), IconId(39)];

/// How many icons there are: every id is below this.
pub const COUNT: u16 = 40;

/// The step icons from id 11, filled and outlined in turn, offset by `outlined`.
const fn steps(outlined: u16) -> [IconId; 12] {
    let mut ids = [IconId(0); 12];
    let mut i = 0;
    while i < 12 {
        ids[i] = IconId(11 + 2 * i as u16 + outlined);
        i += 1;
    }
    ids
}

/// Where each rasterized icon is in the atlas, by icon and size in logical points
/// (docs/beryl-3d.md, "The frame": the `Icon` material). `Input::Assets` fills it. An icon is
/// drawn over a box of its size in points whatever the pixel size of its cell, so when the
/// display's scale changes, the cells of the old scale are drawn scaled until the new ones
/// arrive and replace them.
#[derive(Debug, Default)]
pub struct Icons {
    table: BTreeMap<(IconId, u16), Sprite>,
}

impl Icons {
    /// Records the icons of an atlas page that is ready.
    pub fn add(&mut self, page: u16, cells: &[IconCell]) {
        for cell in cells {
            let sprite = Sprite {
                page,
                rect: cell.rect,
            };
            self.table.insert((cell.icon, cell.size), sprite);
        }
    }

    /// Where `icon` at `size` logical points is, if a page holds it.
    #[must_use]
    pub fn get(&self, icon: IconId, size: u16) -> Option<Sprite> {
        self.table.get(&(icon, size)).copied()
    }
}
