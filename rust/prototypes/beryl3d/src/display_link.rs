//! A display link on macOS: a timer the system fires once per display refresh, created with a
//! preferred frame rate range of the display's highest rate. Without one, macOS paced the
//! window's drawables at whatever rate it judged the window needed, as low as 15 Hz on a
//! 120 Hz ProMotion display, because winit's on-demand redraws and wgpu's FIFO presentation
//! never ask for more. Each tick sends a user event to the event loop, which draws a frame.
//! The link is paused whenever the core needs no frame, so a still table costs nothing.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObject, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::window::UserEvent;

/// What the display link's target holds: where to send each tick.
struct Ivars {
    proxy: EventLoopProxy<UserEvent>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Ticker` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "Beryl3dDisplayLinkTicker"]
    #[ivars = Ivars]
    struct Ticker;

    impl Ticker {
        #[unsafe(method(tick:))]
        fn tick(&self, _link: &CADisplayLink) {
            let _ = self.ivars().proxy.send_event(UserEvent::Tick);
        }
    }
);

impl Ticker {
    fn new(proxy: EventLoopProxy<UserEvent>, mtm: MainThreadMarker) -> Retained<Ticker> {
        let this = Self::alloc(mtm).set_ivars(Ivars { proxy });
        // SAFETY: NSObject's init on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

pub struct DisplayLink {
    link: Retained<CADisplayLink>,
    _ticker: Retained<Ticker>,
    paused: bool,
}

impl DisplayLink {
    /// A display link for `window`'s view, asking for `hz` frames a second. None where the
    /// window has no AppKit view or this is not the main thread.
    pub fn new(window: &Window, proxy: EventLoopProxy<UserEvent>, hz: f32) -> Option<DisplayLink> {
        let mtm = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        let view: NonNull<NSView> = handle.ns_view.cast();
        // SAFETY: winit's AppKit handle points at the window's live content view, which
        // outlives this call; it is only used on the main thread.
        let view: &NSView = unsafe { view.as_ref() };
        let ticker = Ticker::new(proxy, mtm);
        let target: &AnyObject = &ticker;
        // SAFETY: `tick:` is a method of `Ticker` that takes the display link.
        let link = unsafe { view.displayLinkWithTarget_selector(target, sel!(tick:)) };
        link.setPreferredFrameRateRange(CAFrameRateRange {
            minimum: hz * 0.75,
            maximum: hz,
            preferred: hz,
        });
        // SAFETY: the main run loop, in the common modes, so ticks continue during window
        // resizing and menu tracking.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        link.setPaused(true);
        Some(DisplayLink {
            link,
            _ticker: ticker,
            paused: true,
        })
    }

    /// Runs the link while frames are wanted and pauses it otherwise.
    pub fn set_running(&mut self, running: bool) {
        if self.paused == running {
            self.paused = !running;
            self.link.setPaused(!running);
        }
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}
