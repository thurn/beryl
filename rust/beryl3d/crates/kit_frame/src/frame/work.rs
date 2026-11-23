//! What building a frame cost: the numbers that predict the GPU's load, the screen area of its
//! records, and the work the client core did, which the gates hold to budgets.

/// Numbers that predict the GPU's load, added up while the frame is built. The screen areas,
/// which take longer to measure, are `Frame::areas`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub draw_calls: u32,
    pub instances: u32,
    pub upload_bytes: u32,
    /// The particle records of the frame, which only the Enhanced tier writes.
    pub particles: u32,
}

/// The screen area of a frame's records, in screens (`Frame::areas`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Areas {
    /// The screen area of the opaque records.
    pub opaque: f32,
    /// The screen area of the blended records.
    pub blended: f32,
    /// The screen area of contact shadows cast on the table.
    pub contact_shadows: f32,
    /// The screen area of other blended records before the flat layer.
    pub scene_blended: f32,
    /// The screen area of blended records in the flat layer and the bands above it.
    pub layer_blended: f32,
}

/// The work the client core did, counted so that the gates can hold it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Elements visited: each object slot of the table and each node of the flat layer that
    /// the frame looks at, whether or not it draws it.
    pub elements: u32,
    /// Animation segments evaluated: one for each object whose pose comes from a track.
    pub segments: u32,
    /// Layout nodes computed, in `handle`.
    pub layout_nodes: u32,
    /// Runs of text shaped, in `handle`.
    pub text_runs: u32,
    /// Draw records written.
    pub records: u32,
    /// Objects placed: each object of `Placement::rests` whose resting place the table worked
    /// out, whether in `handle`, compiling a script step by step, or in the frame. A group whose
    /// resting places are copied from a placement already made counts nothing, and neither do
    /// the abilities on the stack, which are few.
    pub placed: u32,
    /// Script steps compiled, in `handle` or in the frame: each step of each script the show
    /// was given or compiled again. A step costs work even where it places nothing and writes
    /// no segment, such as an arrow that opens.
    pub steps: u32,
}
