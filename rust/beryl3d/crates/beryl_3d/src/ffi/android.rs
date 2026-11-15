//! The JNI functions the Kotlin host calls, one for each C function. Kotlin's `object Native`
//! in `ca.thurn.beryl3d` declares them `external`. Java strings and the `Surface` cannot cross
//! as C types, so each is turned into its C form here: a string into UTF-8 and the `Surface`
//! into the `ANativeWindow` behind it.

use std::ffi::CStr;
use std::ptr::NonNull;

use beryl_render::Native;
use jni_sys::{JNIEnv, jfloat, jint, jlong, jobject, jstring};

use super::{BERYL_WAKE_AT, BERYL_WAKE_NOW, Beryl3d, BerylInsets};

/// An `ANativeWindow` the app holds a reference to, released when dropped.
pub(super) struct Window(NonNull<ndk_sys::ANativeWindow>);

impl Drop for Window {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: the window came from `ANativeWindow_fromSurface`, which took the reference
        // this releases, once.
        unsafe { ndk_sys::ANativeWindow_release(self.0.as_ptr()) };
    }
}

/// A Java string as Rust's.
///
/// # Safety
///
/// `env` must be the calling thread's JNI environment, and `string` a live `java.lang.String`.
#[allow(unsafe_code)]
unsafe fn string(env: *mut JNIEnv, string: jstring) -> String {
    // SAFETY: the JVM's function table is valid for the call; the characters are released
    // after they are copied.
    unsafe {
        let table = &**env;
        let (Some(get), Some(release)) = (table.GetStringUTFChars, table.ReleaseStringUTFChars)
        else {
            return String::new();
        };
        let chars = get(env, string, std::ptr::null_mut());
        if chars.is_null() {
            return String::new();
        }
        let text = CStr::from_ptr(chars).to_string_lossy().into_owned();
        release(env, string, chars);
        text
    }
}

/// `beryl3d_new`, with the fixture's name and the app's files folder. Returns the app as a
/// `long`, or 0.
///
/// # Safety
///
/// Called by the JVM with its environment and two live strings.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl3d_Native_create(
    env: *mut JNIEnv,
    _this: jobject,
    fixture: jstring,
    data_dir: jstring,
) -> jlong {
    // SAFETY: the JVM's call, as documented.
    let (fixture, data_dir) = unsafe { (string(env, fixture), string(env, data_dir)) };
    super::new(&fixture, &data_dir) as jlong
}

/// `beryl3d_surface`, with a `Surface` from the host's `SurfaceHolder`.
///
/// # Safety
///
/// Called by the JVM with its environment, an app from `create` and a live `Surface`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl3d_Native_surface(
    env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    surface: jobject,
    width: jint,
    height: jint,
    scale: jfloat,
) {
    // SAFETY: `app` came from `create`; `ANativeWindow_fromSurface` takes a reference to the
    // window behind a live `Surface`, which `Window` releases when the surface is lost.
    unsafe {
        let app = &mut *(app as *mut Beryl3d);
        let Some(window) = NonNull::new(ndk_sys::ANativeWindow_fromSurface(env, surface)) else {
            super::report("the Surface has no ANativeWindow");
            return;
        };
        // The window stays referenced while the renderer's surface uses it.
        let native = Native::AndroidWindow(window.cast());
        app.window = Some(Window(window));
        app.surface(
            native,
            width.max(1) as u32,
            height.max(1) as u32,
            scale,
            BerylInsets::default(),
        );
    }
}

/// `beryl3d_surface_lost`.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl3d_Native_surfaceLost(
    _env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
) {
    // SAFETY: `app` came from `create`.
    unsafe { super::beryl3d_surface_lost(app as *mut Beryl3d) };
}

/// `beryl3d_lifecycle`.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl3d_Native_lifecycle(
    _env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    event: jint,
) {
    // SAFETY: `app` came from `create`.
    unsafe { super::beryl3d_lifecycle(app as *mut Beryl3d, event as u32) };
}

/// `beryl3d_frame`, with `System.nanoTime()`. Returns when the next frame is wanted: 0 for now,
/// -1 for never, or that time in nanoseconds.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl3d_Native_frame(
    _env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    now: jlong,
) -> jlong {
    // SAFETY: `app` came from `create`.
    let wake = unsafe { super::beryl3d_frame(app as *mut Beryl3d, now as u64) };
    match wake.kind {
        BERYL_WAKE_NOW => 0,
        BERYL_WAKE_AT => wake.at as jlong,
        _ => -1,
    }
}
