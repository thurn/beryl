// The Android host of Beryl 3D's phone spike (docs/beryl-3d.md, "Hosts"): one SurfaceView, and
// a Choreographer frame callback that asks the Rust library for a frame on every tick. It
// passes the surface, the app's lifecycle and the display's time, and nothing else: everything
// the app does is in the library's C functions, which `Native` reaches through JNI.

package ca.thurn.beryl3d

import android.app.Activity
import android.os.Bundle
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView

/** The library's JNI functions (src/ffi/android.rs in beryl_3d). */
object Native {
    init {
        System.loadLibrary("beryl_3d")
    }

    /**
     * Starts the app replaying `fixture` from `dataDir`, or a live game when it is null; 0
     * when the recording cannot be read. `standIn` names an agent that plays for the person
     * (debug builds only), `displayHz` is the display's highest rate, and `waker.wake()` is
     * called from any thread when the app wants a frame soon.
     */
    external fun create(
        fixture: String?,
        dataDir: String,
        standIn: String?,
        displayHz: Float,
        waker: Any,
    ): Long
    external fun surface(app: Long, surface: Surface, width: Int, height: Int, scale: Float)
    external fun surfaceLost(app: Long)
    external fun pointer(
        app: Long,
        id: Int,
        kind: Int,
        phase: Int,
        x: Float,
        y: Float,
        button: Int,
        wheelX: Float,
        wheelY: Float,
        modifiers: Int,
    )
    external fun key(app: Long, code: Int, phase: Int, modifiers: Int)
    external fun text(app: Long, text: String)
    external fun lifecycle(app: Long, event: Int)
    /** When the next frame is wanted: 0 for now, -1 for never, else that System.nanoTime(). */
    external fun frame(app: Long, now: Long): Long
    /**
     * The next request's kind, or -1 for none. Writes its rectangle into `rect` (x, y,
     * width, height), or for a haptic its strength into `rect[0]`.
     */
    external fun nextRequest(app: Long, rect: FloatArray): Int
    /** The text of the last request `nextRequest` returned, or null. */
    external fun requestText(app: Long): String?

    const val SUSPENDED = 0
    const val RESUMED = 1
    const val LOW_MEMORY = 2
}

/** What the library calls when it wants a frame soon. The spike ticks on every frame anyway. */
class Waker {
    fun wake() {}
}

class MainActivity : Activity(), SurfaceHolder.Callback, Choreographer.FrameCallback {
    private var app = 0L
    private var ticking = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The recording to replay: `just b3d android-smoke` names it.
        val fixture = intent.getStringExtra("fixture") ?: "verbs-zones"
        @Suppress("DEPRECATION") // Activity.display is API 30; this is API 24's call.
        val modes = windowManager.defaultDisplay.supportedModes
        val displayHz = modes.maxOfOrNull { it.refreshRate } ?: 60f
        app = Native.create(fixture, filesDir.path, null, displayHz, Waker())
        val view = SurfaceView(this)
        view.holder.addCallback(this)
        setContentView(view)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {}

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (app == 0L) return
        Native.surface(app, holder.surface, width, height, resources.displayMetrics.density)
        if (!ticking) {
            ticking = true
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        if (app == 0L) return
        ticking = false
        Choreographer.getInstance().removeFrameCallback(this)
        Native.surfaceLost(app)
    }

    override fun doFrame(frameTimeNanos: Long) {
        if (!ticking) return
        Native.frame(app, System.nanoTime())
        Choreographer.getInstance().postFrameCallback(this)
    }

    override fun onPause() {
        super.onPause()
        if (app != 0L) Native.lifecycle(app, Native.SUSPENDED)
    }

    override fun onResume() {
        super.onResume()
        if (app != 0L) Native.lifecycle(app, Native.RESUMED)
    }

    @Deprecated("Activity.onLowMemory is the call API 24 has")
    override fun onLowMemory() {
        super.onLowMemory()
        if (app != 0L) Native.lifecycle(app, Native.LOW_MEMORY)
    }
}
