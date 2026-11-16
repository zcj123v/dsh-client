package com.zcj.dshclient

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.content.IntentFilter
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class AttachmentSelectionTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun evaluate(view: WebView, expression: String): String {
        val ready = CountDownLatch(1)
        var result = ""
        instrumentation.runOnMainSync { view.evaluateJavascript(expression) { result = it; ready.countDown() } }
        assertTrue("JavaScript callback", ready.await(10, TimeUnit.SECONDS))
        return result
    }
    private fun locate(node: View): WebView? {
        if (node is WebView) return node
        if (node is ViewGroup) for (index in 0 until node.childCount) locate(node.getChildAt(index))?.let { return it }
        return null
    }
    private fun fixture(check: (WebView) -> Unit) {
        val id = UUID.randomUUID().toString()
        instrumentation.runOnMainSync { ContentRequests.put(id, ContentRequest("Attachment test", "https://dsh-client-test.invalid/")) }
        val scenario = ActivityScenario.launch<DshContentActivity>(Intent(context, DshContentActivity::class.java).putExtra("connectionId", id))
        lateinit var view: WebView
        val loaded = CountDownLatch(1)
        try {
            scenario.onActivity { activity ->
                view = locate(activity.window.decorView)!!
                view.stopLoading()
                // Keep the real production WebChromeClient/file-chooser handler. Replace only navigation
                // observation for this controlled, local HTML page; no server or DSH is needed.
                view.webViewClient = object : WebViewClient() {
                    override fun onPageFinished(v: WebView, url: String) { if (url == "https://dsh-attachment-test.invalid/") loaded.countDown() }
                }
                view.loadDataWithBaseURL("https://dsh-attachment-test.invalid/", """
                    <!doctype html><meta name="viewport" content="width=device-width, initial-scale=1"><style>input { font-size:24px; width:90%; height:100px; }</style><input id="attachment" type="file" accept="text/plain">
                    <script>
                    window.fileResult = 'waiting';
                    document.querySelector('#attachment').onchange = async event => {
                        const file = event.target.files[0];
                        try { window.fileResult = file ? JSON.stringify({name:file.name,body:await file.text()}) : 'cancelled'; }
                        catch (error) { window.fileResult = 'fixture read failed: ' + error.name; }
                    };
                    </script>
                """.trimIndent(), "text/html", "UTF-8", null)
            }
            assertTrue("HTML file input loaded", loaded.await(20, TimeUnit.SECONDS))
            assertEquals("controlled attachment page rendered: " + evaluate(view, "document.body?.textContent"), "true", evaluate(view, "!!document.querySelector('#attachment')"))
            check(view)
        } finally {
            scenario.close()
            instrumentation.runOnMainSync { PendingProfiles.mark(context, id) }
        }
    }
    private fun choose(view: WebView, resultCode: Int, data: Intent?) {
        // The external system picker is the only substituted boundary. It returns a real content URI,
        // and production onActivityResult hands that URI back to the real WebView file input.
        val filter = IntentFilter(Intent.ACTION_OPEN_DOCUMENT).apply { addCategory(Intent.CATEGORY_OPENABLE); addDataType("*/*") }
        val monitor = instrumentation.addMonitor(filter, Instrumentation.ActivityResult(resultCode, data), true)
        try {
            val box = org.json.JSONObject(evaluate(view, "(() => {const r=document.querySelector('#attachment').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()"))
            val position = IntArray(2)
            instrumentation.runOnMainSync { view.getLocationOnScreen(position) }
            val density = context.resources.displayMetrics.density
            val x = position[0] + box.getDouble("x").toFloat() * density
            val y = position[1] + box.getDouble("y").toFloat() * density
            val time = android.os.SystemClock.uptimeMillis()
            instrumentation.sendPointerSync(android.view.MotionEvent.obtain(time, time, android.view.MotionEvent.ACTION_DOWN, x, y, 0))
            instrumentation.sendPointerSync(android.view.MotionEvent.obtain(time, time + 40, android.view.MotionEvent.ACTION_UP, x, y, 0))
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
            while (monitor.hits == 0 && System.nanoTime() < deadline) Thread.sleep(25)
            assertEquals("exactly one native document chooser", 1, monitor.hits)
            instrumentation.waitForIdleSync()
        } finally { instrumentation.removeMonitor(monitor) }
    }
    private fun selected(view: WebView): String {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
        while (System.nanoTime() < deadline) {
            val value = evaluate(view, "window.fileResult")
            if (value.contains("DSH_CLIENT_ATTACHMENT_FIXTURE")) return value
            Thread.sleep(25)
        }
        return evaluate(view, "window.fileResult")
    }
    @Test
    fun selectedContentUriBecomesReadableWebAttachment() = fixture { view ->
        choose(view, Activity.RESULT_OK, Intent().setData(AttachmentFixtureProvider.URI).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
        val result = org.json.JSONObject(org.json.JSONTokener(selected(view)).nextValue() as String)
        assertEquals("fixture.txt", result.getString("name"))
        assertEquals("page reads exact synthetic attachment bytes", AttachmentFixtureProvider.PAYLOAD, result.getString("body"))
    }
    @Test
    fun nonContentResultIsRejectedAndLaterSelectionStillWorks() = fixture { view ->
        choose(view, Activity.RESULT_OK, Intent().setData(android.net.Uri.parse("file:///DSH_CLIENT_SYNTHETIC_NONEXISTENT.txt")))
        assertEquals("non-content URI must not enter WebView", "0", evaluate(view, "document.querySelector('#attachment').files.length"))
        choose(view, Activity.RESULT_OK, Intent().setData(AttachmentFixtureProvider.URI).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
        val result = selected(view)
        assertTrue("selection after rejected URI: $result", result.contains("DSH_CLIENT_ATTACHMENT_FIXTURE"))
    }
    @Test
    fun cancelledPickerDoesNotPreventSelectingAnotherAttachment() = fixture { view ->
        choose(view, Activity.RESULT_CANCELED, null)
        assertEquals("0", evaluate(view, "document.querySelector('#attachment').files.length"))
        choose(view, Activity.RESULT_OK, Intent().setData(AttachmentFixtureProvider.URI).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
        val result = selected(view)
        assertTrue("read after cancel: $result", result.contains("DSH_CLIENT_ATTACHMENT_FIXTURE"))
    }
}
