//! Counting the fragments a frame shades (docs/beryl-3d.md, "What is counted"). GPU time cannot
//! be measured repeatably, but the driver's own count of fragment shader runs can be read with a
//! pipeline statistics query around each of the frame's render passes. On Mesa's software Vulkan
//! driver the count is exact, the same on every run, and leaves out the pixels the depth buffer
//! rejected. Only Vulkan and DirectX 12 offer the query in wgpu, so it is not on Metal.
//!
//! A Baseline frame has one render pass. An Enhanced frame has up to seven: the shadow map's, the
//! multisampled main pass, bloom's four and the flat layer's. Each pass begins the next query of
//! the set, and the frame's count is the sum of the queries it used.

use std::cell::Cell;

/// The size of one query result: a 64-bit count.
const RESULT: u64 = 8;
/// The most render passes a frame records, and so the queries in the set.
const QUERIES: u32 = 12;

/// The query set around the frame's render passes and the buffers its results are read through.
pub(crate) struct FragmentCounter {
    /// The name of the GPU adapter that counts.
    pub(crate) adapter: String,
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    /// How many queries the frame being recorded has begun.
    begun: Cell<u32>,
    /// How many queries the last resolved frame used, which `read` sums.
    resolved: Cell<u32>,
}

impl FragmentCounter {
    pub(crate) fn new(device: &wgpu::Device, adapter: String) -> Self {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("fragments"),
            ty: wgpu::QueryType::PipelineStatistics(
                wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS,
            ),
            count: QUERIES,
        });
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: RESULT * u64::from(QUERIES),
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            adapter,
            set,
            resolve: buffer(
                "fragments resolved",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            read: buffer(
                "fragments readback",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            begun: Cell::new(0),
            resolved: Cell::new(0),
        }
    }

    /// Records, after the frame's last render pass has ended, the copy of each pass's count to
    /// where `read` finds it, and starts the next frame's queries from the first.
    pub(crate) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let used = self.begun.replace(0);
        self.resolved.set(used);
        if used == 0 {
            return;
        }
        encoder.resolve_query_set(&self.set, 0..used, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, RESULT * u64::from(used));
    }

    /// Waits for the GPU and returns the count of the last frame drawn: the sum over its passes.
    pub(crate) fn read(&self, device: &wgpu::Device) -> u64 {
        let used = u64::from(self.resolved.get());
        if used == 0 {
            return 0;
        }
        let slice = self.read.slice(..RESULT * used);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes the frame");
        let count = {
            let mapped = slice.get_mapped_range().expect("the counts' buffer maps");
            mapped
                .as_chunks::<{ RESULT as usize }>()
                .0
                .iter()
                .map(|bytes| u64::from_le_bytes(*bytes))
                .sum()
        };
        self.read.unmap();
        count
    }
}

/// Begins counting the fragments `pass` shades, when the renderer counts. Every `begin` is
/// matched by an `end` on the same pass before it ends.
pub(crate) fn begin(counter: Option<&FragmentCounter>, pass: &mut wgpu::RenderPass<'_>) {
    if let Some(counter) = counter {
        let index = counter.begun.get();
        debug_assert!(
            index < QUERIES,
            "a frame records more than {QUERIES} passes"
        );
        pass.begin_pipeline_statistics_query(&counter.set, index);
        counter.begun.set(index + 1);
    }
}

/// Ends counting the fragments `pass` shades, begun by `begin`.
pub(crate) fn end(counter: Option<&FragmentCounter>, pass: &mut wgpu::RenderPass<'_>) {
    if counter.is_some() {
        pass.end_pipeline_statistics_query();
    }
}
