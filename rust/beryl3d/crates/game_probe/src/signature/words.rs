//! Counts lines with the bundled fonts at the widths used by the window profiles.

use game_hud::hud::log::{LogKind, LogLine};
use game_table::profiles::Profile;
use game_table::table::Window;
use game_table::table::badges::counter_label;
use kit_base::data::profiles::Profiles;
use kit_base::tokens::{size, space, type_style};
use kit_text::text::{Text, TypeStyle};

struct Widths {
    inspector: Vec<f32>,
    desktop_prompt: Vec<f32>,
    banded_prompt: Vec<(f32, f32)>,
    log: Vec<f32>,
}

/// Text layouts, reused within a recording and between neighboring recordings.
pub(super) struct Words {
    widths: Widths,
    text: Text,
    /// The style the badges shape a named counter's label in.
    name_style: TypeStyle,
}

impl Words {
    pub(super) fn new(profiles: &Profiles, windows: &[Window], name_style: TypeStyle) -> Self {
        let mut widths = Widths {
            inspector: Vec::new(),
            desktop_prompt: Vec::new(),
            banded_prompt: Vec::new(),
            log: Vec::new(),
        };
        for &window in windows {
            let profile = Profile::for_window(profiles, window);
            if let Some(bands) = profile.bands() {
                let room = bands.right - bands.left;
                if let Some(prompt) = profile.data().hud.floating_prompt {
                    let full = room - 2.0 * prompt.pill_padding;
                    widths.banded_prompt.extend([
                        (full, prompt.pill_text),
                        (
                            full - prompt.button_min_width - prompt.gap,
                            prompt.pill_text,
                        ),
                    ]);
                }
                widths
                    .log
                    .push(room - 2.0 * space::M - 2.0 * size::LOG_PADDING_X);
            } else {
                widths
                    .desktop_prompt
                    .extend([size::BUTTON_WIDTH, size::DOCK_MAX_WIDTH]);
                widths.log.push(size::LOG_WIDTH - 2.0 * size::LOG_PADDING_X);
            }
            if let Some(inspector) = profile.data().table.inspector {
                let height = profile
                    .slot("inspector")
                    .expect("validated slot")
                    .rect
                    .height;
                let mut width = inspector.width;
                while width >= inspector.least_width {
                    widths
                        .inspector
                        .push((width - 2.0 * inspector.pad) * height / inspector.stage_height);
                    width -= inspector.narrow_step;
                }
            }
        }
        for values in [
            &mut widths.inspector,
            &mut widths.desktop_prompt,
            &mut widths.log,
        ] {
            values.sort_by(f32::total_cmp);
            values.dedup();
        }
        widths
            .banded_prompt
            .sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        widths.banded_prompt.dedup();
        Self {
            widths,
            text: Text::new(),
            name_style,
        }
    }

    pub(super) fn end_recording(&mut self) {
        self.text.end_pass();
    }

    pub(super) fn counter_label_shape(&mut self, name: &str) -> (u32, usize) {
        let block = self
            .text
            .block(&counter_label(name), self.name_style, &[], None);
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
        let mut counts: Vec<_> = lines(
            &mut self.text,
            words,
            type_style::TITLE,
            &self.widths.desktop_prompt,
        )
        .collect();
        counts.extend(self.widths.banded_prompt.iter().map(|&(width, size)| {
            let style = TypeStyle {
                size,
                ..type_style::TITLE
            };
            line_count(&mut self.text, words, style, width)
        }));
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
    widths
        .iter()
        .map(move |&width| line_count(text, words, style, width))
}

fn line_count(text: &mut Text, words: &str, style: TypeStyle, width: f32) -> usize {
    if words.is_empty() {
        0
    } else {
        text.block(words, style, &[], Some(width)).lines.len()
    }
}
