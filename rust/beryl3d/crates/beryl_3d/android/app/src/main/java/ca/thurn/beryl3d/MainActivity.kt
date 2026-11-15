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

    /** Starts the app replaying `fixture` from `dataDir`; 0 when it cannot be read. */
    external fun create(fixture: String, dataDir: String): Long
    external fun surface(app: Long, surface: Surface, width: Int, height: Int, scale: Float)
    external fun surfaceLost(app: Long)
    external fun lifecycle(app: Long, event: Int)
    /** When the next frame is wanted: 0 for now, -1 for never, else that System.nanoTime(). */
    external fun frame(app: Long, now: Long): Long

    const val SUSPENDED = 0
    const val RESUMED = 1
    const val LOW_MEMORY = 2
}

class MainActivity : Activity(), SurfaceHolder.Callback, Choreographer.FrameCallback {
    private var app = 0L
    private var ticking = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The recording to replay: `just b3d android-smoke` names it.
        val fixture = intent.getStringExtra("fixture") ?: "verbs-zones"
        app = Native.create(fixture, filesDir.path)
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
