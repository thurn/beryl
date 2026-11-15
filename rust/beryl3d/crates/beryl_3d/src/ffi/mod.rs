//! The C functions the phone hosts call (docs/beryl-3d.md, "Hosts"), declared for Swift in
//! `include/beryl3d.h` and wrapped for Kotlin by `android`'s JNI functions. A `Beryl3d` is the
//! whole app: the client, its feed and the phone host (`beryl_host::Phone`). Each function is
//! called on the platform's main thread, which owns the view and the display's ticks.
//!
//! These are the only items in the crate allowed `unsafe`: each trusts a pointer the host
//! passes, and says which and why.

#[cfg(target_os = "android")]
mod android;

use std::ffi::{CStr, c_char, c_void};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::OnceLock;

use beryl_host::Phone;
use beryl_render::Native;
use beryl_scene::frame::Wake;
use beryl_scene::input::{Insets, Lifecycle};
use beryl_scene::{Client, Config};

use crate::app::phone_feed;
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
    /// The Android window the host's surface gave, held until the surface is lost.
    #[cfg(target_os = "android")]
    window: Option<android::Window>,
}

/// What `beryl3d_new` starts.
#[repr(C)]
pub struct BerylConfig {
    /// The recording to replay, a UTF-8 C string: `<data_dir>/<fixture>.json` is read.
    pub fixture: *const c_char,
    /// The app's writable folder, a UTF-8 C string: recordings are read and frame logs written
    /// there.
    pub data_dir: *const c_char,
}

/// The parts of the surface that system bars or a notch cover, in points.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BerylInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// When the next frame is wanted: `kind` is one of the `BERYL_WAKE_` values, and `at` the
/// platform's monotonic time in nanoseconds for `BERYL_WAKE_AT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BerylWake {
    pub kind: u32,
    pub at: u64,
}

pub const BERYL_WAKE_NOW: u32 = 0;
pub const BERYL_WAKE_AT: u32 = 1;
pub const BERYL_WAKE_NEVER: u32 = 2;

pub const BERYL_SUSPENDED: u32 = 0;
pub const BERYL_RESUMED: u32 = 1;
pub const BERYL_LOW_MEMORY: u32 = 2;

impl Beryl3d {
    /// The app replaying the recording `fixture` from `data_dir`.
    fn new(fixture: &str, data_dir: PathBuf) -> Result<Self, String> {
        log_errors_in(&data_dir);
        let replay = Fixture::read(&data_dir, fixture)?;
        let feed = phone_feed(fixture, replay, data_dir);
        Ok(Self {
            phone: Phone::new(Client::new(Config::LARGEST), feed),
            #[cfg(target_os = "android")]
            window: None,
        })
    }

    /// The host's surface, which must stay valid until `surface_lost`.
    #[allow(unsafe_code)]
    fn surface(&mut self, native: Native, width: u32, height: u32, scale: f32, safe: BerylInsets) {
        let safe_area = Insets {
            top: safe.top,
            right: safe.right,
            bottom: safe.bottom,
            left: safe.left,
        };
        // SAFETY: each caller passes a surface the host keeps valid until `surface_lost`.
        let attached = unsafe { self.phone.surface(native, width, height, scale, safe_area) };
        if let Err(error) = attached {
            report(&error);
        }
    }

    fn frame(&mut self, now: u64) -> BerylWake {
        match self.phone.frame(now) {
            Wake::Now => BerylWake {
                kind: BERYL_WAKE_NOW,
                at: 0,
            },
            Wake::At(at) => BerylWake {
                kind: BERYL_WAKE_AT,
                at,
            },
            Wake::Never => BerylWake {
                kind: BERYL_WAKE_NEVER,
                at: 0,
            },
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
}

/// Starts the app, replaying `config.fixture`. Returns null, after printing why, when the
/// recording cannot be read.
///
/// # Safety
///
/// `config` must point to a `BerylConfig` whose strings are valid, NUL-terminated and UTF-8
/// for the length of the call.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn beryl3d_new(config: *const BerylConfig) -> *mut Beryl3d {
    // SAFETY: the host passes a config and strings that live for the call, as documented.
    let (fixture, data_dir) = unsafe {
        let config = &*config;
        (
            CStr::from_ptr(config.fixture)
                .to_string_lossy()
                .into_owned(),
            CStr::from_ptr(config.data_dir)
                .to_string_lossy()
                .into_owned(),
        )
    };
    new(&fixture, &data_dir)
}

/// The app replaying `fixture` from `data_dir`, as a pointer for the host, or null.
fn new(fixture: &str, data_dir: &str) -> *mut Beryl3d {
    match Beryl3d::new(fixture, PathBuf::from(data_dir)) {
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
