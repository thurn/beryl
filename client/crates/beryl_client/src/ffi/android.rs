//! The JNI functions the Kotlin host calls, one for each C function. Kotlin's `object Native`
//! in `ca.thurn.beryl` declares them `external`. Java strings and the `Surface` cannot cross
//! as C types, so each is turned into its C form here: a string into UTF-8 and the `Surface`
//! into the `ANativeWindow` behind it.

use std::cell::RefCell;
use std::ffi::{CStr, c_void};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;

use beryl_host::Wakeup;
use beryl_render::Native;
use jni_sys::{
    JNI_OK, JNI_VERSION_1_6, JNIEnv, JavaVM, jfloat, jfloatArray, jint, jlong, jlongArray,
    jmethodID, jobject, jsize, jstring,
};

use super::{
    BERYL_REQUEST_HAPTIC, BERYL_WAKE_AT, BERYL_WAKE_NOW, BerylClient, BerylInsets, BerylKey,
    BerylPointer, Start,
};

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

/// A Java string as Rust's, or `None` for null.
///
/// # Safety
///
/// As for [`string`], except that `string` may be null.
#[allow(unsafe_code)]
unsafe fn optional(env: *mut JNIEnv, string: jstring) -> Option<String> {
    // SAFETY: the caller's promise.
    (!string.is_null()).then(|| unsafe { self::string(env, string) })
}

/// A Java string as Rust's, read as UTF-16: Java's modified UTF-8 would garble an emoji typed
/// into a text field.
///
/// # Safety
///
/// `env` must be the calling thread's JNI environment, and `string` a live `java.lang.String`.
#[allow(unsafe_code)]
unsafe fn utf16(env: *mut JNIEnv, string: jstring) -> String {
    // SAFETY: the JVM's function table is valid for the call; the characters are released
    // after they are copied.
    unsafe {
        let table = &**env;
        let (Some(length), Some(get), Some(release)) = (
            table.GetStringLength,
            table.GetStringChars,
            table.ReleaseStringChars,
        ) else {
            return String::new();
        };
        let units = usize::try_from(length(env, string)).unwrap_or(0);
        let chars = get(env, string, std::ptr::null_mut());
        if chars.is_null() {
            return String::new();
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(chars, units));
        release(env, string, chars);
        text
    }
}

/// The Kotlin host's waker: the JVM, a global reference to the object and its `void wake()`.
struct Waker {
    vm: *mut JavaVM,
    object: jobject,
    method: jmethodID,
}

// SAFETY: a `JavaVM`, a global reference and a method id are valid on every thread; each call
// attaches its thread first.
#[allow(unsafe_code)]
unsafe impl Send for Waker {}
// SAFETY: as for `Send`; the waker is only called.
#[allow(unsafe_code)]
unsafe impl Sync for Waker {}

impl Waker {
    /// Holds `waker` for calls from any thread.
    ///
    /// # Safety
    ///
    /// `env` must be the calling thread's JNI environment, and `waker` a live object with a
    /// `void wake()` method.
    #[allow(unsafe_code)]
    unsafe fn new(env: *mut JNIEnv, waker: jobject) -> Option<Self> {
        // SAFETY: the JVM's function table is valid for the call.
        unsafe {
            let table = &**env;
            let mut vm: *mut JavaVM = std::ptr::null_mut();
            if waker.is_null() || (table.GetJavaVM?)(env, &raw mut vm) != 0 {
                return None;
            }
            let class = (table.GetObjectClass?)(env, waker);
            let method = (table.GetMethodID?)(env, class, c"wake".as_ptr(), c"()V".as_ptr());
            (table.DeleteLocalRef?)(env, class);
            if method.is_null() {
                (table.ExceptionClear?)(env);
                return None;
            }
            let object = (table.NewGlobalRef?)(env, waker);
            Some(Self { vm, object, method })
        }
    }

    /// Calls the waker's `wake()` on this thread. A thread the JVM does not know yet (the
    /// session's, a socket connection's) is attached as a daemon once, and detached when it
    /// exits: Android's runtime aborts the whole app when a thread it attached exits without
    /// detaching. Later calls on the same thread reuse the attachment, so no call leaks one.
    #[allow(unsafe_code)]
    fn wake(&self) {
        // SAFETY: the JVM, the global reference and the method id stay valid while the app
        // lives, which is the process's life; the thread is attached before the call.
        unsafe {
            let Some(env) = self.env() else {
                return;
            };
            let table = &**env;
            if let Some(call) = table.CallVoidMethodA {
                call(env, self.object, self.method, std::ptr::null());
            }
            if let (Some(check), Some(clear)) = (table.ExceptionCheck, table.ExceptionClear)
                && check(env) != 0
            {
                clear(env);
            }
        }
    }

