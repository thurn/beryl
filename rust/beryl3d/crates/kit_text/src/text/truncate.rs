//! Text cut to its box (docs/beryl-3d-architecture.md, "Toolkit capabilities"): a text that
//! takes more lines than its style allows, or a line wider than its box, is cut at the last
//! place where what is left fits, and ends with an ellipsis, as CSS's `text-overflow:
//! ellipsis` cuts one line and `line-clamp` cuts a paragraph after its last allowed line.

use super::{Block, Span, Text, TypeStyle, symbols};

/// How a text that does not fit its box is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Truncate {
    /// The most lines the text takes, at least 1: CSS's `line-clamp`. One line is CSS's
    /// `white-space: nowrap` with `text-overflow: ellipsis`.
    pub lines: u16,
    /// Whether the cut text ends with an ellipsis, or is only cut off.
    pub ellipsis: bool,
}

impl Truncate {
    /// One line, cut with an ellipsis.
    pub const ELLIPSIS: Self = Self {
        lines: 1,
        ellipsis: true,
    };

    /// At most `lines` lines, the last cut with an ellipsis.
    #[must_use]
    pub const fn clamp(lines: u16) -> Self {
        Self {
            lines,
            ellipsis: true,
        }
    }
}

/// What ends a cut text.
const ELLIPSIS: &str = "\u{2026}";

impl Text {
    /// `text` as `block` lays it out at `max_width`, unless it takes more lines than
    /// `truncate` allows or a line is wider than `max_width`: then the longest start of it that
    /// fits, with the ellipsis after it, whose block names the text it shows (`Block::shown`).
    /// A mana symbol is kept whole or cut whole. When not even the ellipsis fits, the block
    /// shows the ellipsis alone.
    pub fn truncated(
        &mut self,
        text: &str,
        style: TypeStyle,
        spans: &[Span],
        max_width: Option<f32>,
        truncate: Truncate,
    ) -> Block {
        let lines = usize::from(truncate.lines.max(1));
        // One line is laid out unbroken, and cut by its width alone.
        let width = if lines == 1 { None } else { max_width };
        let fits = |block: &Block| {
            block.lines.len() <= lines && max_width.is_none_or(|most| block.width <= most)
        };
        let whole = self.block(text, style, spans, width);
        if fits(whole) {
            return whole.clone();
        }
        // The places the text may be cut: every character boundary outside a mana symbol.
        let tokens = symbols::tokens(text);
        let cuts: Vec<usize> = (text.char_indices().map(|(at, _)| at))
            .filter(|at| !tokens.iter().any(|token| token.start < *at && *at < token.end))
            .collect();
        // The cut at `cuts[0]`, the text's start, shows the ellipsis alone, which is shown even
        // when it does not fit. Search for the last cut that fits.
        let (mut low, mut high) = (0, cuts.len());
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            let (shown, cut_spans) = cut(text, spans, cuts[middle], truncate.ellipsis);
            if fits(self.block(&shown, style, &cut_spans, width)) {
                low = middle;
            } else {
                high = middle;
            }
        }
        let (shown, cut_spans) = cut(text, spans, cuts[low], truncate.ellipsis);
        let mut block = self.block(&shown, style, &cut_spans, width).clone();
        block.max_width = max_width;
        block.shown = Some(shown);
        block
    }
}

/// The start of `text` before byte `at`, without the spaces that end it, and the ellipsis if
/// `ellipsis`, with `spans` cut to it.
fn cut(text: &str, spans: &[Span], at: usize, ellipsis: bool) -> (String, Vec<Span>) {
    let kept = text[..at].trim_end();
    let mut shown = String::with_capacity(kept.len() + ELLIPSIS.len());
    shown.push_str(kept);
    if ellipsis {
        shown.push_str(ELLIPSIS);
    }
    let end = u32::try_from(kept.len()).expect("a text is under 4 GB");
    let spans = (spans.iter())
        .filter(|span| span.range.start < end)
        .map(|span| Span {
            range: span.range.start..span.range.end.min(end),
            ..span.clone()
        })
        .collect();
    (shown, spans)
}
