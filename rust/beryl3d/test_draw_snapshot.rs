//! Effect snapshots record the mesh, texture and clip used by the renderer.

use std::fmt::{self, Debug, Formatter};

use kit_frame::frame::Draw;

pub struct DrawSnapshot<'a>(pub &'a Draw);

impl Debug for DrawSnapshot<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let draw = self.0;
        formatter
            .debug_struct("Draw")
            .field("model", &draw.model)
            .field("uv", &draw.uv)
            .field("layer", &draw.layer)
            .field("material", &draw.material)
            .field("mesh", &draw.mesh())
            .field("texture", &draw.texture_array())
            .field("clip", &draw.clip())
            .field("tint", &draw.tint)
            .field("params", &draw.params)
            .finish()
    }
}
