//! The buffers each frame's uniforms and records are written into (docs/beryl-3d.md,
//! "Measurements").
//!
//! `Queue::write_buffer` copies through a staging buffer, so on Metal every frame that used it
//! encoded a blit, and Apple's driver keeps a pool of about 112 MB for as long as a process
//! encodes a blit at least once a second. On Metal the renderer instead keeps `SLOTS` sets of
//! buffers the CPU writes directly (`wgpu::Features::MAPPABLE_PRIMARY_BUFFERS`, cheap on Apple's
//! unified memory), used in turn so that the CPU never writes a set the GPU may still read. A
//! frame then encodes only its render pass, and a blit happens only when a glyph page or card
//! face is uploaded. Elsewhere (OpenGL ES, Vulkan, DirectX 12, and the floor's limits in tests),
//! one set of buffers is written with `Queue::write_buffer`, as before.

/// How many sets of buffers the mapped path cycles through: the frame being written, and up to
/// two the GPU may still be drawing, as many as a surface has in flight.
const SLOTS: usize = 3;

/// The feature the mapped path needs.
const MAPPED: wgpu::Features = wgpu::Features::MAPPABLE_PRIMARY_BUFFERS;

/// The features to ask a device on `adapter` for so that frames write their buffers directly:
/// `MAPPABLE_PRIMARY_BUFFERS` on Metal, where it removes the blit pool, and nothing elsewhere,
/// where a discrete GPU would read such a buffer across the bus on every draw.
pub(crate) fn mapped_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    if adapter.get_info().backend == wgpu::Backend::Metal {
        adapter.features() & MAPPED
    } else {
        wgpu::Features::empty()
    }
}

/// One frame's buffers and the bind groups that name its uniforms.
pub(crate) struct Slot {
    pub(crate) uniforms: wgpu::Buffer,
    pub(crate) instances: wgpu::Buffer,
    /// One bind group per texture array, by `TextureArray` value.
    pub(crate) bind_groups: [wgpu::BindGroup; 3],
    /// Which of the sets it is, counted from 0 in the order `Slots::uniforms` lists them.
    pub(crate) index: usize,
    /// The submission that last read the buffers, when they are being mapped again after it.
    submitted: Option<wgpu::SubmissionIndex>,
}

/// The sets of buffers frames are written into, in turn.
pub(crate) struct Slots {
    sets: Vec<Slot>,
    /// Whether the buffers are written mapped, rather than with `Queue::write_buffer`.
    mapped: bool,
    /// The slot the last `write` filled.
    current: usize,
    /// Whether that slot was written and neither taken by `take` nor given back by `skip`.
    written: bool,
}

