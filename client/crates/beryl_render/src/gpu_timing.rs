//! Observes scene submission completions and samples the main render pass's GPU timestamps.
//! The render thread uses fixed queues and query buffers. Only the worker waits or maps.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

const MAX_TRACKED: usize = 16;
const QUERY_SLOTS: usize = 4;
const QUERY_STRIDE: u64 = 8;
const QUERY_BYTES: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuCompletion {
    pub tick: u64,
    pub submit_ns: u64,
    pub observed_ns: u64,
    pub main_pass_gpu_ns: Option<u64>,
    pub main_pass_sampled: bool,
}

pub struct GpuTimer {
    epoch: Instant,
    queries: [Option<Arc<Query>>; QUERY_SLOTS],
    sequence: Cell<u64>,
    pending: Arc<AtomicU32>,
    submitted: SyncSender<Submitted>,
    finished: Receiver<GpuCompletion>,
}

pub(crate) struct Work {
    tick: u64,
    query: Option<Arc<Query>>,
}

struct Submitted {
    work: Work,
    index: wgpu::SubmissionIndex,
    submit_ns: u64,
}

struct Query {
    stamps: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    busy: AtomicBool,
}

impl GpuTimer {
    pub(crate) fn new(
        device: wgpu::Device,
        period: f32,
        epoch: Instant,
        worker_setup: fn(),
    ) -> Option<Self> {
        let queries = std::array::from_fn(|_| {
            device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY)
                .then(|| Arc::new(Query::new(&device)))
        });
        let pending = Arc::new(AtomicU32::new(0));
        let count = Arc::clone(&pending);
        let (submitted, work) = sync_channel(MAX_TRACKED);
        let (done, finished) = sync_channel(2 * MAX_TRACKED);
        std::thread::Builder::new()
            .name("scene GPU timing".to_owned())
            .spawn(move || {
                worker_setup();
                for item in work {
                    let sample = observe(&device, f64::from(period), epoch, &item);
                    count.fetch_sub(1, Ordering::Relaxed);
                    if let Some(sample) = sample {
                        let _ = done.try_send(sample);
                    }
                }
            })
            .ok()?;
        Some(Self {
            epoch,
            queries,
            sequence: Cell::new(0),
            pending,
            submitted,
            finished,
        })
    }

    pub fn drain(&self, mut receive: impl FnMut(GpuCompletion)) {
        self.finished.try_iter().for_each(&mut receive);
    }

    pub(crate) fn begin(&self, tick: u64) -> Option<Work> {
        if self.pending.load(Ordering::Relaxed) >= MAX_TRACKED as u32 {
            return None;
        }
        let sequence = self.sequence.get();
        self.sequence.set(sequence + 1);
        let query = if sequence.is_multiple_of(QUERY_STRIDE) {
            self.queries
                .iter()
                .flatten()
                .find(|slot| !slot.busy.swap(true, Ordering::Acquire))
                .cloned()
        } else {
            None
        };
        Some(Work { tick, query })
    }

    pub(crate) fn now(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    pub(crate) fn submitted(
        &self,
        work: Work,
        index: wgpu::SubmissionIndex,
        submit_ns: u64,
    ) -> Option<u32> {
        let pending = self.pending.fetch_add(1, Ordering::Relaxed);
        // The host is the sole producer and checks capacity in begin. If the worker exits,
        // leave its abandoned query slot unavailable so it cannot be reused while on the GPU.
        if self
            .submitted
            .try_send(Submitted {
                work,
                index,
                submit_ns,
            })
            .is_ok()
        {
            Some(pending)
        } else {
            self.pending.fetch_sub(1, Ordering::Relaxed);
            None
        }
    }
}

impl Work {
    pub(crate) fn timestamps(&self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.query
            .as_ref()
            .map(|query| wgpu::RenderPassTimestampWrites {
                query_set: &query.stamps,
                beginning_of_pass_write_index: Some(0),
                end_of_pass_write_index: Some(1),
            })
    }

    pub(crate) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(query) = &self.query {
            encoder.resolve_query_set(&query.stamps, 0..2, &query.resolve, 0);
            encoder.copy_buffer_to_buffer(&query.resolve, 0, &query.read, 0, Some(QUERY_BYTES));
        }
    }
}

impl Query {
    fn new(device: &wgpu::Device) -> Self {
        let stamps = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("main pass timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let buffer = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("main pass timing"),
                size: QUERY_BYTES,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            stamps,
            resolve: buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            read: buffer(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ),
            busy: AtomicBool::new(false),
        }
    }

    fn read(&self, period: f64) -> Option<u64> {
        let view = self.read.slice(..).get_mapped_range().ok()?;
        let start = u64::from_ne_bytes(view[..8].try_into().ok()?);
        let end = u64::from_ne_bytes(view[8..].try_into().ok()?);
        (start != 0 && end > start && end != u64::MAX && period > 0.0)
            .then(|| ((end - start) as f64 * period) as u64)
    }
}

fn observe(
    device: &wgpu::Device,
    period: f64,
    epoch: Instant,
    item: &Submitted,
) -> Option<GpuCompletion> {
    let query = item.work.query.as_ref();
    let mapped = query.map(|query| {
        let (ready, mapped) = sync_channel(1);
        query
            .read
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |status| {
                let _ = ready.try_send(status.is_ok());
            });
        mapped
    });
    // Check the fence for this submission without holding wgpu's resource guard while the
    // GPU is still working. The worker sleeps between checks; the observed time includes that
    // sampling delay as well as CPU submission and GPU queueing.
    loop {
        match device.poll(wgpu::PollType::Wait {
            submission_index: Some(item.index.clone()),
            timeout: Some(Duration::ZERO),
        }) {
            Ok(_) => break,
            Err(wgpu::PollError::Timeout) => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => return None,
        }
    }
    let observed_ns = epoch.elapsed().as_nanos() as u64;
    let main_pass_gpu_ns = query.zip(mapped).and_then(|(query, mapped)| {
        let time = (mapped.recv_timeout(Duration::from_secs(1)).ok() == Some(true))
            .then(|| query.read(period))
            .flatten();
        query.read.unmap();
        query.busy.store(false, Ordering::Release);
        time
    });
    Some(GpuCompletion {
        tick: item.work.tick,
        submit_ns: item.submit_ns,
        observed_ns,
        main_pass_gpu_ns,
        main_pass_sampled: query.is_some(),
    })
}

impl crate::Renderer {
    pub(crate) fn submit(
        &mut self,
        mut encoder: wgpu::CommandEncoder,
        timing: Option<(&GpuTimer, &mut Option<u32>, Work)>,
    ) {
        if let Some(counter) = &self.counter {
            counter.resolve(&mut encoder);
        }
        if let Some((_, _, work)) = &timing {
            work.resolve(&mut encoder);
        }
        let buffer = encoder.finish();
        let at = timing.as_ref().map_or(0, |(timer, _, _)| timer.now());
        let submission = self.queue.submit([buffer]);
        if let Some((timer, pending, work)) = timing {
            *pending = timer.submitted(work, submission.clone(), at);
        }
        self.slots.submitted(submission);
    }
}
