//! A slider that sets a level from 0 to 100, such as the music's volume, drawn as the web
//! interface's range inputs on the sound panel (`.sound-level input` in
//! `ui/src/screens/styles/settings.css`): a thin track 150 points long filled in `--gold` up to
//! a round gold thumb. A drag along it, or a press on it, sets the level in steps of 5, and
//! while it has the focus the left and right keys move it by 5 (`Client::point` and
//! `Client::key`).

use crate::element::{Align, Element, Position, Press, row};
use crate::frame::Role;
use crate::settings::Setting;
use crate::tokens::{color, size};

/// How far a level moves for one step of a drag or one key: the web range input's `step` of
/// 0.05.
pub const STEP: u8 = 5;

/// A slider named `name` for `setting`, which holds the level it shows. A drag sends
/// `Press::Setting` with the same setting at the level dragged to.
#[must_use]
pub fn slider(name: &str, setting: Setting) -> Element {
    let value = f32::from(setting.level().unwrap_or(0).min(100)) / 100.0;
    let track = size::SLIDER_WIDTH;
    let thumb = size::SLIDER_THUMB;
    let mut element = Element::new(Role::Slider)
        .name(name)
        .width(track)
        .height(size::ICON_BUTTON)
        .align_items(Align::Center)
        .on_press(Press::Setting(setting))
        .child(
            row()
                .name("track")
                .grow(1.0)
                .height(size::SLIDER_TRACK)
                .background(color::SLIDER_TRACK)
                .radius(size::SLIDER_TRACK / 2.0)
                .child(
                    row()
                        .name("fill")
                        .width(track * value)
                        .background(color::GOLD)
                        .radius(size::SLIDER_TRACK / 2.0),
                ),
        )
        .child(
            row()
                .name("thumb")
                .position(Position::Absolute)
                .inset([
                    Some((size::ICON_BUTTON - thumb) / 2.0),
                    None,
                    None,
                    Some((track - thumb) * value),
                ])
                .width(thumb)
                .height(thumb)
                .background(color::GOLD)
                .radius(thumb / 2.0),
        );
    element.focus = true;
    element
}

/// The level from 0 to 100 a pointer at `x` sets on a slider whose box runs from `left` for
/// `width` points: where the thumb's centre would be, in steps of `STEP`.
#[must_use]
pub fn level_at(x: f32, left: f32, width: f32) -> u8 {
    let thumb = size::SLIDER_THUMB;
    let room = (width - thumb).max(1.0);
    let share = ((x - left - thumb / 2.0) / room).clamp(0.0, 1.0);
    let step = f32::from(STEP);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let level = ((share * 100.0 / step).round() * step) as u8;
    level
}
