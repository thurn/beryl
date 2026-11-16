//! A switch that is on or off, such as the mute button beside the sound levels or full
//! control, drawn as the web interface's mute toggle (`SoundPanel` in
//! `ui/src/audio/SoundControls.tsx`): an icon button whose glyph turns `--ember` while it is
//! on (`.sound-mute[aria-pressed="true"]` in `ui/src/screens/styles/settings.css`).

use super::IconId;
use super::icon_button::round;
use crate::element::Element;
use crate::frame::Role;
use crate::tokens::color;

/// A switch named `name` showing `icon`, `on` or off. A press on it sends the press the caller
/// gives it with `on_press`, which should turn it the other way. While on, its resolved element
/// is `SELECTED`.
#[must_use]
pub fn toggle(icon: IconId, name: &str, on: bool) -> Element {
    let element = round(Role::Toggle, icon, name).selected(on);
    if on {
        element.color(color::EMBER)
    } else {
        element
    }
}
