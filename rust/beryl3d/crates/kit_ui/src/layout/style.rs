//! An element's `Style` as taffy's: flexbox and grid, absolute position with insets, sizes and
//! their limits, margins, padding and borders, all in logical points (docs/beryl-3d.md,
//! "Elements").

use taffy::style_helpers::{auto, fr, length, line};
use taffy::{
    AlignContent, AlignItems, Dimension, FlexDirection, FlexWrap, GridPlacement, LengthPercentage,
    LengthPercentageAuto, Line,
};

use crate::element::{Align, Direction, Display, Justify, Overflow, Position, Style, Track};

/// Whether two styles use the same layout. Compare the track lists without building taffy's
/// copies, since a grid's paint can change inside a frame without allocating.
pub(super) fn same_layout(old: &Style, new: &Style) -> bool {
    old.grid_template_columns == new.grid_template_columns
        && old.grid_template_rows == new.grid_template_rows
        && layout_fields(old) == layout_fields(new)
}

/// An element's style as taffy's.
pub(super) fn taffy_style(style: &Style) -> taffy::Style {
    taffy::Style {
        grid_template_columns: style
            .grid_template_columns
            .iter()
            .copied()
            .map(track)
            .collect(),
        grid_template_rows: style
            .grid_template_rows
            .iter()
            .copied()
            .map(track)
            .collect(),
        ..layout_fields(style)
    }
}

/// The layout fields shared by comparison and construction. Grid templates are added only
/// when taffy needs a new style.
fn layout_fields(style: &Style) -> taffy::Style {
    let dimension = |value: Option<f32>| value.map_or(Dimension::auto(), Dimension::length);
    let size = |width: Option<f32>, height: Option<f32>| taffy::Size {
        width: dimension(width),
        height: dimension(height),
    };
    let limit = |value: Option<f32>| {
        value.map_or(LengthPercentageAuto::auto(), LengthPercentageAuto::length)
    };
    let limits = |width: Option<f32>, height: Option<f32>| taffy::Size {
        width: limit(width),
        height: limit(height),
    };
    let border = style.border.map_or([0.0; 4], |border| border.width);
    taffy::Style {
        display: match style.display {
            Display::Flex => taffy::Display::Flex,
            Display::Grid => taffy::Display::Grid,
            Display::None => taffy::Display::None,
        },
        position: match style.position {
            Position::Relative => taffy::Position::Relative,
            Position::Absolute => taffy::Position::Absolute,
        },
        inset: sides(style.inset.map(limit)),
        flex_direction: match style.direction {
            Direction::Row => FlexDirection::Row,
            Direction::Column => FlexDirection::Column,
        },
        flex_wrap: if style.wrap {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        gap: taffy::Size {
            width: LengthPercentage::length(style.gap),
            height: LengthPercentage::length(style.gap),
        },
        padding: sides(style.padding.map(LengthPercentage::length)),
        margin: sides(style.margin.map(LengthPercentageAuto::length)),
        border: sides(border.map(LengthPercentage::length)),
        flex_grow: style.grow,
        flex_shrink: style.shrink,
        flex_basis: dimension(style.basis),
        align_items: style.align_items.map_or(AlignItems::NORMAL, align),
        align_self: style.align_self.map(align),
        justify_content: match style.justify_content {
            None => AlignContent::NORMAL,
            Some(Justify::Start) => AlignContent::FLEX_START,
            Some(Justify::Center) => AlignContent::CENTER,
            Some(Justify::End) => AlignContent::FLEX_END,
            Some(Justify::SpaceBetween) => AlignContent::SPACE_BETWEEN,
        },
        size: size(style.width, style.height),
        min_size: limits(style.min_width, style.min_height),
        max_size: limits(style.max_width, style.max_height),
        // An element that clips or scrolls is as large as its own style says, whatever its
        // children need, as in CSS.
        overflow: taffy::Point {
            x: overflow(style.overflow),
            y: overflow(style.overflow),
        },
        grid_column: placement(style.grid_column),
        grid_row: placement(style.grid_row),
        ..taffy::Style::default()
    }
}

fn overflow(overflow: Overflow) -> taffy::Overflow {
    match overflow {
        Overflow::Visible => taffy::Overflow::Visible,
        Overflow::Clip => taffy::Overflow::Clip,
        Overflow::Scroll => taffy::Overflow::Scroll,
    }
}

/// Top, right, bottom and left as taffy's sides.
fn sides<T>([top, right, bottom, left]: [T; 4]) -> taffy::Rect<T> {
    taffy::Rect {
        left,
        right,
        top,
        bottom,
    }
}

fn align(align: Align) -> AlignItems {
    match align {
        Align::Start => AlignItems::FLEX_START,
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::FLEX_END,
        Align::Stretch => AlignItems::STRETCH,
    }
}

fn track(track: Track) -> taffy::GridTemplateComponent<String> {
    match track {
        Track::Points(points) => length(points),
        Track::Fraction(share) => fr(share),
        Track::Auto => auto(),
    }
}

/// The grid lines an element spans, or automatic placement.
fn placement(lines: Option<(i16, i16)>) -> Line<GridPlacement<String>> {
    match lines {
        Some((start, end)) => Line {
            start: line(start),
            end: line(end),
        },
        None => Line {
            start: GridPlacement::Auto,
            end: GridPlacement::Auto,
        },
    }
}
