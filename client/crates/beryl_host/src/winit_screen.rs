//! Draws the desktop window and hands the host timing data from the renderer.

use std::sync::Arc;
use std::time::{Duration, Instant};

use beryl_render::{GpuTimer, Presented, Renderer, Surface};
use kit_frame::frame::Frame;
use winit::window::Window;

use crate::frames::{Draw, FrameLoop, Shown, Ticks};
use crate::timing::{DisplayTick, GpuTiming, Outcome, Timing, elapsed};

#[derive(Clone, Copy, Default)]
pub(super) struct DrawTiming {
    pub outcome: Option<Outcome>,
    pub ambient_motion: bool,
    pub total_ns: u64,
    pub wait_ns: Option<u64>,
    pub encode_ns: Option<u64>,
    pub present_ns: Option<u64>,
    pub submitted_ns: Option<u64>,
    pub gpu_in_flight: Option<u32>,
}

pub(super) struct Screen {
    pub renderer: Renderer,
    pub surface: Surface,
    window: Arc<Window>,
    slow_gpu: Duration,
    epoch: Option<Instant>,
    gpu: Option<GpuTimer>,
    tick: Option<DisplayTick>,
    timing: DrawTiming,
}

impl Screen {
    pub fn new(
        renderer: Renderer,
        surface: Surface,
        window: Arc<Window>,
        slow_gpu: Duration,
        epoch: Option<Instant>,
    ) -> Self {
        let gpu =
            epoch.and_then(|epoch| renderer.gpu_timer(epoch, crate::threads::interactive_thread));
        Self {
            renderer,
            surface,
            window,
            slow_gpu,
            epoch,
            gpu,
            tick: None,
            timing: DrawTiming::default(),
        }
    }

    pub fn begin(&mut self, tick: Option<DisplayTick>) {
        self.tick = tick;
        self.timing = DrawTiming::default();
    }

    pub fn size(&self) -> [u32; 2] {
        self.surface.size()
    }

    pub fn timing(&self) -> DrawTiming {
        self.timing
    }

    pub fn drain(&self, frames: &mut FrameLoop<impl Ticks>) {
        if let Some(gpu) = &self.gpu {
            gpu.drain(|sample| {
                frames.note_timing(&Timing::Gpu(GpuTiming {
                    sequence: sample.tick,
                    submit_ns: sample.submit_ns,
                    observed_ns: sample.observed_ns,
                    main_pass_gpu_ns: sample.main_pass_gpu_ns,
                    main_pass_sampled: sample.main_pass_sampled,
                }));
            });
        }
    }
}

impl Draw for Screen {
    fn draw(&mut self, frame: &Frame) -> Option<Shown> {
        self.timing.ambient_motion = frame.ambient_motion;
        let started = Instant::now();
        let presented = self.renderer.present_timed(
            frame,
            &mut self.surface,
            || {
                if !self.slow_gpu.is_zero() {
                    std::thread::sleep(self.slow_gpu);
                }
                self.window.pre_present_notify();
            },
            self.gpu.as_ref().zip(self.tick.map(|tick| tick.sequence)),
        );
        let total = elapsed(started);
        self.timing.total_ns = total;
        match presented {
            Presented::Shown {
                waited,
                render_cpu,
                present_cpu,
                gpu_in_flight,
                ..
            } => {
                let waited = waited.as_nanos() as u64;
                let present = present_cpu.as_nanos() as u64;
                self.timing.outcome = Some(Outcome::Submitted);
                self.timing.wait_ns = Some(waited);
                self.timing.encode_ns = Some(render_cpu.as_nanos() as u64);
                self.timing.present_ns = Some(present);
                self.timing.submitted_ns = self.epoch.map(|epoch| {
                    (started - epoch).as_nanos() as u64 + total.saturating_sub(present)
                });
                self.timing.gpu_in_flight = gpu_in_flight;
                Some(Shown {
                    waited,
                    encoded: total.saturating_sub(waited).saturating_sub(present),
                    presented: present,
                })
            }
            Presented::Skipped { reason, waited } => {
                self.timing.outcome = Some(reason.into());
                self.timing.wait_ns = Some(waited.as_nanos() as u64);
                None
            }
        }
    }

    fn capture(&mut self, frame: &Frame) -> Option<beryl_render::Readback> {
        Some(self.renderer.read_back_frame(frame, &self.surface))
    }

    fn renderer(&mut self) -> Option<&mut Renderer> {
        Some(&mut self.renderer)
    }
}
