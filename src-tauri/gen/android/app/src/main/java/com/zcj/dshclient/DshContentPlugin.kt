package com.zcj.dshclient

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ClipDescription
import android.os.Build
import android.os.PersistableBundle
import android.content.Context
import android.content.Intent
import androidx.activity.result.ActivityResult
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewFeature
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.net.URL
import javax.net.ssl.HttpsURLConnection

/** The only bridge belongs to the local Tauri shell. ContentActivity has none. */
@TauriPlugin
class DshContentPlugin(private val host: Activity) : Plugin(host) {
    companion object {
        // Mirrors the Rust-side export bound: 1 MiB of UTF-8 text.
        private const val MAX_EXPORT_BYTES = 1024 * 1024
    }

    /** Export text held only while the SAF picker is on screen; never logged. */
    private var pendingExportText: String? = null

    @Command
    fun open(invoke: Invoke) {
        val args = invoke.getArgs()
        val id = args.optString("id")
        val name = args.optString("name")
        val url = args.optString("url")
        host.runOnUiThread {
            if (!ContentPolicy.validId(id) || !ContentPolicy.validInitialUrl(url) || name.isBlank() || name.length > 160) {
                invoke.reject("Invalid connection")
            } else if (!WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
                invoke.reject("Update Android System WebView: isolated profiles unavailable")
            } else {
                try {
                    PendingProfiles.cleanup(host)
                    if (PendingProfiles.contains(host, id)) {
                        invoke.reject("Deleted profile cannot be reopened")
                        return@runOnUiThread
                    }
                    ContentRequests.put(id, ContentRequest(name, url))
                    host.startActivity(Intent(host, DshContentActivity::class.java).putExtra("connectionId", id)
                        .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP))
                    invoke.resolve()
                } catch (_: Exception) {
                    ContentRequests.remove(id)
                    invoke.reject("Unable to open isolated content")
                }
            }
        }
    }
    @Command
    fun close(invoke: Invoke) {
        val id = invoke.getArgs().optString("id")
        host.runOnUiThread {
            if (!ContentPolicy.validId(id)) { invoke.reject("Invalid connection"); return@runOnUiThread }
            ContentRequests.remove(id)
            DshContentActivity.close(id)
            invoke.resolve()
        }
    }
    @Command
    fun delete(invoke: Invoke) {
        val id = invoke.getArgs().optString("id")
        host.runOnUiThread {
            if (!ContentPolicy.validId(id)) { invoke.reject("Invalid connection"); return@runOnUiThread }
            if (!PendingProfiles.mark(host, id)) { invoke.reject("Unable to record profile deletion"); return@runOnUiThread }
            ContentRequests.remove(id)
            DshContentActivity.close(id)
            PendingProfiles.cleanup(host)
            // Loaded profiles are retained as tombstones until the next process start.
            invoke.resolve()
        }
    }

    /** Ordered HTTPS fallback: probe each candidate with the platform HTTPS stack
     *  (never sending the token) and open the first reachable one in an isolated
     *  profile. Resolves { reached, index } where index is the ORIGINAL list index. */
    @Command
    fun openOrdered(invoke: Invoke) {
        val args = invoke.getArgs()
        val id = args.optString("id")
        val name = args.optString("name")
        val candidates = args.optJSONArray("candidates")
        host.runOnUiThread {
            var valid = ContentPolicy.validId(id) && name.isNotBlank() && name.length <= 160
                && candidates != null && candidates.length() > 0
            if (valid) {
                for (i in 0 until candidates!!.length()) {
                    val candidate = candidates!!.optJSONObject(i) ?: continue
                    if (candidate.optInt("index", -1) < 0
                        || !ContentPolicy.validInitialUrl(candidate.optString("probeUrl"))
                        || !ContentPolicy.validInitialUrl(candidate.optString("openUrl"))) {
                        valid = false
                        break
                    }
                }
            }
            if (!valid) {
                invoke.reject("Invalid connection")
            } else if (!WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
                invoke.reject("Update Android System WebView: isolated profiles unavailable")
            } else {
                try {
                    PendingProfiles.cleanup(host)
                    if (PendingProfiles.contains(host, id)) {
                        invoke.reject("Deleted profile cannot be reopened")
                        return@runOnUiThread
                    }
                    val ordered = (0 until candidates!!.length())
                        .map { candidates.optJSONObject(it)!! }
                    Thread {
                        val reachable = ordered.firstOrNull { probeHttps(it.optString("probeUrl")) }
                        host.runOnUiThread {
                            if (reachable == null) {
                                invoke.resolve(JSObject().put("reached", false))
                            } else {
                                try {
                                    ContentRequests.put(id, ContentRequest(name, reachable.optString("openUrl")))
                                    host.startActivity(Intent(host, DshContentActivity::class.java).putExtra("connectionId", id)
                                        .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP))
                                    invoke.resolve(JSObject().put("reached", true).put("index", reachable.optInt("index")))
                                } catch (_: Exception) {
                                    ContentRequests.remove(id)
                                    invoke.reject("Unable to open isolated content")
                                }
                            }
                        }
                    }.start()
                } catch (_: Exception) {
                    invoke.reject("Unable to open isolated content")
                }
            }
        }
    }

    /** Plain-text copy of an export. Text is never logged. */
    @Command
    fun copyConfig(invoke: Invoke) {
        val text = invoke.getArgs().optString("text")
        host.runOnUiThread {
            if (text.isEmpty() || text.toByteArray(Charsets.UTF_8).size > MAX_EXPORT_BYTES) {
                invoke.reject("Invalid export text")
            } else {
                try {
                    val clipboard = host.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    val clip = ClipData.newPlainText("dsh-config", text)
                    if (Build.VERSION.SDK_INT >= 33) {
                        clip.description.extras = PersistableBundle().apply {
                            putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
                        }
                    }
                    clipboard.setPrimaryClip(clip)
                    invoke.resolve()
                } catch (_: Exception) {
                    invoke.reject("Unable to copy export")
                }
            }
        }
    }

    /** SAF save: the UI never sees or chooses a path, only the system picker does. */
    @Command
    fun saveConfig(invoke: Invoke) {
        val text = invoke.getArgs().optString("text")
        host.runOnUiThread {
            if (text.isEmpty() || text.toByteArray(Charsets.UTF_8).size > MAX_EXPORT_BYTES) {
                invoke.reject("Invalid export text")
            } else if (pendingExportText != null) {
                invoke.reject("A save dialog is already open")
            } else {
                try {
                    pendingExportText = text
                    val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "application/json"
                        putExtra(Intent.EXTRA_TITLE, "dsh-config.json")
                    }
                    startActivityForResult(invoke, intent, "saveConfigResult")
                } catch (_: Exception) {
                    pendingExportText = null
                    invoke.reject("Unable to open save dialog")
                }
            }
        }
    }

    @ActivityCallback
    fun saveConfigResult(invoke: Invoke, result: ActivityResult) {
        val uri = result.data?.data
        val text = pendingExportText
        pendingExportText = null
        if (result.resultCode != Activity.RESULT_OK || uri == null || text == null) {
            invoke.resolve(JSObject().put("saved", false))
            return
        }
        Thread {
            val saved = try {
                host.contentResolver.openOutputStream(uri, "wt")?.use { stream ->
                    stream.write(text.toByteArray(Charsets.UTF_8))
                    stream.flush()
                } != null
            } catch (_: Exception) {
                false
            }
            host.runOnUiThread {
                if (saved) invoke.resolve(JSObject().put("saved", true))
                else invoke.reject("Unable to save export")
            }
        }.start()
    }

    /** Reachability probe mirroring the desktop semantics: no redirects, no
     *  credentials, and 2xx–3xx / 401 / 403 count as reachable login gates. */
    private fun probeHttps(url: String): Boolean {
        return try {
            val connection = URL(url).openConnection() as HttpsURLConnection
            connection.requestMethod = "GET"
            connection.connectTimeout = 2000
            connection.readTimeout = 3000
            connection.instanceFollowRedirects = false
            try {
                ContentPolicy.reachableResponse(url, connection.responseCode, connection.getHeaderField("Location"))
            } finally {
                connection.disconnect()
            }
        } catch (_: Exception) {
            false
        }
    }
}

