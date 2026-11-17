//! The settings sheet, shown over any screen: the sound panel (a mute toggle and a slider each
//! for the music and the effects), full control, concede (in a game only) and the credits for
//! the music and sounds. It is the web interface's gear panel (`Settings` in
//! `ui/src/game/Corner.tsx`, `SoundPanel` and `Credits` in `ui/src/audio/SoundControls.tsx`,
//! `ui/src/screens/styles/settings.css`).

use super::{Screen, ScreenIn, Sheet};
use crate::data::space::flat::phone::GUTTER;
use crate::data::space::hud::phone::BUTTON_MIN_WIDTH as PHONE_BUTTON;
use crate::element::{Align, Element, Justify, Overflow, Position, Press, column, row, text};
use crate::settings::{Graphics, Setting};
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{button, icon_button, icons, primary, slider, toggle};

include!(concat!(env!("OUT_DIR"), "/credits.rs"));

/// What the settings sheet's concede button does: asks to confirm on its first press, as the
/// web's `ConcedeButton` does, then concedes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Concede {
    /// "Concede": a press only asks to confirm.
    #[default]
    Asks,
    /// "Confirm Concede": a press concedes the game.
    Confirms,
}

/// How wide the panel is, in logical points.
const WIDTH: f32 = size::SETTINGS_WIDTH;

/// The gap between the window's edge and the panel at the least.
const MARGIN: f32 = size::SCREEN_MARGIN;

/// How far the gear stands from the window's top and right edges, in logical points.
pub const GEAR_INSET: f32 = size::SETTINGS_GEAR_INSET;

/// The column the client shows at the game screen's top right (UI Conventions, "Chrome"): the
/// gear that opens the sheet, the bug button under it when `bug`, and undo under that when
/// `undo`, with no gap kept for a button left out. This is the web's `Corner` and `UndoButton`
/// (`ui/src/game/Corner.tsx`). The web hides its bug button while the report is open.
#[must_use]
pub fn corner(bug: bool, undo: bool) -> Element {
    column()
        .name("corner")
        .gap(size::CORNER_GAP)
        .child(icon_button(icons::GEAR, "Settings").on_press(Press::Sheet(Some(Sheet::Settings))))
        .children(bug.then(|| {
            icon_button(icons::BUG, "Report bug").on_press(Press::Sheet(Some(Sheet::BugReport)))
        }))
        .children(undo.then(|| icon_button(icons::UNDO, "Undo").on_press(Press::Undo)))
}

/// What the flat layer shows for the sheet: the panel in the middle of the window.
#[must_use]
pub fn settings(input: &ScreenIn) -> Element {
    let settings = input.settings;
    let muted = settings.muted;
    let mute = toggle(
        if muted {
            icons::SPEAKER_OFF
        } else {
            icons::SPEAKER
        },
        if muted { "Unmute" } else { "Mute" },
        muted,
    )
    .on_press(Press::Setting(Setting::Muted(!muted)));
    let levels = column()
        .gap(space::S)
        .child(level("Music", Setting::Music(settings.music)))
        .child(level("Effects", Setting::Effects(settings.effects)));
    let full_control = toggle(icons::FULL_CONTROL, "Full control", settings.full_control)
        .on_press(Press::Setting(Setting::FullControl(!settings.full_control)));
    let window = input.window;
    let safe = window.safe_area;
    // On a phone the panel keeps inside the safe area, the gutter in from its sides, and is no
    // wider than the window leaves.
    let (width, margin) = if super::phone(input) {
        let across = window.size.width - safe.left - safe.right - 2.0 * GUTTER;
        (WIDTH.min(across), GUTTER)
    } else {
        (WIDTH, MARGIN)
    };
    let tall = window.size.height - safe.top - safe.bottom - 2.0 * margin;
    let mut panel = column()
        .name("settings panel")
        .width(width)
        .max_height(tall.max(0.0))
        .overflow(Overflow::Scroll)
        .padding(space::L)
        .gap(space::L)
        .background(color::PANEL)
        .radius(size::PANEL_RADIUS)
        .child(
            row()
                .align_items(Align::Center)
                .justify_content(Justify::SpaceBetween)
                .child(text("Settings").style(type_style::TITLE))
                .child(icon_button(icons::CLOSE, "close").on_press(Press::Sheet(None))),
        )
        .child(
            row()
                .gap(space::L)
                .align_items(Align::Center)
                .child(mute)
                .child(levels),
        )
        .child(graphics(settings.graphics))
        .child(
            row()
                .gap(space::L)
                .align_items(Align::Center)
                .child(full_control)
                .child(
                    column()
                        .child(text("Full control"))
                        .child(text("Stop at every priority")),
                ),
        );
    // The phone has no room for the desktop's log column or bug button, so its settings, which
    // its menu button opens, hold both, as the web's phone `Menu` does.
    if input.window.phone() && input.screen == Screen::Game {
        panel = panel.child(
            row()
                .gap(space::M)
                .child(button("Log").min_width(PHONE_BUTTON).on_press(Press::Log))
                .child(
                    button("Report bug")
                        .min_width(PHONE_BUTTON)
                        .on_press(Press::Sheet(Some(Sheet::BugReport))),
                ),
        );
    }
    if in_game(input) {
        // Asked once in place, as the web's `ConcedeButton`: the first press turns the button
        // into the confirmation, which closing the sheet puts back.
        let concede = if input.concede == Concede::Confirms {
            primary("Confirm Concede")
        } else {
            button("Concede")
        };
        panel = panel.child(concede.on_press(Press::Concede));
    }
    panel = panel.child(credits());
    let mut sheet = column()
        .name("settings")
        .position(Position::Absolute)
        .inset([Some(0.0); 4])
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(panel);
    sheet.style.padding = [safe.top, safe.right, safe.bottom, safe.left];
    sheet
}

/// Whether there is a game to concede: the game screen, until the game is over.
fn in_game(input: &ScreenIn) -> bool {
    use beryl_protocol::protocol::Status;
    input.screen == Screen::Game
        && !input
            .view
            .is_some_and(|view| matches!(view.status, Status::Over { .. }))
}

/// The graphics row: a label and one button for each way the visual tier is chosen, the saved
/// one shown as the primary button and marked selected. Neither the web interface nor Arena has
/// this setting, so it is built from the sheet's own buttons.
fn graphics(chosen: Graphics) -> Element {
    let choice = |label: &str, graphics: Graphics| {
        let element = if graphics == chosen {
            primary(label)
        } else {
            button(label)
        };
        element
            .min_width(0.0)
            .selected(graphics == chosen)
            .on_press(Press::Setting(Setting::Graphics(graphics)))
    };
    row()
        .name("graphics")
        .gap(space::M)
        .align_items(Align::Center)
        .child(text("Graphics"))
        .child(choice("Automatic", Graphics::Automatic))
        .child(choice("Baseline", Graphics::Baseline))
        .child(choice("Enhanced", Graphics::Enhanced))
}

/// A slider for `setting` named `label`, with the label beside it.
fn level(label: &str, setting: Setting) -> Element {
    row()
        .gap(space::M)
        .align_items(Align::Center)
        .child(text(label))
        .child(slider(label, setting))
}

/// Who made the music and sounds, each author with their license and their packs.
fn credits() -> Element {
    column()
        .name("credits")
        .gap(space::S)
        .children(CREDITS.iter().map(|&(author, license, kinds, packs)| {
            column()
                .child(text(&format!("{kinds}: {author} {license}")))
                .children(packs.iter().map(|pack| text(pack)))
        }))
}
