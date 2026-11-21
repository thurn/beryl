//! Counts lines with the bundled fonts at the widths used by the window profiles.

use game_hud::hud::log::{LogKind, LogLine};
use game_table::profiles::phone::phone_bands;
use game_table::table::Window;
use game_table::table::badges::{NAME_STYLE, counter_label};
use kit_base::data::space::hud::phone;
use kit_base::data::space::layout::desktop::inspector;
use kit_base::tokens::{size, space, type_style};
use kit_text::text::{Text, TypeStyle};

struct Widths {
    inspector: Vec<f32>,
    desktop_prompt: Vec<f32>,
    phone_prompt: Vec<f32>,
    log: Vec<f32>,
}

/// Text layouts, reused within a recording and between neighboring recordings.
pub(super) struct Words {
    widths: Widths,
    text: Text,
}

impl Words {
    pub(super) fn new(windows: &[Window]) -> Self {
        let mut widths = Widths {
            inspector: Vec::new(),
            desktop_prompt: Vec::new(),
            phone_prompt: Vec::new(),
            log: Vec::new(),
        };
        for &window in windows {
            if let Some(bands) = phone_bands(window) {
                let room = bands.right - bands.left;
                widths.phone_prompt.extend([
                    room - 2.0 * phone::PILL_PADDING,
                    room - 2.0 * phone::PILL_PADDING - phone::BUTTON_MIN_WIDTH - phone::DOCK_GAP,
                ]);
                widths
                    .log
                    .push(room - 2.0 * space::M - 2.0 * size::LOG_PADDING_X);
            } else {
                widths
                    .desktop_prompt
                    .extend([size::BUTTON_WIDTH, size::DOCK_MAX_WIDTH]);
                widths.log.push(size::LOG_WIDTH - 2.0 * size::LOG_PADDING_X);
                let mut width = inspector::WIDTH;
                while width >= inspector::LEAST_WIDTH {
                    widths.inspector.push(
                        (width - 2.0 * inspector::PAD) * window.size.height
                            / inspector::STAGE_HEIGHT,
                    );
                    width -= inspector::NARROW_STEP;
                }
            }
        }
        for values in [
            &mut widths.inspector,
            &mut widths.desktop_prompt,
            &mut widths.phone_prompt,
            &mut widths.log,
        ] {
            values.sort_by(f32::total_cmp);
            values.dedup();
        }
        Self {
            widths,
            text: Text::new(),
        }
    }

    pub(super) fn end_recording(&mut self) {
        self.text.end_pass();
    }

    pub(super) fn counter_label_shape(&mut self, name: &str) -> (u32, usize) {
        let block = self.text.block(&counter_label(name), NAME_STYLE, &[], None);
        (block.width.to_bits(), block.glyphs.len())
    }

    pub(super) fn inspector(&mut self, words: &str) -> Vec<usize> {
        lines(
            &mut self.text,
            words,
            type_style::BODY,
            &self.widths.inspector,
        )
        .collect()
    }

    pub(super) fn prompt(&mut self, words: &str) -> Vec<usize> {
        let phone_style = TypeStyle {
            size: phone::PILL_TEXT,
            ..type_style::TITLE
        };
        let mut counts: Vec<_> = lines(
            &mut self.text,
            words,
            type_style::TITLE,
            &self.widths.desktop_prompt,
        )
        .collect();
        counts.extend(lines(
            &mut self.text,
            words,
            phone_style,
            &self.widths.phone_prompt,
        ));
        counts
    }

    pub(super) fn log(&mut self, line: &LogLine) -> Vec<usize> {
        let (style, words) = match line.kind {
            LogKind::Turn => (
                type_style::LOG_TURN,
                std::borrow::Cow::Owned(line.text.to_uppercase()),
            ),
            LogKind::Counter | LogKind::Action => (
                type_style::LOG,
                std::borrow::Cow::Borrowed(line.text.as_str()),
            ),
        };
        lines(&mut self.text, &words, style, &self.widths.log).collect()
    }

    pub(super) fn manual_log(&mut self, words: &str) -> Vec<usize> {
        lines(&mut self.text, words, type_style::LOG, &self.widths.log).collect()
    }
}

fn lines<'a>(
    text: &'a mut Text,
    words: &'a str,
    style: TypeStyle,
    widths: &'a [f32],
) -> impl Iterator<Item = usize> + 'a {
    widths.iter().map(move |&width| {
        if words.is_empty() {
            0
        } else {
            text.block(words, style, &[], Some(width)).lines.len()
        }
    })
}
