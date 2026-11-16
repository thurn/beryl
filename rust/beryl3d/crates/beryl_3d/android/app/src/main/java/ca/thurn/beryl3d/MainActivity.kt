// The Android host passes Android events to the Rust client and schedules its next frame.

package ca.thurn.beryl3d

import android.app.Activity
import android.content.res.Configuration
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceHolder
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.window.OnBackInvokedDispatcher

/** The Rust library can call wake from any thread. Pending inputs share one main-thread wake. */
class Waker(private val requestFrame: Runnable) {
    private val handler = Handler(Looper.getMainLooper())
    private var posted = false

    @Synchronized
    fun wake() {
        if (posted) return
        posted = true
        handler.post {
            synchronized(this) { posted = false }
            requestFrame.run()
        }
    }
}

class MainActivity : Activity(), SurfaceHolder.Callback, Choreographer.FrameCallback {
    private var app = 0L
    private var currentSurface: SurfaceHolder? = null
    private var width = 0
    private var height = 0
    private var insets = SafeInsets()
    private var lastRate = 0f
    private var framePending = false
    private var frameDelayed = false
    private var traceFrames = false
    private var callbackCount = 0L
    private val rect = FloatArray(4)
    private val choreographer by lazy { Choreographer.getInstance() }

    @Suppress("DEPRECATION") // Keep the window and display calls for API 24.
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val fixture = intent.getStringExtra("fixture")
        val standIn = if (BuildConfig.DEBUG) intent.getStringExtra("standIn") else null
        traceFrames = BuildConfig.DEBUG && intent.getBooleanExtra("traceFrames", false)
        val modes = windowManager.defaultDisplay.supportedModes
        val displayHz = modes.maxOfOrNull { it.refreshRate } ?: 60f
        app = Native.create(fixture, filesDir.path, standIn, displayHz, Waker(::requestFrame))

        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
        } else {
            window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
        }
        if (Build.VERSION.SDK_INT >= 28) {
            val attributes = window.attributes
            attributes.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            window.attributes = attributes
        }
        if (Build.VERSION.SDK_INT >= 33) {
            onBackInvokedDispatcher.registerOnBackInvokedCallback(
                OnBackInvokedDispatcher.PRIORITY_DEFAULT, ::sendBack,
            )
        }

        val view = Touches(this, ::sendTouch)
        view.holder.addCallback(this)
        view.setOnApplyWindowInsetsListener { _, windowInsets ->
            val next = SafeInsets.from(windowInsets)
            if (next != insets) {
                insets = next
                updateSurface()
            }
            windowInsets
        }
        setContentView(view)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        updateSurface()
    }

    override fun surfaceCreated(holder: SurfaceHolder) {}

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        currentSurface = holder
        this.width = width
        this.height = height
        updateSurface()
    }

    private fun updateSurface() {
        val holder = currentSurface ?: return
        if (app == 0L) return
        val scale = resources.displayMetrics.density
        // A preceding idle callback no longer describes this surface.
        if (traceFrames) Log.d("Beryl3dFrames", "$callbackCount 0")
        Native.surface(
            app, holder.surface, width, height, scale,
            insets.top / scale, insets.right / scale, insets.bottom / scale, insets.left / scale,
        )
        requestFrame()
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        currentSurface = null
        lastRate = 0f
        if (framePending) choreographer.removeFrameCallback(this)
        framePending = false
        frameDelayed = false
        if (app != 0L) Native.surfaceLost(app)
    }

    private fun requestFrame() {
        if (app == 0L || currentSurface == null || (framePending && !frameDelayed)) return
        if (framePending) choreographer.removeFrameCallback(this)
        framePending = true
        frameDelayed = false
        choreographer.postFrameCallback(this)
    }

    override fun doFrame(frameTimeNanos: Long) {
        framePending = false
        frameDelayed = false
        val holder = currentSurface ?: return
        val result = Native.frame(app, System.nanoTime())
        val hz = Float.fromBits(result[1].toInt())
        if (Build.VERSION.SDK_INT >= 30 && hz != lastRate) {
            holder.surface.setFrameRate(hz, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE)
            lastRate = hz
        }

        while (true) {
            val kind = Native.nextRequest(app, rect)
            if (kind == -1) break
            perform(kind, rect, Native.requestText(app))
        }

        val wake = result[0]
        if (traceFrames) Log.d("Beryl3dFrames", "${++callbackCount} $wake")
        when (wake) {
            -1L -> Unit
            0L -> requestFrame()
            else -> {
                val nanos = (wake - System.nanoTime()).coerceAtLeast(0L)
                val millis = (nanos + 999_999L) / 1_000_000L
                if (!framePending) {
                    framePending = true
                    frameDelayed = true
                    choreographer.postFrameCallbackDelayed(this, millis)
                }
            }
        }
    }

    @Suppress("UNUSED_PARAMETER") // These fields will be used when the requests are implemented.
    private fun perform(kind: Int, rect: FloatArray, text: String?) {
        when (kind) {
            Native.REQUEST_BEGIN_TEXT_ENTRY, Native.REQUEST_END_TEXT_ENTRY,
            Native.REQUEST_SHARE, Native.REQUEST_HAPTIC, Native.REQUEST_OPEN_URL ->
                Log.d("Beryl3d", "Received platform request " + kind)
        }
    }

    private fun sendTouch(id: Int, phase: Int, x: Float, y: Float) {
        if (app == 0L || currentSurface == null) return
        Native.pointer(app, id, Native.POINTER_TOUCH, phase, x, y, Native.BUTTON_PRIMARY, 0f, 0f, 0)
        requestFrame()
    }

    private fun sendBack() {
        if (app == 0L) return
        Native.key(app, Native.KEY_BACK, Native.KEY_DOWN, 0)
        Native.key(app, Native.KEY_BACK, Native.KEY_UP, 0)
        requestFrame()
    }

    @Deprecated("Used for Back on Android 12 and earlier")
    override fun onBackPressed() = sendBack()

    override fun onPause() {
        super.onPause()
        if (app != 0L) Native.lifecycle(app, Native.SUSPENDED)
    }

    override fun onResume() {
        super.onResume()
        if (app != 0L) Native.lifecycle(app, Native.RESUMED)
        requestFrame()
    }

    @Deprecated("Activity.onLowMemory is the call API 24 has")
    override fun onLowMemory() {
        super.onLowMemory()
        if (app != 0L) Native.lifecycle(app, Native.LOW_MEMORY)
        requestFrame()
    }
}

/** The largest obstruction on each edge, in pixels until the surface is reported. */
private data class SafeInsets(
    val top: Int = 0, val right: Int = 0, val bottom: Int = 0, val left: Int = 0,
) {
    companion object {
        @Suppress("DEPRECATION") // The per-edge getters are needed before API 30.
        fun from(insets: WindowInsets): SafeInsets {
            if (Build.VERSION.SDK_INT >= 30) {
                val edges = insets.getInsets(
                    WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout(),
                )
                return SafeInsets(edges.top, edges.right, edges.bottom, edges.left)
            }
            val cutout = if (Build.VERSION.SDK_INT >= 28) insets.displayCutout else null
            return SafeInsets(
                maxOf(insets.systemWindowInsetTop, cutout?.safeInsetTop ?: 0),
                maxOf(insets.systemWindowInsetRight, cutout?.safeInsetRight ?: 0),
                maxOf(insets.systemWindowInsetBottom, cutout?.safeInsetBottom ?: 0),
                maxOf(insets.systemWindowInsetLeft, cutout?.safeInsetLeft ?: 0),
            )
        }
    }
}