internal data class ContentRequest(val name: String, val url: String)
internal object ContentRequests {
    private val pending = mutableMapOf<String, ContentRequest>()
    fun put(id: String, request: ContentRequest) { pending[id] = request }
    fun take(id: String): ContentRequest? = pending.remove(id)
    fun remove(id: String) { pending.remove(id) }
}
internal object PendingProfiles {
    private const val PREFS = "dsh-profile-deletions"
    private const val KEY = "ids"
    fun contains(context: Context, id: String): Boolean = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getStringSet(KEY, emptySet())!!.contains(id)
    fun mark(context: Context, id: String): Boolean {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        return prefs.edit().putStringSet(KEY, prefs.getStringSet(KEY, emptySet())!!.toSet() + id).commit()
    }
    fun cleanup(context: Context) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) return
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val ids = prefs.getStringSet(KEY, emptySet())!!.toSet()
        val pending = ids.toMutableSet()
        for (id in ids) {
            if (!ContentPolicy.validId(id)) { pending.remove(id); continue }
            try {
                // Never getProfile/getOrCreateProfile here: that would load it and prevent deletion.
                ProfileStore.getInstance().deleteProfile("dsh-$id")
                pending.remove(id)
            } catch (_: IllegalStateException) { /* Next process start: loaded profile cannot be deleted. */ }
            catch (_: Exception) { /* Keep the durable tombstone on any platform error. */ }
        }
        if (pending != ids) prefs.edit().putStringSet(KEY, pending).commit()
    }
}
