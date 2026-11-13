package ca.thurn.beryl.mobile

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Build
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.IOException

@TauriPlugin
class BerylMobilePlugin(private val activity: Activity) : Plugin(activity) {
    private lateinit var webView: WebView

    override fun load(webView: WebView) {
        this.webView = webView
    }

    @Command
    fun snapshot(invoke: Invoke) {
        activity.runOnUiThread {
            if (!::webView.isInitialized || webView.width == 0 || webView.height == 0) {
                invoke.reject("The WebView is unavailable")
                return@runOnUiThread
            }

            try {
                val bitmap = Bitmap.createBitmap(webView.width, webView.height, Bitmap.Config.ARGB_8888)
                try {
                    webView.draw(Canvas(bitmap))
                    val file = File.createTempFile("snapshot-", ".png", activity.cacheDir)
                    try {
                        file.outputStream().use {
                            if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) {
                                throw IOException("The WebView made no PNG")
                            }
                        }
                    } catch (error: Exception) {
                        file.delete()
                        throw error
                    }
                    invoke.resolve(JSObject().put("path", file.absolutePath))
                } finally {
                    bitmap.recycle()
                }
            } catch (error: Exception) {
                invoke.reject("Could not snapshot the WebView", error)
            }
        }
    }

    @Command
    fun device(invoke: Invoke) {
        val simulator = Build.FINGERPRINT.contains("generic") ||
            Build.HARDWARE in listOf("ranchu", "goldfish")
        invoke.resolve(
            JSObject()
                .put("os", Build.VERSION.RELEASE)
                .put("model", Build.MODEL)
                .put("simulator", simulator)
        )
    }
}
