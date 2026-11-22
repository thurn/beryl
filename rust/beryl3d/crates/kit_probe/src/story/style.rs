//! Story properties mapped to the flat layer's widget builders and paints.

use std::fmt;

use kit_base::paint::{Gradient, Shadow, Stop};
use kit_frame::frame::StateBits;
use kit_ui::element::Element;
use kit_ui::widgets::{self, FieldState, MenuGroup, MenuRow, Widget, icons};

use super::Node;
use super::parse::number;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Property {
    Fill(Color),
    Ink(Color),
    Radius(f32),
    Border(f32, Color),
    Linear(f32, Color, Color),
    Shadow([f32; 3], Color),
    Text(String),
    Level(u8),
    Widget(Widget),
}

type Color = [u8; 4];

impl Property {
    pub(super) fn parse(key: &str, value: &str) -> Option<Self> {
        let mut parts = value.split(',');
        let property = match key {
            "fill" => Self::Fill(color(value)?),
            "ink" => Self::Ink(color(value)?),
            "radius" => Self::Radius(number(value, 0.0)?),
            "border" => Self::Border(number(parts.next()?, 0.0)?, color(parts.next()?)?),
            "linear" => Self::Linear(
                number(parts.next()?, -360.0)?,
                color(parts.next()?)?,
                color(parts.next()?)?,
            ),
            "shadow" => Self::Shadow(
                [
                    number(parts.next()?, -16384.0)?,
                    number(parts.next()?, -16384.0)?,
                    number(parts.next()?, 0.0)?,
                ],
                color(parts.next()?)?,
            ),
            "level" => Self::Level(value.parse::<u8>().ok().filter(|level| *level <= 100)?),
            "widget" => Self::Widget(
                Widget::ALL
                    .iter()
                    .copied()
                    .find(|widget| widget.word() == value)?,
            ),
            _ => return None,
        };
        (!matches!(key, "border" | "linear" | "shadow") || parts.next().is_none())
            .then_some(property)
    }

    pub(super) fn apply(&self, element: &mut Element) {
        let style = &mut element.style;
        match *self {
            Self::Fill(color) => style.background = Some(color),
            Self::Ink(color) => style.color = Some(color),
            Self::Radius(radius) => style.radius = radius,
            Self::Border(width, color) => style.border = Some((color, width).into()),
            Self::Linear(angle, start, end) => {
                style.background_image =
                    vec![Gradient::linear(angle, &[Stop::new(start), Stop::new(end)])];
            }
            Self::Shadow([x, y, blur], color) => {
                style.box_shadow = vec![Shadow::new(x, y, blur, color)];
            }
            Self::Text(_) | Self::Widget(_) | Self::Level(_) => {}
        }
    }
}

fn color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    (hex.len() == 8).then_some(())?;
    Some(u32::from_str_radix(hex, 16).ok()?.to_be_bytes())
}

struct ColorText(Color);

impl fmt::Display for ColorText {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "#{:08x}", u32::from_be_bytes(self.0))
    }
}

impl fmt::Display for Property {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Fill(color) => write!(out, "fill={}", ColorText(color)),
            Self::Ink(color) => write!(out, "ink={}", ColorText(color)),
            Self::Radius(radius) => write!(out, "radius={radius}"),
            Self::Border(width, color) => write!(out, "border={width},{}", ColorText(color)),
            Self::Linear(angle, start, end) => {
                write!(
                    out,
                    "linear={angle},{},{}",
                    ColorText(start),
                    ColorText(end)
                )
            }
            Self::Shadow([x, y, blur], color) => {
                write!(out, "shadow={x},{y},{blur},{}", ColorText(color))
            }
            Self::Text(ref words) => write!(
                out,
                "text=\"{}\"",
                words.replace('\\', "\\\\").replace('"', "\\\"")
            ),
            Self::Level(level) => write!(out, "level={level}"),
            Self::Widget(widget) => write!(out, "widget={}", widget.word()),
        }
    }
}

pub(super) fn build_widget(
    widget: Widget,
    node: &Node,
    words: &str,
    bits: StateBits,
    actions: &kit_base::data::profiles::ScreenMetrics,
) -> Element {
    let name = node.name.as_deref().unwrap_or_default();
    let side = node.rect.width.min(node.rect.height);
    let selected = bits.has(StateBits::SELECTED);
    let button = || {
        if bits.has(StateBits::PRIMARY) {
            widgets::primary(words)
        } else {
            widgets::button(words)
        }
    };
    match widget {
        Widget::Button => button(),
        Widget::SmallButton => widgets::start_panel::small(button()),
        Widget::CompactAction => widgets::screen::compact_action(button(), actions),
        Widget::PassOn => widgets::pass_on(name, side),
        Widget::Check => widgets::check_mark(side),
        Widget::Checkbox => widgets::start_panel::checkbox(words, selected),
        Widget::CheckSquare => widgets::start_panel::check_square(side, selected),
        Widget::StartPanel => widgets::start_panel::panel_look(kit_ui::element::column()),
        Widget::Field => widgets::field(
            name,
            &FieldState {
                text: words.to_owned(),
                caret: u32::try_from(words.len()).unwrap_or(u32::MAX),
                placeholder: "Type here".to_owned(),
                ..FieldState::default()
            },
        ),
        Widget::IconButton => widgets::icon_button(icons::GEAR, name),
        Widget::List => widgets::list(name, []),
        Widget::Menu => {
            let mut row = MenuRow::new(words);
            row.checked = selected;
            row.enabled = !bits.has(StateBits::DISABLED);
            row.action = Some(kit_ui::element::Action::new(()));
            widgets::menu(name, "", &[MenuGroup { rows: vec![row] }])
        }

        Widget::Slider => {
            let level = node
                .properties
                .iter()
                .find_map(|property| match property {
                    Property::Level(level) => Some(*level),
                    _ => None,
                })
                .unwrap_or_default();
            widgets::slider(name, level, 0u8)
        }
        Widget::Toggle => widgets::toggle(icons::GEAR, name, selected),
        Widget::Tooltip => widgets::tip_label(words.to_owned()),
    }
}
