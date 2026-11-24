//! A read-only display of the native host's FPS and pacing.

use std::convert::Infallible;

use game_hud::hud::FloatFeature;
use kit_base::tokens::{color, size, space, type_style};
use kit_scene::{Context, Feature};
use kit_ui::element::{Align, Element, FloatId, Justify, column, floats, holder, row, text};

/// The read-only feature that keeps the latest sample supplied by the host.
pub struct FpsMonitor;

/// The frame rates shown by the monitor, rounded to whole frames per second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameRates {
    /// Throughput over the most recent sample.
    pub current: Option<u16>,
    /// The lowest rate implied by the gap between two submitted frames in the last minute.
    pub low: Option<u16>,
    /// The rate Beryl currently requests.
    pub target: Option<u16>,
    /// The cadence reported by the platform's display link, when available.
    pub cadence: Option<u16>,
}

impl Feature for FpsMonitor {
    const SOURCE: &'static str = file!();

    type State = FrameRates;
    type Message = FrameRates;
    type Inputs<'a> = bool;
    type ViewKey = bool;
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        state: &mut FrameRates,
        message: &FrameRates,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        std::mem::replace(state, *message) != *message
    }

    fn view_key(_: &FrameRates, compact: &bool) -> bool {
        *compact
    }

    fn view(state: &FrameRates, compact: bool) -> Element {
        monitor(*state, compact)
    }
}

impl FloatFeature for FpsMonitor {
    fn floats() -> impl Iterator<Item = FloatId> {
        std::iter::once(floats::FRAME_RATES)
    }
}

fn monitor(rates: FrameRates, compact: bool) -> Element {
    let accent = pace_color(rates);
    let current = row()
        .align_items(Align::Center)
        .gap(space::S)
        .child(
            holder()
                .width(size::FPS_MONITOR_INDICATOR)
                .height(size::FPS_MONITOR_INDICATOR)
                .radius(size::FPS_MONITOR_INDICATOR * 0.5)
                .background(accent),
        )
        .child(
            holder()
                .min_width(size::FPS_MONITOR_DIGITS)
                .justify_content(Justify::End)
                .child(
                    text(&number(rates.current))
                        .style(type_style::FPS_MONITOR_NUMBER)
                        .color(accent),
                ),
        )
        .child(label("FPS", color::TEXT));
    let low = row()
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .gap(space::S)
        .child(label("1 min low", color::TEXT_DIM))
        .child(label(&number(rates.low), color::TEXT));
    let target = label(&format!("{} target", number(rates.target)), color::TEXT_DIM);
    let cadence = rates
        .cadence
        .map(|rate| label(&format!("{rate} Hz link"), color::TEXT_DIM));
    let panel = column()
        .name("FPS monitor")
        .background(color::PANEL)
        .border((color::ICON_BUTTON_RIM, 1.0))
        .radius(size::PANEL_RADIUS)
        .passes(true);
    if compact {
        let mut panel = panel;
        panel.style.padding = [space::S, space::M, space::S, space::M];
        return panel.child(
            row()
                .align_items(Align::Center)
                .gap(space::M)
                .child(current)
                .child(low)
                .child(target)
                .children(cadence),
        );
    }
    panel
        .width(size::FPS_MONITOR_WIDTH)
        .gap(space::S)
        .padding(space::M)
        .child(
            row()
                .align_items(Align::Center)
                .justify_content(Justify::SpaceBetween)
                .child(current)
                .child(
                    column()
                        .align_items(Align::End)
                        .child(target)
                        .children(cadence),
                ),
        )
        .child(holder().height(1.0).background(color::ICON_BUTTON_RIM))
        .child(low)
}

fn label(words: &str, tint: [u8; 4]) -> Element {
    text(words).style(type_style::FPS_MONITOR_LABEL).color(tint)
}

fn number(rate: Option<u16>) -> String {
    rate.map_or_else(|| "—".to_owned(), |rate| rate.to_string())
}

fn pace_color(rates: FrameRates) -> [u8; 4] {
    let Some((current, target)) = rates.current.zip(rates.target) else {
        return color::TEXT_DIM;
    };
    let expected = u32::from(rates.cadence.map_or(target, |cadence| cadence.min(target)));
    let hundredths = u32::from(current) * 100;
    if hundredths >= expected * 95 {
        color::STAT_UP
    } else if hundredths >= expected * 80 {
        color::GOLD
    } else {
        color::STAT_DOWN
    }
}