impl Slots {
    /// The buffers for a device, `uniforms` and `instances` bytes each, mapped when the device
    /// has `MAPPABLE_PRIMARY_BUFFERS`. `bind` makes a slot's bind groups from its uniforms.
    pub(crate) fn new(
        device: &wgpu::Device,
        uniforms: u64,
        instances: u64,
        bind: impl Fn(&wgpu::Buffer) -> [wgpu::BindGroup; 3],
    ) -> Self {
        let mapped = device.features().contains(MAPPED);
        let (count, write) = if mapped {
            (SLOTS, wgpu::BufferUsages::MAP_WRITE)
        } else {
            (1, wgpu::BufferUsages::COPY_DST)
        };
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: usage | write,
                mapped_at_creation: mapped,
            })
        };
        let sets = (0..count)
            .map(|index| {
                let uniforms = buffer("uniforms", uniforms, wgpu::BufferUsages::UNIFORM);
                let bind_groups = bind(&uniforms);
                Slot {
                    uniforms,
                    instances: buffer("records", instances, wgpu::BufferUsages::VERTEX),
                    bind_groups,
                    index,
                    submitted: None,
                }
            })
            .collect();
        Self {
            sets,
            mapped,
            current: 0,
            written: false,
        }
    }

    /// Every slot's uniform buffer, by `Slot::index`.
    pub(crate) fn uniforms(&self) -> impl Iterator<Item = &wgpu::Buffer> {
        self.sets.iter().map(|slot| &slot.uniforms)
    }

    /// Makes every slot's bind groups again, after the texture arrays changed.
    pub(crate) fn rebind(&mut self, bind: impl Fn(&wgpu::Buffer) -> [wgpu::BindGroup; 3]) {
        for slot in &mut self.sets {
            slot.bind_groups = bind(&slot.uniforms);
        }
    }

    /// Writes a frame's `uniforms` and records into the next slot, or returns `None` when the
    /// device was lost while the slot was being mapped. On the mapped path this waits for the GPU
    /// only when it is still drawing the frame `SLOTS` frames back, and the buffers stay mapped
    /// until `take`, so that a frame can write its slot before it has a drawable to draw into
    /// and give the slot back with `skip` if it gets none.
    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uniforms: &[u8],
        records: &[u8],
    ) -> Option<()> {
        debug_assert!(
            !self.written,
            "a written slot was neither taken nor skipped"
        );
        self.current = (self.current + 1) % self.sets.len();
        let slot = &mut self.sets[self.current];
        if !self.mapped {
            queue.write_buffer(&slot.uniforms, 0, uniforms);
            if !records.is_empty() {
                queue.write_buffer(&slot.instances, 0, records);
            }
            self.written = true;
            return Some(());
        }
        // `submitted` asked for the buffers to be mapped once that submission ends, and a wait
        // for it returns after the mapping callbacks have run.
        if let Some(submission) = slot.submitted.take() {
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: None,
                })
                .ok()?;
        }
        write_mapped(&slot.uniforms, uniforms)?;
        write_mapped(&slot.instances, records)?;
        self.written = true;
        Some(())
    }

    /// The slot the last `write` filled, ready for a render pass to read: on the mapped path its
    /// buffers are unmapped, as a submission that reads them requires.
    pub(crate) fn take(&mut self) -> &Slot {
        let slot = &self.sets[self.current];
        if self.mapped && self.written {
            slot.uniforms.unmap();
            slot.instances.unmap();
        }
        self.written = false;
        slot
    }

    /// Gives back the slot the last `write` filled, for a frame that will not be drawn: the next
    /// `write` fills the same slot again. Its buffers are still mapped and no submission reads
    /// them, so that `write` does not wait.
    pub(crate) fn skip(&mut self) {
        if self.written {
            self.current = (self.current + self.sets.len() - 1) % self.sets.len();
            self.written = false;
        }
    }

    /// Records that `submission` reads the slot `take` last returned, and on the mapped path
    /// asks for its buffers to be mapped again once the GPU has finished with them.
    pub(crate) fn submitted(&mut self, submission: wgpu::SubmissionIndex) {
        if !self.mapped {
            return;
        }
        let slot = &mut self.sets[self.current];
        // The callbacks capture nothing: `write` learns that the mapping is done by waiting for
        // the submission, not from them.
        slot.uniforms
            .slice(..)
            .map_async(wgpu::MapMode::Write, |_| {});
        slot.instances
            .slice(..)
            .map_async(wgpu::MapMode::Write, |_| {});
        slot.submitted = Some(submission);
    }
}

/// Copies `bytes` to the start of the mapped `buffer`, or returns `None` when it is not mapped.
fn write_mapped(buffer: &wgpu::Buffer, bytes: &[u8]) -> Option<()> {
    if !bytes.is_empty() {
        let mut view = buffer.get_mapped_range_mut(..bytes.len() as u64).ok()?;
        view.copy_from_slice(bytes);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::{SLOTS, Slots, mapped_features};

    /// A device with the mapped buffers where the adapter has them (Metal), as a window's has.
    fn device() -> (wgpu::Device, wgpu::Queue) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("a GPU adapter");
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: mapped_features(&adapter),
            ..wgpu::DeviceDescriptor::default()
        }))
        .expect("a GPU device")
    }

    /// A frame that gets no drawable writes its slot and gives it back with `skip`. The next
    /// frame must then write the same slot without failing to map it, and every slot must go on
    /// mapping after the submissions that read it.
    #[test]
    fn a_skipped_frame_leaves_the_next_write_able_to_map_its_slot() {
        let (device, queue) = device();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[],
        });
        let bind = |_: &wgpu::Buffer| {
            std::array::from_fn(|_| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &layout,
                    entries: &[],
                })
            })
        };
        let mut slots = Slots::new(&device, 256, 256, bind);
        let (uniforms, records) = ([1_u8; 64], [2_u8; 128]);
        for frame in 0..4 * SLOTS {
            assert!(
                slots.write(&device, &queue, &uniforms, &records).is_some(),
                "frame {frame} could not write its slot"
            );
            if frame % 3 == 1 {
                let skipped = slots.current;
                slots.skip();
                assert!(slots.write(&device, &queue, &uniforms, &records).is_some());
                assert_eq!(slots.current, skipped, "the skipped slot is written again");
            }
            slots.take();
            slots.submitted(queue.submit([]));
        }
    }
}
