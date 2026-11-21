//! The frame's paint table (docs/beryl-3d.md, "The frame"): the color stops of every gradient
//! the frame draws and the mask of each clip, which the renderer uploads beside the frame's
//! uniforms and its shader reads by index. A draw record has no room for a gradient's stops, so a
//! `Gradient` record names its entry of the table in its `layer`, and a mask belongs to a clip,
//! whose entry every record already names in its flags. Building the table allocates nothing.

use super::{CLIPS, FrameBuilder};
use kit_base::geometry::Rect;
use kit_base::paint::{MOST_STOPS, Resolved};

/// How many gradients a frame's table holds. Elements whose gradients resolve to the same stops
/// share an entry.
pub const GRADIENTS: usize = 32;

/// One gradient's stops as the shader reads them: each color as RGBA bytes in one `u32` (red in
/// the low byte), not premultiplied, and its offset along the gradient line, from 0 to 1. Stops
/// past the gradient's last repeat it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GradientEntry {
    pub colors: [u32; MOST_STOPS],
    pub offsets: [f32; MOST_STOPS],
}

const _: () = assert!(size_of::<GradientEntry>() == 64);

impl GradientEntry {
    /// The entry of a gradient resolved against its box.
    #[must_use]
    pub fn of(resolved: &Resolved) -> Self {
        Self {
            colors: resolved.colors.map(u32::from_le_bytes),
            offsets: resolved.offsets,
        }
    }
}

/// The mask of one clip: a linear gradient over the box `rect` (in physical pixels: x, y,
/// width, height) whose opacity scales every fragment of the records drawn under the clip.
/// `line` is the gradient's angle in radians (0 up, clockwise), its entry in the gradients as a
/// number, and 1 when the clip is masked or 0 when it is not, then 0.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaskEntry {
    pub rect: [f32; 4],
    pub line: [f32; 4],
}

/// The paint table as the shader's uniform block lays it out: the masks, by clip entry, then the
/// gradients.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PaintTable {
    pub masks: [MaskEntry; CLIPS],
    pub gradients: [GradientEntry; GRADIENTS],
}

/// The frame's paint table and how much of it is used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paints {
    pub table: PaintTable,
    /// How many of `table.gradients` are used.
    pub gradients: u8,
    /// Whether any clip has a mask.
    pub masked: bool,
    /// Whether a gradient was asked for after all `GRADIENTS` entries were taken. The element is
    /// drawn with no gradient, and the checks report it as a fault.
    pub full: bool,
}

impl Default for Paints {
    fn default() -> Self {
        Self {
            table: bytemuck::Zeroable::zeroed(),
            gradients: 0,
            masked: false,
            full: false,
        }
    }
}

impl Paints {
    /// The bytes of the table the frame uses, which the renderer uploads: none when it draws no
    /// gradient and no mask, and otherwise the masks and the gradients up to the last used.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        if self.gradients == 0 && !self.masked {
            return &[];
        }
        let used = size_of::<[MaskEntry; CLIPS]>()
            + usize::from(self.gradients) * size_of::<GradientEntry>();
        &bytemuck::bytes_of(&self.table)[..used]
    }
}

impl FrameBuilder<'_> {
    /// The entry of the frame's gradients that holds `resolved`'s stops, adding it if no entry
    /// holds the same, or `None` once all `GRADIENTS` are taken, which sets `Paints::full`.
    pub fn gradient(&mut self, resolved: &Resolved) -> Option<u16> {
        let entry = GradientEntry::of(resolved);
        let paints = &mut self.frame.paints;
        let used = usize::from(paints.gradients);
        if let Some(at) = paints.table.gradients[..used]
            .iter()
            .position(|held| *held == entry)
        {
            return u16::try_from(at).ok();
        }
        if used >= GRADIENTS {
            paints.full = true;
            return None;
        }
        paints.table.gradients[used] = entry;
        paints.gradients += 1;
        u16::try_from(used).ok()
    }

    /// Masks the records drawn under clip entry `clip` with the linear gradient at entry
    /// `gradient` of the frame's gradients, run at `angle` radians over `rect`, in physical
    /// pixels.
    pub fn mask(&mut self, clip: u8, rect: Rect, angle: f32, gradient: u16) {
        let paints = &mut self.frame.paints;
        paints.masked = true;
        paints.table.masks[usize::from(clip)] = MaskEntry {
            rect: [rect.x, rect.y, rect.width, rect.height],
            line: [angle, f32::from(gradient), 1.0, 0.0],
        };
    }

    /// The mask of clip entry `clip`, if it has one: its box in physical pixels, its angle and
    /// its gradient's entry, so that a clip made inside it can carry it on.
    #[must_use]
    pub fn mask_of(&self, clip: u8) -> Option<(Rect, f32, u16)> {
        let entry = self.frame.paints.table.masks.get(usize::from(clip))?;
        if entry.line[2] == 0.0 {
            return None;
        }
        let [x, y, width, height] = entry.rect;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an entry of GRADIENTS, written as a whole number"
        )]
        let gradient = entry.line[1] as u16;
        Some((
            Rect {
                x,
                y,
                width,
                height,
            },
            entry.line[0],
            gradient,
        ))
    }
}
