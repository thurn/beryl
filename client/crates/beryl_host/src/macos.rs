//! The macOS calls the desktop host makes that winit does not: a display link that paces frames
//! at the display's highest rate, and the time macOS stamped each mouse event. Each `unsafe`
//! block here is the one platform call that needs it.

use std::cell::Cell;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEvent, NSEventType, NSView};
use objc2_foundation::{NSObject, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CACurrentMediaTime, CADisplayLink, CAFrameRateRange};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::timing::{DisplayTick, PacerState, elapsed};
use crate::winit_host::UserEvent;

/// What the display link's target holds: where to send each tick.
struct Ivars {
    proxy: EventLoopProxy<UserEvent>,
    started: Instant,
    sequence: Cell<u64>,
    epoch: Cell<u64>,
    hz: Cell<f32>,
}

define_class!(
    // SAFETY: `define_class!` declares an Objective-C subclass of `NSObject`, which has no
    // subclassing requirements, and `Ticker` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BerylClientDisplayLinkTicker"]
    #[ivars = Ivars]
    struct Ticker;

    impl Ticker {
        // SAFETY: `tick:` takes the `CADisplayLink` that fired, as the link calls it.
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            // macOS can deliver a different cadence from the one the app asked for.
            let ivars = self.ivars();
            let callback_ns = elapsed(ivars.started);
            let media_now = CACurrentMediaTime();
            let display = link.timestamp();
            let target = link.targetTimestamp();
            let interval = target - display;
            let local = |media: f64| {
                (media.is_finite() && media > 0.0)
                    .then(|| callback_ns as i64 + ((media - media_now) * 1e9) as i64)
            };
            let sequence = ivars.sequence.get() + 1;
            ivars.sequence.set(sequence);
            let tick = DisplayTick {
                sequence,
                epoch: ivars.epoch.get(),
                requested_hz: ivars.hz.get(),
                callback_ns,
                display_ns: local(display),
                target_ns: local(target),
                reported_hz: (interval.is_finite() && interval > 0.0).then(|| interval.recip()),
            };
            // The event loop has ended if this fails, and then no frame is wanted.
            let _ = ivars.proxy.send_event(UserEvent::Tick(tick));
        }
    }
);

impl Ticker {
    #[expect(unsafe_code, reason = "NSObject's `init` through Objective-C")]
    fn new(
        proxy: EventLoopProxy<UserEvent>,
        started: Instant,
        hz: f32,
        mtm: MainThreadMarker,
    ) -> Retained<Ticker> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            proxy,
            started,
            sequence: Cell::new(0),
            epoch: Cell::new(0),
            hz: Cell::new(hz),
        });
        // SAFETY: `NSObject`'s `init` on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

/// A display link on the window's view: a timer macOS fires once per display refresh, asking
/// for the display's highest rate. Without one, winit's on-demand redraws and wgpu's FIFO
/// presentation never ask for more than macOS chooses, and the prototype's window ran at
/// 60 Hz on a 120 Hz display. Each tick sends `UserEvent::Tick` to the event loop.
pub struct DisplayLink {
    link: Retained<CADisplayLink>,
    ticker: Retained<Ticker>,
    running: bool,
}

impl DisplayLink {
    /// A paused display link for `window`'s view, asking for `hz` frames a second. `None` where
    /// the window has no `AppKit` view or this is not the main thread.
    #[expect(
        unsafe_code,
        reason = "NSView's displayLinkWithTarget:selector: and CADisplayLink's addToRunLoop:forMode:"
    )]
    pub fn new(
        window: &Window,
        proxy: EventLoopProxy<UserEvent>,
        hz: f32,
        started: Instant,
    ) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        let view: NonNull<NSView> = handle.ns_view.cast();
        // SAFETY: winit's AppKit handle points at the window's live content view, which
        // outlives this call, and it is used only on the main thread.
        let view: &NSView = unsafe { view.as_ref() };
        let ticker = Ticker::new(proxy, started, hz, mtm);
        let target: &AnyObject = &ticker;
        // SAFETY: NSView's `displayLinkWithTarget:selector:` (macOS 14); `tick:` is a method of
        // `Ticker` that takes the display link, and the link retains its target.
        let link = unsafe { view.displayLinkWithTarget_selector(target, sel!(tick:)) };
        link.setPreferredFrameRateRange(range(hz));
        // SAFETY: CADisplayLink's `addToRunLoop:forMode:` with the main run loop in the common
        // modes, so ticks continue while the window is resized or a menu is open.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        link.setPaused(true);
        Some(Self {
            link,
            ticker,
            running: false,
        })
    }

    /// Asks for `hz` frames a second from now on: a rate that divides the display's
    /// (`crate::rate`).
    pub fn set_rate(&mut self, hz: f32) {
        self.ticker.ivars().hz.set(hz);
        self.link.setPreferredFrameRateRange(range(hz));
    }

    /// Runs the link while frames are wanted and pauses it otherwise.
    pub fn set_running(&mut self, running: bool) {
        if self.running != running {
            self.running = running;
            if running {
                let epoch = &self.ticker.ivars().epoch;
                epoch.set(epoch.get() + 1);
            }
            self.link.setPaused(!running);
        }
    }
    pub fn state(&self, at_ns: u64) -> PacerState {
        PacerState {
            at_ns,
            epoch: self.ticker.ivars().epoch.get(),
            running: self.running,
            requested_hz: self.ticker.ivars().hz.get(),
        }
    }
}

/// The frame rate range that asks for exactly `hz`. No lower rate is acceptable, because the
/// rule in `crate::rate` chooses the rate and counts frames against it.
fn range(hz: f32) -> CAFrameRateRange {
    CAFrameRateRange {
        minimum: hz,
        maximum: hz,
        preferred: hz,
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}

/// Asks `AppKit` for every mouse event macOS makes. By default it merges the mouse moves and
/// drags waiting in the app's queue into the latest one, which carries the latest one's time
/// stamp. Then a run of events the app took late would look like one event the system sent
/// late, and `pointer_stamp` could not tell the two apart.
pub fn deliver_every_mouse_event() {
    NSEvent::setMouseCoalescingEnabled(false);
}

/// When macOS stamped the mouse event `AppKit` is dispatching, as an `Instant`, or `None`
/// outside a mouse event or off the main thread. winit turns the event into a window event while
/// `AppKit` dispatches it, so a host calls this from the window event to learn how long the event
/// took to reach the app.
pub fn pointer_stamp() -> Option<Instant> {
    let mtm = MainThreadMarker::new()?;
    let event = NSApplication::sharedApplication(mtm).currentEvent()?;
    let mouse = [
        NSEventType::MouseMoved,
        NSEventType::LeftMouseDown,
        NSEventType::LeftMouseUp,
        NSEventType::LeftMouseDragged,
        NSEventType::RightMouseDown,
        NSEventType::RightMouseUp,
        NSEventType::RightMouseDragged,
        NSEventType::OtherMouseDown,
        NSEventType::OtherMouseUp,
        NSEventType::OtherMouseDragged,
    ];
    if !mouse.contains(&event.r#type()) {
        return None;
    }
    // An event's time stamp and `CACurrentMediaTime` are both seconds since the machine
    // started, not counting sleep.
    let age = CACurrentMediaTime() - event.timestamp();
    let now = Instant::now();
    if age.is_finite() && age >= 0.0 {
        now.checked_sub(Duration::from_secs_f64(age))
    } else {
        Some(now)
    }
}
