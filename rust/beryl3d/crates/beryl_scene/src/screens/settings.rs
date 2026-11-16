//! The settings sheet, shown over any screen: the sound panel (a mute toggle and a slider each
//! for the music and the effects), full control, concede (in a game only) and the credits for
//! the music and sounds. It is the web interface's gear panel (`Settings` in
//! `ui/src/game/Corner.tsx`, `SoundPanel` and `Credits` in `ui/src/audio/SoundControls.tsx`,
//! `ui/src/screens/styles/settings.css`).

use super::{Screen, ScreenIn, Sheet};
use crate::element::{Align, Element, Justify, Overflow, Position, Press, column, row, text};
use crate::settings::Setting;
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
const WIDTH: f32 = 360.0;

/// The gap between the window's edge and the panel at the least.
const MARGIN: f32 = 24.0;

/// How far the gear stands from the window's top and right edges, in logical points.
pub const GEAR_INSET: f32 = 12.0;

/// The gear that opens the sheet, which the client shows at the game screen's top right.
#[must_use]
pub fn gear() -> Element {
    icon_button(icons::GEAR, "Settings").on_press(Press::Sheet(Some(Sheet::Settings)))
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
    let mut panel = column()
        .name("settings panel")
        .width(WIDTH)
        .max_height((input.window.size.height - 2.0 * MARGIN).max(0.0))
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
    column()
        .name("settings")
        .position(Position::Absolute)
        .inset([Some(0.0); 4])
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(panel)
}

/// Whether there is a game to concede: the game screen, until the game is over.
fn in_game(input: &ScreenIn) -> bool {
    use beryl_protocol::protocol::Status;
    input.screen == Screen::Game
        && !input
            .view
            .is_some_and(|view| matches!(view.status, Status::Over { .. }))
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
