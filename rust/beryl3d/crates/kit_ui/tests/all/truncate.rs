//! Text cut to its box with an ellipsis, laid out and written by the flat layer.

use glam::{Affine3A, Vec2};
use kit_base::geometry::{Window, plane::ScreenPlane};
use kit_frame::frame::{Frame, FrameBuilder, TextLine};
use kit_ui::element::{Element, column, text};
use kit_ui::input::Tier;
use kit_ui::layout::Layer;

const LONG: &str = "Return the selected documents to the folder where you originally found them";

/// The frame `layer` writes, with the words of each line kept.
fn write(layer: &mut Layer) -> Frame {
    let mut frame = Frame::with_capacity(64, 256, 4096);
    frame.looking = true;
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline, crate::card());
    let plane = ScreenPlane::new(Affine3A::IDENTITY, Window::default(), 1.0, 1.0);
    layer.write(&mut builder, &plane, true);
    builder.finish();
    frame
}

/// The lines of `frame`'s text, each with its words.
fn lines(frame: &Frame) -> Vec<(TextLine, String)> {
    (frame.lines.iter())
        .map(|line| (*line, frame.name(line.words).trim_end().to_owned()))
        .collect()
}

/// Whether `line` lies inside the box of the element it belongs to.
fn inside(frame: &Frame, line: &TextLine) -> bool {
    let corners = frame.resolved[line.element as usize].corners;
    let min = corners.into_iter().reduce(Vec2::min).unwrap();
    let max = corners.into_iter().reduce(Vec2::max).unwrap();
    let rect = line.rect;
    rect.x >= min.x - 0.01
        && rect.y >= min.y - 0.01
        && rect.x + rect.width <= max.x + 0.01
        && rect.y + rect.height <= max.y + 0.01
}

fn shown(words: Element) -> Frame {
    let mut layer = Layer::new();
    layer.show(Some(column().name("screen").width(160.0).child(words)));
    write(&mut layer)
}

#[test]
fn an_ellipsis_keeps_a_long_line_inside_its_box() {
    let frame = shown(text(LONG).name("rules").ellipsis());
    let lines = lines(&frame);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let (line, words) = &lines[0];
    assert!(words.ends_with('\u{2026}'), "{words}");
    assert!(
        LONG.starts_with(words.trim_end_matches('\u{2026}')),
        "{words}"
    );
    assert!(inside(&frame, line), "{line:?}");
}

#[test]
fn a_line_clamp_keeps_two_lines_inside_the_box() {
    let frame = shown(text(LONG).name("rules").line_clamp(2));
    let lines = lines(&frame);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].1.ends_with('\u{2026}'), "{lines:?}");
    assert!(
        lines.iter().all(|(line, _)| inside(&frame, line)),
        "{lines:?}"
    );
}

#[test]
fn text_without_truncation_takes_every_line() {
    let frame = shown(text(LONG).name("rules"));
    assert!(lines(&frame).len() > 2);
}
