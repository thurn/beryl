# Working in `kit_frame`

This toolkit crate defines the resolved frame shared by rendering, hit testing
and inspection. It works with opaque IDs and drawing records. The client
supplies the game meaning and the renderer supplies the GPU work.

Start at `frame::Frame`, `Resolved`, `Draw` and `Material`. Frames retain
storage for reuse. Keep IDs stable within a scene and keep drawing order and the
order used for pointer hits in agreement.

`tests/all/` covers paint order and filtered frame output. Backdrop tests live
beside their code. When paint order changes, check a covered interactive element
as well as the visible pixels.
