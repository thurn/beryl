//! Text that shrinks to fit its box (`Style::fit`), as the web interface's `useTextFit`
//! (`ui/src/components/fit.ts`) shrinks a button's long label rather than cutting it off: the
//! text stays on one line, and while it is wider than the content box its type size is scaled
//! by 0.98 of how far it overflows, up to 12 times. This runs while `Client::handle` lays the
//! screen out, never while a frame is written.

use super::{Layer, Node};

/// How many times a text is shrunk at most, as `useTextFit` tries.
const TRIES: usize = 12;

/// The share of the box a shrunk text is aimed at, which leaves room for rounding.
const AIM: f32 = 0.98;

impl Layer {
    /// Lays out `node`'s text on one line, shrunk until it fits its content box's width, unless
    /// it was already fitted to that width.
    pub(super) fn fit(&mut self, node: &mut Node) {
        let Some(words) = &node.element.text else {
            return;
        };
        let width = node.content.width;
        if node.block.is_some()
            && node
                .fitted
                .is_some_and(|was| was.to_bits() == width.to_bits())
        {
            return;
        }
        // Each span keeps its bytes and color, and its size is scaled with the text's.
        let element = &node.element;
        let mut scale = 1.0;
        let mut spans = element.spans.clone();
        let mut style = element.style.text;
        for _ in 0..TRIES {
            let block = self.text.block(words, style, &spans, None);
            let along = if width > 0.0 {
                block.width / width
            } else {
                1.0
            };
            if along <= 1.0 {
                break;
            }
            scale *= AIM / along;
            style.size = element.style.text.size * scale;
            for (span, given) in spans.iter_mut().zip(&element.spans) {
                span.style.size = given.style.size * scale;
            }
        }
        node.block = Some(self.text.block(words, style, &spans, None).clone());
        node.fitted = Some(width);
    }
}
