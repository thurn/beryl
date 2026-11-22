//! The phase icon and full control mark along the table's middle band.

use beryl_protocol::protocol::{PlayerView, Side, Step};
use glam::Vec2;

use game_table::profiles::Profile;
use kit_base::geometry::Rect;
use kit_base::tokens::{color, shadow};
use kit_frame::frame::Role;
use kit_ui::element::{Action, Anchor, Element, Shown};
use kit_ui::widgets::{icon_button, icons};

/// The source of the phase indicator and menu button, for the client map.
pub const SOURCE: &str = file!();

/// The menu's button, at the right end of the opponent's strip: it opens the settings, which
/// hold the log and the bug report (`MenuButton` in `ui/src/mobile/chrome.tsx`).
#[must_use]
pub fn menu(profile: Profile, press: Action) -> Option<Element> {
    let slot = profile.slot("menu")?.rect;
    let mut button = icon_button(icons::MENU, "Menu");
    button.press = Some(press);
    button.anchor = Some(Anchor::Point(
        Vec2::new(slot.x + slot.width, slot.y + slot.height / 2.0),
        Vec2::new(1.0, 0.5),
    ));
    Some(button)
}

/// The middle band's furniture, with the phase icon on
/// the band's line, lit while it is the viewer's turn, centred, or at the band's left while the
/// stack holds anything; and full control's mark at the band's right end while it is on. Neither
/// takes a press: the phase is shown, and full control is set in the settings.
#[must_use]
pub fn middle(
    view: &PlayerView,
    full_control: Option<Element>,
    profile: Profile,
) -> Option<Element> {
    let slot = profile.slot("phase")?.rect;
    let bounds = phase_box(view.stack.is_empty(), slot);
    let yours = view.turn.active == Side::You;
    let side = f32::from(icons::PHASE_SIDE);
    let line = slot.y + slot.height / 2.0;
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
    phase.anchor = Some(Anchor::Point(
        Vec2::new(bounds.x, line),
        Vec2::new(0.0, 0.5),
    ));
    Some(
        kit_ui::element::holder()
            .name("middle")
            .child(phase)
            .children(full_control),
    )
}

/// The left and right edges of the phase icon in the middle band: centred while the stack is
/// empty, and at the band's left while it holds anything.
#[must_use]
pub fn phase_box(stack_empty: bool, slot: Rect) -> Rect {
    let side = f32::from(icons::PHASE_SIDE);
    let left = if stack_empty {
        slot.x + (slot.width - side) / 2.0
    } else {
        slot.x
    };
    Rect {
        x: left,
        y: slot.y + (slot.height - side) / 2.0,
        width: side,
        height: side,
    }
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
