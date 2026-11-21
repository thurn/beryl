//! Places an element’s children in the boxes solved by the Grid policy.

use kit_base::geometry::Rect;
use kit_space::arrange::Grid;
use kit_ui::element::{Element, Position};

/// Gives the element and its children the grid's solved boxes. An enclosing element
/// can limit the height and scroll, carrying the children's Show relations with it.
#[must_use]
pub fn grid_element(
    element: Element,
    boxes: Vec<Element>,
    grid: Grid,
    width: f32,
    centred: bool,
) -> Element {
    let height = grid.size(boxes.len()).y;
    let region = Rect {
        width,
        height,
        ..Rect::default()
    };
    let positions = grid.boxes(boxes.len(), region, centred);
    element
        .min_width(width)
        .height(height)
        .children(boxes.into_iter().zip(positions).map(|(mut item, rect)| {
            item.style.position = Position::Absolute;
            item.style.inset = [Some(rect.y), None, None, Some(rect.x)];
            item
        }))
}
