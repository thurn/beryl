//! The C functions the phone hosts call (docs/beryl-3d.md, "Hosts"), declared for Swift in
//! `include/beryl3d.h` and wrapped for Kotlin by `android`'s JNI functions. A `Beryl3d` is the
//! whole app: the client, its feed and the phone host (`beryl_host::Phone`). Each function is
//! called on the platform's main thread, which owns the view and the display's ticks. Only the
//! wake callback the host gives `beryl3d_new` is called from other threads.
//!
//! These are the only items in the crate allowed `unsafe`: each trusts a pointer the host
//! passes, and says which and why.

#[cfg(target_os = "android")]
mod android;
mod types;

use std::ffi::{CStr, CString, c_char, c_void};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

use beryl_host::{Phone, Wakeup};
use beryl_render::Native;
use beryl_scene::frame::Wake;
use beryl_scene::input::Lifecycle;
use beryl_scene::{Client, Config};

pub use types::*;

use crate::app::{phone_feed, phone_live_feed};
use crate::fixture::Fixture;

/// The app's error log, `beryl3d.log` in its data folder: an Android app's standard error goes
/// nowhere, so errors and panics are written here too, for `just b3d android-smoke` to read.
static ERRORS: OnceLock<PathBuf> = OnceLock::new();

/// Prints `message` and adds it to the error log.
fn report(message: &str) {
    eprintln!("beryl3d: {message}");
    let Some(path) = ERRORS.get() else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}

/// Writes the warnings and errors of the libraries that report through the `log` crate (wgpu
/// among them) to `wgpu.log` in the app's data folder: a phone shows no standard error.
struct LibraryLog(PathBuf);

impl log::Log for LibraryLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.0)
        {
            let _ = writeln!(
                file,
                "{} {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Sends errors and panics to the error log in `data_dir`, and libraries' warnings to
/// `wgpu.log` beside it, once.
fn log_errors_in(data_dir: &Path) {
    if ERRORS.set(data_dir.join("beryl3d.log")).is_ok() {
        let logger = Box::new(LibraryLog(data_dir.join("wgpu.log")));
        if log::set_boxed_logger(logger).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            report(&format!("panic: {info}"));
            default(info);
        }));
    }
}

/// The app, as the hosts hold it: an opaque pointer.
pub struct Beryl3d {
    phone: Phone,
    /// The text of the last request `beryl3d_next_request` returned, which its struct points
    /// into until the next call.
    request_text: CString,
    /// Whether the last request `nextRequest` returned to the Kotlin host had a text.
    #[cfg(target_os = "android")]
    request_has_text: bool,
    /// The Android window the host's surface gave, held until the surface is lost.
    #[cfg(target_os = "android")]
    window: Option<android::Window>,
}

/// What a host passes `beryl3d_new`, as Rust's.
struct Start {
    /// The recording to replay, or `None` for a live game.
    fixture: Option<String>,
    data_dir: PathBuf,
    /// The agent that answers the person's decisions, in a debug build.
    stand_in: Option<String>,
    display_hz: f32,
    wake: Option<Wakeup>,
}

/// The host's wake callback and its context.
struct HostWake {
    wake: unsafe extern "C" fn(*mut c_void),
    context: *mut c_void,
}

// SAFETY: the header requires the callback to be callable with its context from any thread.
#[allow(unsafe_code)]
unsafe impl Send for HostWake {}
// SAFETY: as for `Send`; the app only ever calls the callback.
#[allow(unsafe_code)]
unsafe impl Sync for HostWake {}

impl HostWake {
    #[allow(unsafe_code)]
    fn call(&self) {
        // SAFETY: the host gave this callback and context to be called from any thread.
        unsafe { (self.wake)(self.context) };
    }
}

