//! The "text overlaps nothing" interface check (docs/beryl-3d.md, "Interface checks"): a line
//! of text drawn over another element's text or over a control is unreadable, and the "no
//! overlap" check sees neither, since it compares only interactive elements. A float of the
//! flat layer, such as a life total or a pool, is laid out as text too, so its lines count.

use std::collections::BTreeSet;

use super::checks::{fault, interactive, subject};
use super::fault::{Context, Fault, Rule};
use super::hit::{intersection, is_within, painted_over, visible_bounds};
use kit_base::data::audit;
use kit_frame::frame::{Frame, Place, Role};
use kit_ui::input::Rect;

/// How far two boxes may reach into each other and not count as overlapping, in logical
/// points: a line's box is as tall as its line, leading and all, so the boxes of two lines set
/// close together may touch where no glyph does.
const SLACK: f32 = 1.0;

/// Text overlaps nothing: no line of text (`Frame::lines`) crosses a line of another element's
/// text or an interactive element of the flat layer, unless one of the two elements holds the
/// other in the tree, as a button holds its label. Each line is judged by the part its
/// element's clip shows, and a line whose element is no more opaque than `audit::TRANSPARENT`
/// is not seen. Cards are left out, as they are of "no overlap": the table's cards lie under
/// the flat layer by design. One fault names each pair of elements once.
#[must_use]
pub fn text_overlap(frame: &Frame, context: &Context) -> Vec<Fault> {
    let lines: Vec<(u32, Rect)> = frame
        .lines
        .iter()
        .filter_map(|line| {
            let element = &frame.resolved[line.element as usize];
            if element.opacity <= audit::TRANSPARENT {
                return None;
            }
            let rect = match element.clip {
                Some(clip) => intersection(line.rect, clip)?,
                None => line.rect,
            };
            Some((line.element, shrunk(rect)?))
        })
        .collect();
    let controls: Vec<(u32, Rect)> = interactive(frame)
        .filter_map(|index| {
            let element = &frame.resolved[index as usize];
            let seen = element.place == Place::Layer
                && element.role != Role::Card
                && element.opacity > audit::TRANSPARENT;
            Some((index, shrunk(visible_bounds(element).filter(|_| seen)?)?))
        })
        .collect();
    let mut pairs = BTreeSet::new();
    for (at, &(first, a)) in lines.iter().enumerate() {
        let others = lines[at + 1..].iter().chain(&controls);
        for &(second, b) in others {
            let related = is_within(frame, first, second) || is_within(frame, second, first);
            let Some(shared) = intersection(a, b) else {
                continue;
            };
            if related || hidden(frame, first, second, shared) {
                continue;
            }
            pairs.insert((first.min(second), first.max(second)));
        }
    }
    pairs
        .into_iter()
        .map(|(first, second)| {
            let (a, b) = (subject(frame, first), subject(frame, second));
            fault(
                Rule::TextOverlap,
                context,
                format!("text of {} overlaps {}", a.describe(), b.describe()),
                vec![a, b],
            )
        })
        .collect()
}

/// Whether a panel drawn after one of the two elements covers the middle of the part where their
/// boxes meet, as an open menu covers the prompt under it: the lower element's text is not seen
/// there, so it overlaps nothing.
fn hidden(frame: &Frame, first: u32, second: u32, shared: Rect) -> bool {
    let middle = glam::Vec2::new(
        shared.x + shared.width / 2.0,
        shared.y + shared.height / 2.0,
    );
    painted_over(frame, first.min(second), middle).is_some()
}

/// `rect` less `SLACK` on each side, or `None` when nothing is left.
fn shrunk(rect: Rect) -> Option<Rect> {
    let (width, height) = (rect.width - 2.0 * SLACK, rect.height - 2.0 * SLACK);
    (width > 0.0 && height > 0.0).then_some(Rect {
        x: rect.x + SLACK,
        y: rect.y + SLACK,
        width,
        height,
    })
}
