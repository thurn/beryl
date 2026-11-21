//! How a frame's draw list is cut into draw calls (docs/beryl-3d.md, "The frame": few draw
//! calls). Consecutive records that share a pass, a mesh, a texture array and a clip are one
//! instanced draw call, whatever their materials.

use std::ops::Range;

use kit_frame::frame::{Band, Draw, Frame};

/// The five ways a record is drawn, in the order they come in the draw list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// `draws[..opaque]`: no blending, writing the depth buffer.
    Opaque,
    /// `draws[opaque..moving_opaque]`: blended, tested against the depth buffer, not writing it.
    Table,
    /// Fully opaque moving cards, drawn over the resting table.
    MovingOpaque,
    /// `draws[moving..layer]`: blended and shaken, tested against moving opaque cards.
    Moving,
    /// `draws[layer..]`: the flat layer, blended with no depth test and not shaken.
    Layer,
}

/// One instanced draw call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub pass: Pass,
    /// A `Mesh` value.
    pub mesh: u8,
    /// A `TextureArray` value.
    pub array: u8,
    /// An entry of `Frame::clips` in any pass, or 0 for the whole target.
    pub clip: u8,
    /// The records drawn, as a range of `Frame::draws`.
    pub records: Range<u32>,
}

fn mesh(draw: &Draw) -> u8 {
    (draw.flags >> 2) & 0b11
}

fn array(draw: &Draw) -> u8 {
    draw.flags & 0b11
}

/// The draw calls of `frame`, in drawing order.
pub fn runs(frame: &Frame) -> Runs<'_> {
    let total = frame.draws.len() as u32;
    let opaque = frame.opaque.min(total);
    Runs {
        frame,
        next: 0,
        opaque,
        moving_opaque: frame.bands[Band::MovingOpaque as usize].clamp(opaque, total),
        moving: frame.bands[Band::Moving as usize].clamp(opaque, total),
        layer: frame.layer.clamp(opaque, total),
    }
}

/// The iterator `runs` returns. It allocates nothing.
pub struct Runs<'a> {
    frame: &'a Frame,
    next: u32,
    opaque: u32,
    moving_opaque: u32,
    moving: u32,
    layer: u32,
}

impl Runs<'_> {
    fn pass(&self, index: u32) -> Pass {
        if index < self.opaque {
            Pass::Opaque
        } else if index < self.moving_opaque {
            Pass::Table
        } else if index < self.moving {
            Pass::MovingOpaque
        } else if index < self.layer {
            Pass::Moving
        } else {
            Pass::Layer
        }
    }
}

impl Iterator for Runs<'_> {
    type Item = Run;

    fn next(&mut self) -> Option<Run> {
        let draws = &self.frame.draws;
        let start = self.next;
        let first = draws.get(start as usize)?;
        let key =
            |index: u32, draw: &Draw| (self.pass(index), mesh(draw), array(draw), draw.clip());
        let first_key = key(start, first);
        let mut end = start + 1;
        while let Some(draw) = draws.get(end as usize) {
            if key(end, draw) != first_key {
                break;
            }
            end += 1;
        }
        self.next = end;
        let (pass, mesh, array, clip) = first_key;
        Some(Run {
            pass,
            mesh,
            array,
            clip,
            records: start..end,
        })
    }
}

#[cfg(test)]
mod tests {
    use glam::Affine3A;
    use kit_base::atlas::{FaceSize, Tier};
    use kit_frame::frame::{FrameBuilder, Sprite};

    use super::*;

    fn card() -> Draw {
        Draw::card(
            Affine3A::IDENTITY,
            [255; 4],
            FaceSize::Board,
            1,
            0,
            0.0,
            0.0,
        )
    }

    #[test]
    fn records_split_by_pass_mesh_array_and_clip() {
        let mut frame = Frame::with_capacity(0, 16, 0);
        let mut builder =
            FrameBuilder::new(&mut frame, Tier::Baseline, glam::Vec2::new(63.0, 88.0));
        builder.draw(Band::Opaque, card());
        builder.draw(
            Band::Opaque,
            Draw::table(Affine3A::IDENTITY, [0, 0, 0, 255]),
        );
        let shadow = Draw::contact_shadow(Affine3A::IDENTITY, [0, 0, 0, 128], 2.0);
        builder.draw(Band::ContactShadows, shadow);
        builder.draw(Band::ContactShadows, shadow);
        builder.draw(Band::TableMarks, card().edge());
        let glow = Draw::glow(Affine3A::IDENTITY, [255; 4], Sprite::default(), 0.5);
        builder.draw(Band::Glows, glow);
        builder.draw(Band::Layer, glow);
        builder.draw(Band::Layer, glow.with_clip(1));
        let frame = builder.finish();
        let found: Vec<(Pass, Range<u32>)> =
            runs(frame).map(|run| (run.pass, run.records)).collect();
        assert_eq!(
            found,
            [
                (Pass::Opaque, 0..2),
                (Pass::Table, 2..4),
                (Pass::Table, 4..5),
                (Pass::Table, 5..6),
                (Pass::Layer, 6..7),
                (Pass::Layer, 7..8),
            ]
        );
    }
}