impl Beryl3d {
    /// The app `start` describes: replaying a recording from its data folder, or playing a
    /// live game whose replays and ledger go there.
    fn new(start: Start) -> Result<Self, String> {
        let Start {
            fixture,
            data_dir,
            stand_in,
            display_hz,
            wake,
        } = start;
        log_errors_in(&data_dir);
        // An agent stands in for the person only in a debug build.
        #[cfg(not(debug_assertions))]
        let stand_in: Option<String> = stand_in.and(None);
        let client = Client::new(Config::LARGEST);
        let phone = match fixture {
            Some(name) => {
                let replay = Fixture::read(&data_dir, &name)?;
                let feed = phone_feed(&name, replay, data_dir, display_hz);
                Phone::new(client, display_hz, wake, |_| feed)
            }
            None => Phone::new(client, display_hz, wake, |inbox| {
                phone_live_feed(data_dir, stand_in, display_hz, inbox)
            }),
        };
        Ok(Self {
            phone,
            request_text: CString::default(),
            #[cfg(target_os = "android")]
            request_has_text: false,
            #[cfg(target_os = "android")]
            window: None,
        })
    }

    /// The host's surface, which must stay valid until `surface_lost`.
    #[allow(unsafe_code)]
    fn surface(&mut self, native: Native, width: u32, height: u32, scale: f32, safe: BerylInsets) {
        let (scale, safe_area) = surface_from_c(scale, safe);
        // SAFETY: each caller passes a surface the host keeps valid until `surface_lost`.
        let attached = unsafe { self.phone.surface(native, width, height, scale, safe_area) };
        if let Err(error) = attached {
            report(&error);
        }
    }

    fn frame(&mut self, now: u64) -> BerylWake {
        let (kind, at) = match self.phone.frame(now) {
            Wake::Now => (BERYL_WAKE_NOW, 0),
            Wake::At(at) => (BERYL_WAKE_AT, at),
            Wake::Never => (BERYL_WAKE_NEVER, 0),
        };
        BerylWake {
            kind,
            at,
            hz: self.phone.hz(),
        }
    }

    fn lifecycle(&mut self, event: u32) {
        let lifecycle = match event {
            BERYL_SUSPENDED => Lifecycle::Suspended,
            BERYL_RESUMED => Lifecycle::Resumed,
            BERYL_LOW_MEMORY => Lifecycle::LowMemory,
            _ => return,
        };
        self.phone.lifecycle(lifecycle);
    }

    /// The next request the platform carries out, skipping those the app keeps.
    fn next_request(&mut self) -> Option<BerylRequest> {
        while let Some(request) = self.phone.next_request() {
            if let Some(out) = request_to_c(&request, &mut self.request_text) {
                return Some(out);
            }
        }
        None
    }
}

/// Starts the app: replaying `config.fixture`, or a live game when it is NULL. Returns null,
/// after printing why, when the recording cannot be read.
///
/// # Safety
///
/// `config` must point to a `BerylConfig` whose strings are NULL (where allowed) or valid,
/// NUL-terminated and UTF-8 for the length of the call, and whose `wake`, if not NULL, may be
/// called with `context` from any thread for as long as the app lives.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_new(config: *const BerylConfig) -> *mut Beryl3d {
    // SAFETY: the host passes a config and strings that live for the call, as documented.
    let (config, fixture, data_dir, stand_in) = unsafe {
        let config = &*config;
        (
            config,
            optional(config.fixture),
            optional(config.data_dir).unwrap_or_default(),
            optional(config.stand_in),
        )
    };
    let wake = config.wake.map(|wake| {
        let host = HostWake {
            wake,
            context: config.context,
        };
        Arc::new(move || host.call()) as Wakeup
    });
    new(Start {
        fixture,
        data_dir: PathBuf::from(data_dir),
        stand_in,
        display_hz: config.display_hz,
        wake,
    })
}

/// A C string as Rust's, or `None` for NULL.
///
/// # Safety
///
/// `string` must be NULL or valid and NUL-terminated for the length of the call.
#[allow(unsafe_code)]
unsafe fn optional(string: *const c_char) -> Option<String> {
    // SAFETY: the caller's promise.
    (!string.is_null()).then(|| {
        unsafe { CStr::from_ptr(string) }
            .to_string_lossy()
            .into_owned()
    })
}

/// The app `start` describes, as a pointer for the host, or null.
fn new(start: Start) -> *mut Beryl3d {
    match Beryl3d::new(start) {
        Ok(app) => Box::into_raw(Box::new(app)),
        Err(error) => {
            report(&error);
            std::ptr::null_mut()
        }
    }
}

