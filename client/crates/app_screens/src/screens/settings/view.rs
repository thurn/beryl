//! The settings sheet, shown over any screen: the sound panel (a mute toggle and a slider each
//! for the music and the effects), full control, concede (in a game only) and the credits for
//! the music and sounds. It is the web interface's gear panel (`Settings` in
//! `ui/src/game/Corner.tsx`, `SoundPanel` and `Credits` in `ui/src/audio/SoundControls.tsx`,
//! `ui/src/screens/styles/settings.css`).

use super::{Inputs, Message, State};
use crate::screens::Screen;
use crate::settings::{Graphics, ScreenSettings, Setting};
use kit_base::tokens::{color, size, space, type_style};
use kit_scene::SizeClass;
use kit_ui::element::{Action, Align, Element, Justify, Overflow, Position, column, row, text};
use kit_ui::widgets::{button, icon_button, icons, primary, slider, toggle};

include!(concat!(env!("OUT_DIR"), "/credits.rs"));

/// How far the gear stands from the window's top and right edges, in logical points.
pub const GEAR_INSET: f32 = size::SETTINGS_GEAR_INSET;

/// The column the client shows at the game screen's top right (UI Conventions, "Chrome"): the
/// gear that opens the sheet, the bug button under it when `bug`, and undo under that when
/// `undo` holds the press it sends, with no gap kept for a button left out. This is the web's `Corner` and `UndoButton`
/// (`ui/src/game/Corner.tsx`). The web hides its bug button while the report is open.
#[must_use]
pub fn corner(bug: bool, undo: Option<Action>) -> Element {
    // Clear, as `element::holder` is: a float's top element is otherwise drawn on a panel.
    column()
        .name("corner")
        .background([0; 4])
        .border(([0; 4], 0.0))
        .gap(size::CORNER_GAP)
        .child(icon_button(icons::GEAR, "Settings").on_press(Message::Open))
        .children(bug.then(|| {
            icon_button(icons::BUG, "Report bug")
                .on_press(crate::screens::bug_report::Message::Open)
        }))
        .children(undo.map(|press| {
            let mut button = icon_button(icons::UNDO, "Undo");
            button.press = Some(press);
            button
        }))
}

/// What the flat layer shows for the sheet: the panel in the middle of the window.
#[must_use]
pub(super) fn settings(state: &State, input: Inputs) -> Element {
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
    .on_press(Message::Change(Setting::Muted(!muted)));
    let levels = column()
        .gap(space::S)
        .child(level("Music", Setting::Music(settings.music)))
        .child(level("Effects", Setting::Effects(settings.effects)));
    let full_control = toggle(icons::FULL_CONTROL, "Full control", settings.full_control).on_press(
        Message::Change(Setting::FullControl(!settings.full_control)),
    );
    let size = input.window.size;
    let layout = input.layout;
    let bounds = layout.slot.rect;
    let mut panel = column()
        .name("settings panel")
        .width(bounds.width)
        .max_height(bounds.height)
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
                .child(icon_button(icons::CLOSE, "close").on_press(Message::Close)),
        )
        .child(
            row()
                .gap(space::L)
                .align_items(Align::Center)
                .child(mute)
                .child(levels),
        )
        .child(graphics(settings, layout.size_class))
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
    // In a compact window the menu opens the settings sheet, which also holds the log and
    // bug actions.
    if layout.size_class == SizeClass::Compact && input.screen == Screen::Game {
        panel = panel.child(
            row()
                .gap(space::M)
                .child(
                    button("Log")
                        .min_width(layout.quick_action_min_width)
                        .on_press(Message::ShowLog),
                )
                .child(
                    button("Report bug")
                        .min_width(layout.quick_action_min_width)
                        .on_press(Message::ReportBug),
                ),
        );
    }
    if input.screen == Screen::Game && !input.game_over {
        // Asked once in place, as the web's `ConcedeButton`: the first press turns the button
        // into the confirmation, which closing the sheet puts back.
        let concede = if state.confirm_concede {
            primary("Confirm Concede")
        } else {
            button("Concede")
        };
        panel = panel.child(concede.on_press(Message::Concede));
    }
    panel = panel.child(credits());
    let mut sheet = column()
        .name("settings")
        .holds_focus()
        .position(Position::Absolute)
        .inset([Some(0.0); 4])
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(panel);
    sheet.style.padding = [
        bounds.y,
        (size.width - bounds.x - bounds.width).max(0.0),
        (size.height - bounds.y - bounds.height).max(0.0),
        bounds.x,
    ];
    sheet
}

/// The graphics row: a label and one button for each way the visual tier is chosen, the saved
/// one shown as the primary button and marked selected. Neither the web interface nor Arena has
/// this setting, so it is built from the sheet's own buttons. In a compact window the buttons
/// stand one to a line to keep their text readable.
fn graphics(settings: ScreenSettings, size_class: SizeClass) -> Element {
    let chosen = settings.graphics;
    let choice = |label: &str, graphics: Graphics| {
        let element = if graphics == chosen {
            primary(label)
        } else {
            button(label)
        };
        element
            .min_width(0.0)
            .selected(graphics == chosen)
            .on_press(Message::Change(Setting::Graphics(graphics)))
    };
    let choices = [
        choice("Automatic", Graphics::Automatic),
        choice("Baseline", Graphics::Baseline),
        choice("Enhanced", Graphics::Enhanced),
    ];
    let tier = if size_class == SizeClass::Compact {
        column().gap(space::S).align_items(Align::Stretch)
    } else {
        row().gap(space::M).align_items(Align::Center)
    };
    column()
        .gap(space::S)
        .child(
            tier.name("graphics")
                .child(text("Graphics"))
                .children(choices),
        )
        .child(fps_monitor(settings.fps_monitor))
}

/// Keeps the control's position and width steady when its text changes.
fn fps_monitor(enabled: bool) -> Element {
    let mut control = if enabled {
        primary("On")
    } else {
        button("Off")
    };
    control.role = kit_frame::frame::Role::Toggle;
    row()
        .gap(space::M)
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .child(text("FPS monitor"))
        .child(
            control
                .name("FPS monitor")
                .min_width(size::FPS_MONITOR_TOGGLE)
                .radius(size::BUTTON_HEIGHT / 2.0)
                .selected(enabled)
                .on_press(Message::Change(Setting::FpsMonitor(!enabled))),
        )
}

/// A slider for `setting` named `label`, with the label beside it.
fn level(label: &str, setting: Setting) -> Element {
    row()
        .gap(space::M)
        .align_items(Align::Center)
        .child(text(label))
        .child(slider(
            label,
            setting.level().unwrap_or(0),
            Message::Change(setting),
        ))
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
