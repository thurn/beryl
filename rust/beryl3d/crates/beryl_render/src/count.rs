//! Counting the fragments a frame shades (docs/beryl-3d.md, "What is counted"). GPU time cannot
//! be measured repeatably, but the driver's own count of fragment shader runs can be read with a
//! pipeline statistics query around the frame's render pass. On Mesa's software Vulkan driver
//! the count is exact, the same on every run, and leaves out the pixels the depth buffer
//! rejected. Only Vulkan and DirectX 12 offer the query in wgpu, so it is not on Metal.

/// The size of one query result: a 64-bit count.
const RESULT: u64 = 8;

/// The query set around the frame's render pass and the buffers its result is read through.
pub(crate) struct FragmentCounter {
    /// The name of the GPU adapter that counts.
    pub(crate) adapter: String,
    pub(crate) set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

impl FragmentCounter {
    pub(crate) fn new(device: &wgpu::Device, adapter: String) -> Self {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("fragments"),
            ty: wgpu::QueryType::PipelineStatistics(
                wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS,
            ),
            count: 1,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fragments resolved"),
            size: RESULT,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fragments readback"),
            size: RESULT,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            adapter,
            set,
            resolve,
            read,
        }
    }

    /// Records, after the render pass has ended, the copy of its count to where `read` finds it.
    pub(crate) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.set, 0..1, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, RESULT);
    }

    /// Waits for the GPU and returns the count of the last frame drawn.
    pub(crate) fn read(&self, device: &wgpu::Device) -> u64 {
        let slice = self.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes the frame");
        let count = {
            let mapped = slice.get_mapped_range().expect("the count's buffer maps");
            u64::from_le_bytes(mapped[..8].try_into().expect("eight bytes"))
        };
        self.read.unmap();
        count
    }
}
