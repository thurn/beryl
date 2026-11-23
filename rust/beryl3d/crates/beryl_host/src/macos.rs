//! The macOS call the desktop host makes that winit does not: a display link that paces frames
//! at the display's highest rate. Each `unsafe` block here is the one platform call that needs
//! it.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObject, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::winit_host::UserEvent;

/// What the display link's target holds: where to send each tick.
struct Ivars {
    proxy: EventLoopProxy<UserEvent>,
}

define_class!(
    // SAFETY: `define_class!` declares an Objective-C subclass of `NSObject`, which has no
    // subclassing requirements, and `Ticker` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "Beryl3dDisplayLinkTicker"]
    #[ivars = Ivars]
    struct Ticker;

    impl Ticker {
        // SAFETY: `tick:` takes the `CADisplayLink` that fired, as the link calls it.
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            // macOS can deliver a different cadence from the one the app asked for.
            let interval = link.targetTimestamp() - link.timestamp();
            let cadence = (interval.is_finite() && interval > 0.0).then(|| interval.recip());
            // The event loop has ended if this fails, and then no frame is wanted.
            let _ = self.ivars().proxy.send_event(UserEvent::Tick(cadence));
        }
    }
);

impl Ticker {
    #[expect(unsafe_code, reason = "NSObject's `init` through Objective-C")]
    fn new(proxy: EventLoopProxy<UserEvent>, mtm: MainThreadMarker) -> Retained<Ticker> {
        let this = Self::alloc(mtm).set_ivars(Ivars { proxy });
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
    _ticker: Retained<Ticker>,
    running: bool,
}

impl DisplayLink {
    /// A paused display link for `window`'s view, asking for `hz` frames a second. `None` where
    /// the window has no `AppKit` view or this is not the main thread.
    #[expect(
        unsafe_code,
        reason = "NSView's displayLinkWithTarget:selector: and CADisplayLink's addToRunLoop:forMode:"
    )]
    pub fn new(window: &Window, proxy: EventLoopProxy<UserEvent>, hz: f32) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        let view: NonNull<NSView> = handle.ns_view.cast();
        // SAFETY: winit's AppKit handle points at the window's live content view, which
        // outlives this call, and it is used only on the main thread.
        let view: &NSView = unsafe { view.as_ref() };
        let ticker = Ticker::new(proxy, mtm);
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
            _ticker: ticker,
            running: false,
        })
    }

    /// Asks for `hz` frames a second from now on: a rate that divides the display's
    /// (`crate::rate`).
    pub fn set_rate(&mut self, hz: f32) {
        self.link.setPreferredFrameRateRange(range(hz));
    }

    /// Runs the link while frames are wanted and pauses it otherwise.
    pub fn set_running(&mut self, running: bool) {
        if self.running != running {
            self.running = running;
            self.link.setPaused(!running);
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
