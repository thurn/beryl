//! The icons of the flat layer, by id. Every part of the client names an icon by one of these
//! constants, so that the widgets track can draw them from one atlas.

use std::collections::BTreeMap;

use crate::input::IconCell;
use kit_frame::frame::Sprite;

pub use kit_base::atlas::IconId;

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
/// The bug report's button that reports the whole window: four corners of a frame.
pub const WHOLE: IconId = IconId(40);
/// The phone's menu button, which opens the settings: three bars.
pub const MENU: IconId = IconId(41);
/// The lock a permanent that doesn't untap wears in its corner (`table::badges`).
pub const LOCK: IconId = IconId(42);
/// The atlas size of the lock, in logical points: large enough that a table card drawn
/// big on a Retina display still samples it at about one texel a pixel.
pub const LOCK_SIDE: u16 = 24;

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

/// The atlas size of a phase tracker icon, in logical points.
pub const PHASE_SIDE: u16 = 32;

/// The atlas size of an icon button's glyph and of a step icon, in logical points
/// (`.icon-button > svg`, icon-button.css).
pub const BUTTON_SIDE: u16 = 22;

/// The atlas size of full control's mark, in logical points (`.full-control svg`,
/// controls.css).
pub const FULL_CONTROL_SIDE: u16 = 28;

/// The size in logical points that the atlas holds for a widget icon. It matches the
/// icon's artwork in `crate::art::icons::ALL`.
#[must_use]
pub const fn atlas_size(icon: IconId) -> u16 {
    if icon.0 == FULL_CONTROL.0 {
        FULL_CONTROL_SIDE
    } else if icon.0 == LOCK.0 {
        LOCK_SIDE
    } else if icon.0 >= PHASES[0].0 && icon.0 <= PHASES[4].0 {
        PHASE_SIDE
    } else {
        BUTTON_SIDE
    }
}

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
    /// By icon, size and shadow (`IconCell::shadow`), 0 for the icon itself.
    table: BTreeMap<(IconId, u16, u8), Sprite>,
}

impl Icons {
    /// Records the icons of an atlas page that is ready.
    pub fn add(&mut self, page: u16, cells: &[IconCell]) {
        for cell in cells {
            let sprite = Sprite {
                page,
                rect: cell.rect,
            };
            self.table
                .insert((cell.icon, cell.size, cell.shadow), sprite);
        }
    }

    /// Where `icon` at `size` logical points is, if a page holds it.
    #[must_use]
    pub fn get(&self, icon: IconId, size: u16) -> Option<Sprite> {
        self.shadow(icon, size, 0)
    }

    /// Where the cell `shadow` (`IconCell::shadow`) of one of the drop shadows of `icon` at
    /// `size` logical points is, if a page holds it.
    #[must_use]
    pub fn shadow(&self, icon: IconId, size: u16, shadow: u8) -> Option<Sprite> {
        self.table.get(&(icon, size, shadow)).copied()
    }
}

/// One layer of an icon assembled from several atlas cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconLayer {
    pub icon: IconId,
    /// The cell's rectangle in artwork units.
    pub view: [f32; 4],
    pub color: [u8; 4],
    /// Draw this layer only when the icon is highlighted.
    pub highlight: bool,
}

/// An icon whose atlas cells are drawn in order, each at its own box in the artwork.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayeredIcon {
    pub layers: &'static [IconLayer],
    /// The artwork's top left corner, relative to the origin of its layers.
    pub origin: [f32; 2],
    /// Logical points per artwork unit.
    pub scale: f32,
    pub highlighted: bool,
}

/// The width in logical points of a cell `units` wide drawn at `scale` points to a unit: a
/// whole number of points, the size an icon is keyed by.
#[must_use]
pub fn cell_width(units: f32, scale: f32) -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a cell is a few hundred points wide"
    )]
    let width = (units * scale).round().max(1.0) as u16;
    width
}
