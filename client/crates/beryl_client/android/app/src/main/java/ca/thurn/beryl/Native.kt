package ca.thurn.beryl

import android.view.Surface

/** The library's JNI functions (src/ffi/android.rs in beryl_client). */
object Native {
    init {
        System.loadLibrary("beryl_client")
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
    external fun surface(
        app: Long,
        surface: Surface,
        width: Int,
        height: Int,
        scale: Float,
        top: Float,
        right: Float,
        bottom: Float,
        left: Float,
    )
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
    /**
     * Returns [wake, hzBits]. Wake is 0 for now, -1 for never, or a System.nanoTime() deadline.
     * Decode the display rate with Float.fromBits.
     */
    external fun frame(app: Long, now: Long): LongArray
    /**
     * The next request's kind, or -1 for none. Writes its rectangle into `rect` (x, y,
     * width, height), or for a haptic its strength into `rect[0]`.
     */
    external fun nextRequest(app: Long, rect: FloatArray): Int
    /** The text of the last request `nextRequest` returned, or null. */
    external fun requestText(app: Long): String?

    const val POINTER_TOUCH = 1
    const val PHASE_DOWN = 0
    const val PHASE_MOVE = 1
    const val PHASE_UP = 2
    const val PHASE_CANCEL = 3
    const val BUTTON_PRIMARY = 0

    const val KEY_BACK = 46
    const val KEY_DOWN = 0
    const val KEY_UP = 2

    const val REQUEST_BEGIN_TEXT_ENTRY = 0
    const val REQUEST_END_TEXT_ENTRY = 1
    const val REQUEST_SHARE = 2
    const val REQUEST_HAPTIC = 3
    const val REQUEST_OPEN_URL = 4

    const val HAPTIC_MEDIUM = 1

    const val SUSPENDED = 0
    const val RESUMED = 1
    const val LOW_MEMORY = 2
}