/// The host has a surface to draw in: on iOS a `CAMetalLayer`, `width` by `height` physical
/// pixels at `scale` pixels per point.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`. `native_surface` must be a live `CAMetalLayer` that the
/// host keeps until it calls `beryl3d_surface_lost`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_surface(
    app: *mut Beryl3d,
    native_surface: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
    safe_area: BerylInsets,
) {
    let Some(layer) = NonNull::new(native_surface) else {
        return;
    };
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    let app = unsafe { &mut *app };
    app.surface(Native::MetalLayer(layer), width, height, scale, safe_area);
}

/// The host lost its surface, or may no longer draw in it.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_surface_lost(app: *mut Beryl3d) {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    let app = unsafe { &mut *app };
    app.phone.surface_lost();
    #[cfg(target_os = "android")]
    {
        app.window = None;
    }
}

/// The app was suspended (`BERYL_SUSPENDED`), resumed (`BERYL_RESUMED`), or memory is low
/// (`BERYL_LOW_MEMORY`). Any other value is ignored.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_lifecycle(app: *mut Beryl3d, event: u32) {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    unsafe { &mut *app }.lifecycle(event);
}

/// The display is ready for a frame at `now_nanoseconds`, the platform's monotonic time. Draws
/// one if one is wanted, and says when the next is.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_frame(app: *mut Beryl3d, now_nanoseconds: u64) -> BerylWake {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    unsafe { &mut *app }.frame(now_nanoseconds)
}

/// A mouse, touch or pen event. One whose id is over 255 or whose kind, phase, button or
/// modifier bits name nothing is dropped, as is any before the first surface.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_pointer(app: *mut Beryl3d, event: BerylPointer) {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    let app = unsafe { &mut *app };
    if let Some(pointer) = pointer_from_c(event) {
        app.phone.pointer(pointer);
    }
}

/// A key event. One whose code, phase or modifier bits name nothing is dropped, as is any
/// before the first surface.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_key(app: *mut Beryl3d, event: BerylKey) {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    let app = unsafe { &mut *app };
    if let Some(key) = key_from_c(event) {
        app.phone.key(key);
    }
}

/// The edited field's whole new contents, UTF-8. NULL is ignored.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`, and `utf8` be NULL or valid and NUL-terminated for the
/// length of the call.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_text(app: *mut Beryl3d, utf8: *const c_char) {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread, and `utf8`
    // the host's string, as documented.
    let (app, text) = unsafe { (&mut *app, optional(utf8)) };
    if let Some(text) = text {
        app.phone.text(text);
    }
}

/// Writes the next thing the client asks of the platform into `out` and returns true, or
/// returns false when there is none. The request's text stays valid until the next call of
/// this function or of `beryl3d_frame`.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`, and `out` point to a writable `BerylRequest`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_next_request(app: *mut Beryl3d, out: *mut BerylRequest) -> bool {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    let app = unsafe { &mut *app };
    let Some(request) = app.next_request() else {
        return false;
    };
    // SAFETY: the host passes a writable request, as documented.
    unsafe { out.write(request) };
    true
}

/// Draws into an offscreen target of `width` by `height` pixels in place of a native surface,
/// for the test of the header (`tests/c/header.c`), which has no view to give. Declared in
/// `tests/c/offscreen.h`, not in the hosts' header, and left out of phone and release builds.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`.
#[cfg(feature = "tools")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_offscreen(app: *mut Beryl3d, width: u32, height: u32) {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, used on one thread.
    let app = unsafe { &mut *app };
    if let Err(error) = app.phone.offscreen(width, height) {
        report(&error);
    }
}

/// Ends the app. Its frame log is finished and written before this returns, so that the test
/// of the header can read it. Declared in `tests/c/offscreen.h`: the phone hosts never end
/// their app.
///
/// # Safety
///
/// `app` must come from `beryl3d_new`, and is not used again.
#[cfg(feature = "tools")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_close(app: *mut Beryl3d) {
    // SAFETY: `app` is the host's pointer from `beryl3d_new`, given up here.
    drop(unsafe { Box::from_raw(app) });
}
