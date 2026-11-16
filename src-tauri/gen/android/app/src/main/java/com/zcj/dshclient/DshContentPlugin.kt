package com.zcj.dshclient

import android.app.Activity
import android.content.Context
import android.content.Intent
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewFeature

/** The only bridge belongs to the local Tauri shell. ContentActivity has none. */
@TauriPlugin
class DshContentPlugin(private val host: Activity) : Plugin(host) {
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
