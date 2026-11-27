//! Generic frame checks for a standalone story.

use crate::agent::{Checked, Context, checks};

use super::Player;

impl Player {
    /// Checks the story at the supplied times, comparing persistent lines across frames.
    pub fn check(&mut self, times: impl IntoIterator<Item = u64>, context: &Context) -> Checked {
        let mut checked = Checked::default();
        let mut context = context.clone();
        self.look(true);
        let mut kept = Vec::new();
        for time in times {
            self.seek(time);
            context.at = time;
            let frame = self.frame();
            checked.frames += 1;
            checked.judged.frame(frame, &context);
            checked.judged.lines(&kept, frame);
            for fault in checks::check(frame, &context)
                .into_iter()
                .chain(checks::lines_hold(&kept, frame, &context))
            {
                if !checked.faults.iter().any(|previous| {
                    previous.rule == fault.rule && previous.elements == fault.elements
                }) {
                    checked.faults.push(fault);
                }
            }
            checked.line_pairs += usize::from(checks::arrows_held(&kept, frame) != 0);
            checks::keep_arrows(frame, &mut kept);
        }
        checked
    }
}