    /// This thread's JNI environment, attaching the thread first if the JVM does not know it.
    #[allow(unsafe_code)]
    fn env(&self) -> Option<*mut JNIEnv> {
        // SAFETY: `self.vm` is the process's JVM, valid on every thread.
        unsafe {
            let invoke = &**self.vm;
            let mut env: *mut c_void = std::ptr::null_mut();
            let get = invoke.GetEnv?;
            if get(self.vm, &raw mut env, JNI_VERSION_1_6) == JNI_OK {
                return Some(env.cast());
            }
            let attach = invoke.AttachCurrentThreadAsDaemon?;
            if attach(self.vm, &raw mut env, std::ptr::null_mut()) != 0 {
                return None;
            }
            ATTACHED.with(|attached| *attached.borrow_mut() = Some(Detach(self.vm)));
            Some(env.cast())
        }
    }
}

thread_local! {
    /// Set on a thread that `Waker::env` attached to the JVM, so that the thread detaches when
    /// it exits. Rust's thread-local destructors run before Android's runtime checks, at thread
    /// exit, that the thread detached.
    static ATTACHED: RefCell<Option<Detach>> = const { RefCell::new(None) };
}

/// Detaches the current thread from a JVM when dropped.
struct Detach(*mut JavaVM);

impl Drop for Detach {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: the JVM lives as long as the process, and this thread attached itself to it
        // in `Waker::env` and is exiting.
        unsafe {
            if let Some(detach) = (**self.0).DetachCurrentThread {
                detach(self.0);
            }
        }
    }
}

/// `beryl_client_new`, with the recording's name or null for a live game, the app's files folder,
/// the stand-in agent or null, the display's highest rate and an object whose `void wake()`
/// asks for a frame soon. Returns the app as a `long`, or 0.
///
/// # Safety
///
/// Called by the JVM with its environment, live strings (the fixture and stand-in may be
/// null) and a live waker.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_create(
    env: *mut JNIEnv,
    _this: jobject,
    fixture: jstring,
    data_dir: jstring,
    stand_in: jstring,
    stand_in_deck: jstring,
    stand_in_opponent: jstring,
    stand_in_seed: jlong,
    display_hz: jfloat,
    waker: jobject,
) -> jlong {
    // SAFETY: the JVM's call, as documented.
    let (fixture, data_dir, stand_in, stand_in_deck, stand_in_opponent, waker) = unsafe {
        (
            optional(env, fixture),
            string(env, data_dir),
            optional(env, stand_in),
            optional(env, stand_in_deck),
            optional(env, stand_in_opponent),
            Waker::new(env, waker),
        )
    };
    let wake = waker.map(|waker| Arc::new(move || waker.wake()) as Wakeup);
    let stand_in = stand_in.map(|agent| crate::app::StandIn {
        agent,
        deck: stand_in_deck,
        opponent: stand_in_opponent,
        seed: (stand_in_seed != 0).then_some(stand_in_seed as u64),
    });
    super::new(Start {
        fixture,
        data_dir: PathBuf::from(data_dir),
        stand_in,
        display_hz,
        wake,
    }) as jlong
}

/// `beryl_client_surface`, with a `Surface` from the host's `SurfaceHolder`.
///
/// # Safety
///
/// Called by the JVM with its environment, an app from `create` and a live `Surface`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_surface(
    env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    surface: jobject,
    width: jint,
    height: jint,
    scale: jfloat,
    top: jfloat,
    right: jfloat,
    bottom: jfloat,
    left: jfloat,
) {
    // SAFETY: `app` came from `create`; `ANativeWindow_fromSurface` takes a reference to the
    // window behind a live `Surface`, which `Window` releases when the surface is lost.
    unsafe {
        let app = &mut *(app as *mut BerylClient);
        let Some(window) = NonNull::new(ndk_sys::ANativeWindow_fromSurface(env, surface)) else {
            super::report("the Surface has no ANativeWindow");
            return;
        };
        // The window stays referenced while the renderer's surface uses it.
        let native = Native::AndroidWindow(window.cast());
        // Hold the old reference through the resize, in case the renderer still uses it.
        let _previous = app.window.replace(Window(window));
        app.surface(
            native,
            width.max(1) as u32,
            height.max(1) as u32,
            scale,
            BerylInsets {
                top,
                right,
                bottom,
                left,
            },
        );
    }
}

/// `beryl_client_surface_lost`.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_surfaceLost(
    _env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
) {
    // SAFETY: `app` came from `create`.
    unsafe { super::beryl_client_surface_lost(app as *mut BerylClient) };
}

