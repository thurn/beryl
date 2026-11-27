//! The phase icon and full control mark along the table's middle band.

use beryl_protocol::protocol::{PlayerView, Side, Step};
use glam::Vec2;

use game_table::profiles::Profile;
use kit_base::geometry::Rect;
use kit_base::tokens::{color, shadow, space};
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

/// The middle band's furniture: the phase icon at one end of the band (`phase_box`), and full
/// control's mark at the other end while it is on, across from the phase. Neither takes a
/// press: the phase is shown, and full control is set in the settings. While a prompt's bar
/// stands on the table, the bar draws the phase icon itself (`with_phase` unset), so that it can
/// leave it out where its pill needs the icon's room.
#[must_use]
pub fn middle(
    view: &PlayerView,
    full_control: Option<Element>,
    profile: Profile,
    with_phase: bool,
) -> Option<Element> {
    let slot = middle_band(profile, view)?;
    let phase = with_phase.then(|| icon(view, phase_box(slot, view.shared.is_some())));
    Some(
        kit_ui::element::holder()
            .name("middle")
            .children(phase)
            .children(full_control),
    )
}

/// The phase icon in `bounds`, lit while it is `view`'s viewer's turn.
#[must_use]
pub fn icon(view: &PlayerView, bounds: Rect) -> Element {
    let yours = view.turn.active == Side::You;
    let side = f32::from(icons::PHASE_SIDE);
    let mut phase = Element::new(Role::Icon)
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
        Vec2::new(bounds.x, bounds.y + bounds.height / 2.0),
        Vec2::new(0.0, 0.5),
    ));
    phase
}

/// The part of the middle band (the `phase` slot) where its furniture and a prompt's controls
/// may stand in the game `view`: the whole band, or, when both players share one library and
/// graveyard, the part between those two piles, which lie at the band's ends, less a gap on
/// each side.
#[must_use]
pub fn middle_band(profile: Profile, view: &PlayerView) -> Option<Rect> {
    let slot = profile.slot("phase")?.rect;
    Some(clear_of_shared_piles(profile, slot, view.shared.is_some()))
}

/// `area`, a box across the middle band, narrowed to stand between the shared library and
/// graveyard with a gap on each side when the game is `shared`.
#[must_use]
pub fn clear_of_shared_piles(profile: Profile, area: Rect, shared: bool) -> Rect {
    let Some((library, graveyard)) = profile.shared_pile_edges().filter(|_| shared) else {
        return area;
    };
    let left = area.x.max(library + space::M);
    let right = (area.x + area.width).min(graveyard - space::M);
    Rect {
        x: left,
        width: (right - left).max(0.0),
        ..area
    }
}

/// The box of the phase icon in the middle band `slot`, on the band's line. It stands at an end
/// of the band, so that it never covers the stack, which is centred on the line and leaves room
/// at both ends (`banded::Stack::ends`), or the pill that says what a prompt asks, which the
/// prompt bar fits beside the icon. It stands at the left end, because the prompt bar puts its
/// buttons at the right end, where they stand over the dock. On a table whose players share
/// their piles (`shared`), it stands at the right end instead: the cards a look takes from the
/// library lie beside the library at the band's left end, and nothing lies beside the
/// graveyard at its right end.
#[must_use]
pub fn phase_box(slot: Rect, shared: bool) -> Rect {
    let side = f32::from(icons::PHASE_SIDE);
    Rect {
        x: if shared {
            slot.x + slot.width - side
        } else {
            slot.x
        },
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
