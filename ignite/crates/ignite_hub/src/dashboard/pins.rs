//! Machine and provider pins on ticket cards and in ticket properties.

use ignite_core::pins::Pins;

use super::components::{self as c, Tone, esc};

pub(super) fn chips(pins: &Pins) -> String {
    pins.machine
        .iter()
        .chain(pins.provider.iter())
        .map(|name| c::chip(name, Tone::Muted, None))
        .collect()
}

pub(super) fn property(value: Option<&str>, unrestricted: &str, inherited: bool) -> String {
    value.map_or_else(
        || c::muted(unrestricted),
        |value| {
            let source = if inherited {
                " <span class=\"c-muted\">inherited</span>"
            } else {
                ""
            };
            format!("{}{source}", esc(value))
        },
    )
}