/// `beryl_client_lifecycle`.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_lifecycle(
    _env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    event: jint,
) {
    // SAFETY: `app` came from `create`.
    unsafe { super::beryl_client_lifecycle(app as *mut BerylClient, event as u32) };
}

/// `beryl_client_frame`, with `System.nanoTime()`. Returns [wake, hz bits], where wake is 0 for now,
/// -1 for never, or the requested time in nanoseconds.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_frame(
    env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    now: jlong,
) -> jlongArray {
    // SAFETY: the JVM supplies its environment and `app` came from `create`.
    unsafe {
        let wake = super::beryl_client_frame(app as *mut BerylClient, now as u64);
        let at = match wake.kind {
            BERYL_WAKE_NOW => 0,
            BERYL_WAKE_AT => wake.at as jlong,
            _ => -1,
        };
        let table = &**env;
        let (Some(new), Some(set)) = (table.NewLongArray, table.SetLongArrayRegion) else {
            return std::ptr::null_mut();
        };
        let result = new(env, 2);
        if !result.is_null() {
            let values = [at, jlong::from(wake.hz.to_bits())];
            set(env, result, 0, 2, values.as_ptr());
        }
        result
    }
}

/// `beryl_client_pointer`.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_pointer(
    _env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    id: jint,
    kind: jint,
    phase: jint,
    x: jfloat,
    y: jfloat,
    button: jint,
    wheel_x: jfloat,
    wheel_y: jfloat,
    modifiers: jint,
) {
    let event = BerylPointer {
        id: id as u32,
        kind: kind as u32,
        phase: phase as u32,
        x,
        y,
        button: button as u32,
        wheel_x,
        wheel_y,
        modifiers: modifiers as u32,
    };
    // SAFETY: `app` came from `create`.
    unsafe { super::beryl_client_pointer(app as *mut BerylClient, event) };
}

/// `beryl_client_key`.
///
/// # Safety
///
/// `app` must come from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_key(
    _env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    code: jint,
    phase: jint,
    modifiers: jint,
) {
    let event = BerylKey {
        code: code as u32,
        phase: phase as u32,
        modifiers: modifiers as u32,
    };
    // SAFETY: `app` came from `create`.
    unsafe { super::beryl_client_key(app as *mut BerylClient, event) };
}

/// `beryl_client_text`, with the field's new contents.
///
/// # Safety
///
/// Called by the JVM with its environment, an app from `create` and a live string.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_text(
    env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    text: jstring,
) {
    // SAFETY: the JVM's call, as documented.
    unsafe {
        let text = utf16(env, text);
        (*(app as *mut BerylClient)).surface_host.text(text);
    }
}

/// `beryl_client_next_request`. Returns the request's kind, or -1 for none, and writes its
/// rectangle into `rect` (x, y, width, height), or for a haptic its strength into `rect[0]`.
/// `requestText` then gives its text.
///
/// # Safety
///
/// Called by the JVM with its environment, an app from `create` and a live array of at least
/// four floats.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_nextRequest(
    env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
    rect: jfloatArray,
) -> jint {
    // SAFETY: the JVM's call, as documented.
    unsafe {
        let app = &mut *(app as *mut BerylClient);
        let request = app.next_request();
        app.request_has_text = request.is_some_and(|request| !request.text.is_null());
        let Some(request) = request else {
            return -1;
        };
        let values: [jfloat; 4] = if request.kind == BERYL_REQUEST_HAPTIC {
            [request.haptic as jfloat, 0.0, 0.0, 0.0]
        } else {
            [request.x, request.y, request.width, request.height]
        };
        if let Some(set) = (**env).SetFloatArrayRegion {
            set(env, rect, 0, values.len() as jsize, values.as_ptr());
        }
        request.kind as jint
    }
}

/// The text of the last request `nextRequest` returned, or null when it had none.
///
/// # Safety
///
/// Called by the JVM with its environment and an app from `create`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_ca_thurn_beryl_Native_requestText(
    env: *mut JNIEnv,
    _this: jobject,
    app: jlong,
) -> jstring {
    // SAFETY: the JVM's call, as documented.
    unsafe {
        let app = &*(app as *mut BerylClient);
        if !app.request_has_text {
            return std::ptr::null_mut();
        }
        let units: Vec<u16> = app.request_text.to_string_lossy().encode_utf16().collect();
        match (**env).NewString {
            Some(new) => new(env, units.as_ptr(), units.len() as jsize),
            None => std::ptr::null_mut(),
        }
    }
}
