//! A frame's backdrop filter (`Style::backdrop`): a blur and color filters that the
//! renderer applies to the whole window as drawn so far, before it draws the rest of the flat
//! layer over it. The web interface's end screen blurs and darkens the table under the result
//! this way (`.table-dimmed` in `ui/src/screens/styles/table.css`).
//!
//! The filter stands at a place in the flat layer's band (`Band::Layer`), whose records are drawn
//! in the order they are pushed, so the records pushed before it are filtered and the ones pushed
//! after it are drawn sharp over it. The bands after the flat layer's (cards shown inside
//! flat-layer elements and what covers them) are drawn sharp whenever they were pushed.

use super::FrameBuilder;
use crate::element::Filter;

/// The blur and color filters drawn over `draws[..at]` of a finished frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Backdrop {
    /// The records filtered are `draws[..at]`, once the frame is finished.
    pub at: u32,
    /// How many records had been pushed when the filter was asked for, the push order `at`
    /// is found from.
    pub(super) pushed: u32,
    /// The standard deviation of the Gaussian blur, in physical pixels; 0 blurs nothing.
    pub blur: f32,
    /// The color filters as one matrix, applied after the blur to the sRGB values of each pixel,
    /// not premultiplied: each row the weights of red, green and blue.
    pub color: [[f32; 3]; 3],
}

/// The identity matrix: no color filter.
const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

impl FrameBuilder<'_> {
    /// Asks for `filters`, CSS's `backdrop-filter` functions, over every record pushed so far,
    /// in a window of `scale` physical pixels a point. Only the first ask of a frame counts.
    /// What is drawn under the filter is behind a modal element, as the web's end screen is a
    /// dialog over the table, so no element added so far takes a press.
    pub fn backdrop(&mut self, filters: &[Filter], scale: f32) {
        if self.frame.backdrop.is_some() || filters.is_empty() {
            return;
        }
        for element in &mut self.frame.resolved {
            element.interactive = false;
        }
        let mut color = IDENTITY;
        for filter in filters {
            if let Some(matrix) = filter.matrix() {
                color = multiply(matrix, color);
            }
        }
        self.frame.backdrop = Some(Backdrop {
            at: 0,
            pushed: super::index(self.frame.draws.len()),
            blur: crate::element::paint::blur_of(filters) * scale,
            color,
        });
    }
}

/// The product `a` times `b`: `b`'s change, then `a`'s.
fn multiply(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_applies_the_filters_in_order() {
        let brightness = Filter::Brightness(0.5).matrix().unwrap_or(IDENTITY);
        let saturate = Filter::Saturate(0.0).matrix().unwrap_or(IDENTITY);
        let both = multiply(saturate, brightness);
        // Red at full brightness, halved and then made gray, is a gray of half its luminance.
        let red = [both[0][0], both[1][0], both[2][0]];
        for channel in red {
            assert!((channel - 0.5 * 0.213).abs() < 1e-6, "{red:?}");
        }
    }
}
