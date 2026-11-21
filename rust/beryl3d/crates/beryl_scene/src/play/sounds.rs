//! The sounds a playing script lists in frames as its clock reaches them.

use super::Playing;
use crate::hud::counts::Counts;
use crate::light::Lit;
use kit_frame::frame::FrameBuilder;

impl Playing {
    /// Gives `lit` and `counts` what the script's clock has reached by `now` (`light`) and lists
    /// its sounds in the frame `builder` makes.
    pub(super) fn give(
        &mut self,
        now: u64,
        (lit, counts): (&mut Lit, &mut Counts),
        builder: &mut FrameBuilder,
    ) {
        self.light(now, lit, counts);
        self.sound(now, builder);
    }

    /// Lists in the frame each sound of the script whose real time is at or before `now` that
    /// no frame has listed yet, in order.
    pub(super) fn sound(&mut self, now: u64, builder: &mut FrameBuilder) {
        while let Some(run) = self.compiled.sounds.get(self.sounds_given) {
            if self.real(run.at) > now {
                break;
            }
            builder.sound(run.sound);
            self.sounds_given += 1;
        }
    }
}
