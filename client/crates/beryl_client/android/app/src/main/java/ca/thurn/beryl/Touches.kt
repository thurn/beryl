package ca.thurn.beryl

import android.content.Context
import android.view.MotionEvent
import android.view.SurfaceView

/** Maps Android's pointer ids to Beryl's touch ids, which start at one. */
internal class Touches(
    context: Context,
    private val send: (id: Int, phase: Int, x: Float, y: Float) -> Unit,
) : SurfaceView(context) {
    private val ids = mutableMapOf<Int, Int>()

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val index = event.actionIndex
                val id = (1..255).firstOrNull { it !in ids.values } ?: return true
                ids[event.getPointerId(index)] = id
                send(event, index, Native.PHASE_DOWN)
            }
            MotionEvent.ACTION_MOVE -> {
                for (index in 0 until event.pointerCount) send(event, index, Native.PHASE_MOVE)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> {
                val index = event.actionIndex
                send(event, index, Native.PHASE_UP)
                ids.remove(event.getPointerId(index))
            }
            MotionEvent.ACTION_CANCEL -> {
                for (index in 0 until event.pointerCount) send(event, index, Native.PHASE_CANCEL)
                ids.clear()
            }
        }
        return true
    }

    private fun send(event: MotionEvent, index: Int, phase: Int) {
        val id = ids[event.getPointerId(index)] ?: return
        val scale = resources.displayMetrics.density
        send(id, phase, event.getX(index) / scale, event.getY(index) / scale)
    }
}
