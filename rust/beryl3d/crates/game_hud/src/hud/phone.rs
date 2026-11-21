//! The game screen's furniture on the phone layout (docs/ui-conventions.md, "The phone"), which
//! the web interface's `PhoneStageBoard` (`ui/src/stage/PhoneStageBoard.tsx`) and
//! `PhoneChrome.tsx` put in the table's bands (`table::phone_bands`):
//!
//! - The menu button at the right end of the opponent's strip, which opens the settings. The
//!   settings hold the log and the bug report on the phone, which has no room for the desktop's
//!   log column or its gear.
//! - The middle band: the phase icon on its line, at its left while the stack holds anything,
//!   and full control's mark at its right end, across from the phase.
//! - The log, as a sheet over the table while it is open.
use beryl_protocol::protocol::{PlayerView, Side, Step};
use glam::Vec2;

use game_table::profiles::TableBands;
use kit_base::tokens::{color, shadow};
use kit_frame::frame::Role;
use kit_ui::element::{Action, Anchor, Element, Shown};
use kit_ui::widgets::{icon_button, icons};

/// The menu's button, at the right end of the opponent's strip: it opens the settings, which
/// hold the log and the bug report (`MenuButton` in `ui/src/mobile/chrome.tsx`).
#[must_use]
pub fn menu(bands: &TableBands, press: Action) -> Element {
    let mut button = icon_button(icons::MENU, "Menu");
    button.press = Some(press);
    button.anchor = Some(Anchor::Point(
        Vec2::new(bands.right, bands.opponent_strip.middle()),
        Vec2::new(1.0, 0.5),
    ));
    button
}

/// The middle band's furniture, as the web's `Middle` and `MiddleEnd` draw it: the phase icon on
/// the band's line, lit while it is the viewer's turn, centred, or at the band's left while the
/// stack holds anything; and full control's mark at the band's right end while it is on. Neither
/// takes a press: the phase is shown, and full control is set in the settings.
#[must_use]
pub fn middle(view: &PlayerView, full_control: bool, bands: &TableBands) -> Element {
    let yours = view.turn.active == Side::You;
    let side = f32::from(icons::PHASE_SIDE);
    let line = bands.middle.middle();
    let mut phase = Element::new(Role::Region)
        .name("phase")
        .width(side)
        .height(side)
        .radius(side / 2.0)
        .background(color::ICON_BUTTON)
        .color(if yours { color::GOLD } else { color::TEXT_DIM })
        // `.phase-current .phase-icon`'s glow while lit, else `.phase-icon`'s shadow
        // (phase-tracker.css), as the web's `PhaseIcon` lights it.
        .drop_shadow(if yours {
            shadow::CURRENT_GLOW.to_vec()
        } else {
            shadow::PHASE_ICON.to_vec()
        })
        .passes(true);
    phase.shows = Some(Shown::Icon(phase_icon(view.turn.step), icons::PHASE_SIDE));
    let (left, _) = phase_edges(view.stack.is_empty(), bands);
    phase.anchor = Some(Anchor::Point(Vec2::new(left, line), Vec2::new(0.0, 0.5)));
    let mark = full_control.then(|| {
        let mut mark = Element::new(Role::Region)
            .name("full control")
            .width(side)
            .height(side)
            .color(color::GOLD)
            .drop_shadow(shadow::FULL_CONTROL.to_vec())
            .passes(true);
        mark.shows = Some(Shown::Icon(
            icons::FULL_CONTROL,
            icons::atlas_size(icons::FULL_CONTROL),
        ));
        mark.anchor = Some(Anchor::Point(
            Vec2::new(bands.right, line),
            Vec2::new(1.0, 0.5),
        ));
        mark
    });
    kit_ui::element::holder()
        .name("middle")
        .child(phase)
        .children(mark)
}

/// The left and right edges of the phase icon in the middle band: centred while the stack is
/// empty, and at the band's left while it holds anything.
pub fn phase_edges(stack_empty: bool, bands: &TableBands) -> (f32, f32) {
    let side = f32::from(icons::PHASE_SIDE);
    let left = if stack_empty {
        f32::midpoint(bands.left, bands.right) - side / 2.0
    } else {
        bands.left
    };
    (left, left + side)
}

/// The icon of the phase `step` is in, of the web's five (`phaseOf` in `ui/src/mobile/layout.ts`):
/// beginning, first main, combat, second main and end.
fn phase_icon(step: Step) -> kit_ui::widgets::IconId {
    let phase = match step {
        Step::Untap | Step::Upkeep | Step::Draw => 0,
        Step::PrecombatMain => 1,
        Step::BeginningOfCombat
        | Step::DeclareAttackers
        | Step::DeclareBlockers
        | Step::FirstStrikeDamage
        | Step::CombatDamage
        | Step::EndOfCombat => 2,
        Step::PostcombatMain => 3,
        Step::End | Step::Cleanup => 4,
    };
    icons::PHASES[phase]
}
