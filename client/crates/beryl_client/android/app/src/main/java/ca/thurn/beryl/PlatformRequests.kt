package ca.thurn.beryl

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.util.Log
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import java.io.File
import java.io.IOException

/** Carries out requests on Android's main thread. */
internal class PlatformRequests(private val activity: Activity, private val view: View) {
    private var testReceiver: BroadcastReceiver? = null

    fun perform(kind: Int, rect: FloatArray, text: String?) {
        val name = when (kind) {
            Native.REQUEST_BEGIN_TEXT_ENTRY -> "begin-text-entry"
            Native.REQUEST_END_TEXT_ENTRY -> "end-text-entry"
            Native.REQUEST_SHARE -> "share"
            Native.REQUEST_HAPTIC -> "haptic"
            Native.REQUEST_OPEN_URL -> "open-url"
            else -> return
        }

        try {
            when (kind) {
                Native.REQUEST_SHARE -> share(File(requireNotNull(text)))
                Native.REQUEST_HAPTIC -> {
                    val feedback = if (rect[0].toInt() == Native.HAPTIC_MEDIUM) {
                        HapticFeedbackConstants.VIRTUAL_KEY
                    } else {
                        HapticFeedbackConstants.CLOCK_TICK
                    }
                    view.performHapticFeedback(feedback)
                }
                Native.REQUEST_OPEN_URL -> {
                    activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(requireNotNull(text))))
                }
            }
            trace(name)
        } catch (error: ActivityNotFoundException) {
            reportFailure(name, error)
        } catch (error: IOException) {
            reportFailure(name, error)
        } catch (error: IllegalArgumentException) {
            reportFailure(name, error)
        } catch (error: SecurityException) {
            reportFailure(name, error)
        }
    }

    private fun share(source: File) {
        val folder = File(activity.cacheDir, "share")
        val shared = File(folder, source.name)
        if (source.canonicalFile != shared.canonicalFile) {
            source.copyTo(shared, overwrite = true)
        }
        val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", shared)
        val send = Intent(Intent.ACTION_SEND).apply {
            type = if (source.extension == "jsonl") "text/plain" else "application/zip"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        activity.startActivity(Intent.createChooser(send, null))
    }

    private fun reportFailure(name: String, error: Exception) {
        // The source path or URL can contain private data.
        val message = "$name failed: ${error.javaClass.simpleName}"
        Log.w("BerylClient", message)
        trace(message)
    }

    fun trace(event: String) {
        if (BuildConfig.DEBUG) {
            try {
                File(activity.filesDir, "host.log").appendText("$event\n")
            } catch (error: IOException) {
                Log.w("BerylClient", "Could not write host.log", error)
            }
        }
    }

    fun registerTestReceiver(perform: (Int, FloatArray, String?) -> Unit) {
        if (!BuildConfig.DEBUG) return
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                val rect = FloatArray(4)
                when (intent.getStringExtra("what")) {
                    "haptic" -> {
                        rect[0] = Native.HAPTIC_MEDIUM.toFloat()
                        perform(Native.REQUEST_HAPTIC, rect, null)
                    }
                    "share" -> {
                        val log = activity.filesDir.listFiles()
                            ?.filter { it.extension == "jsonl" }
                            ?.maxByOrNull { it.lastModified() }
                        perform(Native.REQUEST_SHARE, rect, log?.path)
                    }
                    "open-url" -> perform(Native.REQUEST_OPEN_URL, rect, "https://example.com")
                }
            }
        }
        ContextCompat.registerReceiver(
            activity, receiver, IntentFilter("ca.thurn.beryl.TEST"),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        testReceiver = receiver
    }

    fun unregisterTestReceiver() {
        testReceiver?.let { activity.unregisterReceiver(it) }
    }
}
