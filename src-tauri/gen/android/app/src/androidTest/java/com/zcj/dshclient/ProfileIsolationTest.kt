package com.zcj.dshclient

import android.content.ComponentName
import android.content.pm.ApplicationInfo
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class ProfileIsolationTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun loaded(profile: String): WebView {
        val ready = CountDownLatch(1)
        lateinit var view: WebView
        instrumentation.runOnMainSync {
            view = WebView(context)
            WebViewCompat.setProfile(view, "dsh-$profile")
            view.settings.javaScriptEnabled = true
            view.settings.domStorageEnabled = true
            view.webViewClient = object : WebViewClient() {
                override fun onPageFinished(v: WebView, url: String) { ready.countDown() }
            }
            view.loadDataWithBaseURL("https://dsh-client-test.invalid/", "<html><body>isolation fixture</body></html>", "text/html", "UTF-8", null)
        }
        assertTrue("fixture loaded", ready.await(20, TimeUnit.SECONDS))
        return view
    }
    private fun evaluate(view: WebView, script: String): String {
        val ready = CountDownLatch(1)
        var result = ""
        instrumentation.runOnMainSync { view.evaluateJavascript(script) { result = it; ready.countDown() } }
        assertTrue("JavaScript callback", ready.await(10, TimeUnit.SECONDS))
        return result
    }
    @Test
    fun profilesIsolateCookiesAndLocalStorageAndPersistAcrossViews() {
        assertTrue("Update System WebView to support isolated profiles", WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE))
        val firstId = UUID.randomUUID().toString()
        val secondId = UUID.randomUUID().toString()
        var first: WebView? = null
        var second: WebView? = null
        var reopened: WebView? = null
        try {
            first = loaded(firstId)
            evaluate(first, "localStorage.setItem('profile-proof','first'); document.cookie='profileproof=first;path=/;Secure'; 'written'")
            assertEquals("\"first\"", evaluate(first, "localStorage.getItem('profile-proof')"))
            assertTrue(evaluate(first, "document.cookie").contains("profileproof=first"))
            second = loaded(secondId)
            assertEquals("null", evaluate(second, "localStorage.getItem('profile-proof')"))
            assertFalse(evaluate(second, "document.cookie").contains("profileproof=first"))
            instrumentation.runOnMainSync { WebViewCompat.getProfile(first!!).cookieManager.flush(); first!!.destroy() }
            first = null
            reopened = loaded(firstId)
            assertEquals("\"first\"", evaluate(reopened, "localStorage.getItem('profile-proof')"))
            assertTrue(evaluate(reopened, "document.cookie").contains("profileproof=first"))
        } finally {
            instrumentation.runOnMainSync {
                first?.destroy(); second?.destroy(); reopened?.destroy()
                PendingProfiles.mark(context, firstId)
                PendingProfiles.mark(context, secondId)
            }
        }
    }
    @Test
    fun contentViewSurvivesActivityRecreationWithoutSavingToken() {
        val id = UUID.randomUUID().toString()
        instrumentation.runOnMainSync { ContentRequests.put(id, ContentRequest("Test", "https://dsh-client-test.invalid/?token=TEST_ACTIVITY_TRANSIENT")) }
        val intent = android.content.Intent(context, DshContentActivity::class.java).putExtra("connectionId", id)
        val scenario = androidx.test.core.app.ActivityScenario.launch<DshContentActivity>(intent)
        try {
            scenario.onActivity { activity ->
                assertEquals(1, webViews(activity.window.decorView))
                val state = android.os.Bundle()
                val save = DshContentActivity::class.java.getDeclaredMethod("onSaveInstanceState", android.os.Bundle::class.java)
                save.isAccessible = true
                save.invoke(activity, state)
                assertEquals("https://dsh-client-test.invalid/", state.getString("connectionRoot"))
                assertFalse(state.toString().contains("TEST_ACTIVITY_TRANSIENT"))
            }
            scenario.recreate()
            scenario.onActivity { activity -> assertEquals("content WebView must be recreated", 1, webViews(activity.window.decorView)) }
        } finally {
            scenario.close()
            instrumentation.runOnMainSync { PendingProfiles.mark(context, id) }
        }
    }
    private fun webViews(view: android.view.View): Int {
        if (view is WebView) return 1
        if (view !is android.view.ViewGroup) return 0
        return (0 until view.childCount).sumOf { webViews(view.getChildAt(it)) }
    }
    @Test
    fun authenticationFailureRemainsVisibleAfterPageFinishedAndResetsOnRetry() {
        withContentView { activity, view ->
            val client = view.webViewClient
            val root = "https://dsh-client-test.invalid/"
            client.onPageStarted(view, root, null)
            val request = mainRequest(root)
            client.onReceivedHttpError(view, request, android.webkit.WebResourceResponse("text/html", "UTF-8", 401, "Unauthorized", emptyMap(), "denied".byteInputStream()))
            client.onPageFinished(view, root)
            assertTrue("401 must remain visible after finish", texts(activity.window.decorView).any { it.contains("认证未通过") })
            client.onPageStarted(view, root, null)
            client.onPageFinished(view, root)
            assertTrue("retry success clears previous failure", texts(activity.window.decorView).any { it.contains("登录状态见 DSH 页面") })
        }
    }
    @Test
    fun actualNetworkFailureCannotBeOverwrittenByPageFinished() {
        val id = UUID.randomUUID().toString()
        val root = "https://dsh-client-network-failure.invalid/"
        instrumentation.runOnMainSync { ContentRequests.put(id, ContentRequest("Network failure", root)) }
        val intent = android.content.Intent(context, DshContentActivity::class.java).putExtra("connectionId", id)
        val scenario = androidx.test.core.app.ActivityScenario.launch<DshContentActivity>(intent)
        val ready = CountDownLatch(1)
        try {
            scenario.onActivity { activity ->
                val view = locateWebView(activity.window.decorView)!!
                view.stopLoading()
                val actual = view.webViewClient
                view.webViewClient = object : WebViewClient() {
                    override fun onPageStarted(v: WebView, url: String, favicon: android.graphics.Bitmap?) { actual.onPageStarted(v, url, favicon) }
                    override fun onReceivedError(v: WebView, request: android.webkit.WebResourceRequest, error: android.webkit.WebResourceError) { actual.onReceivedError(v, request, error) }
                    override fun onPageFinished(v: WebView, url: String) { actual.onPageFinished(v, url); ready.countDown() }
                }
                view.loadUrl(root)
            }
            assertTrue("failed network navigation finished", ready.await(30, TimeUnit.SECONDS))
            scenario.onActivity { activity ->
                val visible = texts(activity.window.decorView)
                assertTrue("network failure remains visible", visible.any { it.contains("连接失败") })
                assertFalse(visible.any { it.contains("登录状态见 DSH 页面") })
            }
        } finally {
            scenario.close()
            instrumentation.runOnMainSync { PendingProfiles.mark(context, id) }
        }
    }
    @Test
    fun actualUntrustedTlsCertificateIsCancelledAndFailureRemainsVisible() {
        val root = InstrumentationRegistry.getArguments().getString("tlsFixtureUrl")
        org.junit.Assume.assumeNotNull(root)
        val target = root!!
        assertTrue("TLS fixture is loopback only", target.startsWith("https://localhost:") || target.startsWith("https://127.0.0.1:"))
        val id = UUID.randomUUID().toString()
        instrumentation.runOnMainSync { ContentRequests.put(id, ContentRequest("TLS fixture", target)) }
        val intent = android.content.Intent(context, DshContentActivity::class.java).putExtra("connectionId", id)
        val scenario = androidx.test.core.app.ActivityScenario.launch<DshContentActivity>(intent)
        val rejected = CountDownLatch(1)
        lateinit var currentView: WebView
        try {
            scenario.onActivity { activity ->
                currentView = locateWebView(activity.window.decorView)!!
                currentView.stopLoading()
                val actual = currentView.webViewClient
                currentView.webViewClient = object : WebViewClient() {
                    override fun onPageStarted(v: WebView, url: String, favicon: android.graphics.Bitmap?) { actual.onPageStarted(v, url, favicon) }
                    override fun onReceivedSslError(v: WebView, handler: android.webkit.SslErrorHandler, error: android.net.http.SslError) {
                        actual.onReceivedSslError(v, handler, error)
                        // Exercise a late finish callback too, regardless of the engine's error-page ordering.
                        actual.onPageFinished(v, target)
                        rejected.countDown()
                    }
                    override fun onPageFinished(v: WebView, url: String) { actual.onPageFinished(v, url) }
                }
                currentView.loadUrl(target)
            }
            assertTrue("System WebView delivered certificate rejection", rejected.await(30, TimeUnit.SECONDS))
            scenario.onActivity { activity -> assertTrue(texts(activity.window.decorView).any { it.contains("TLS 证书验证失败") }) }
            assertFalse("untrusted TLS page must not execute", evaluate(currentView, "document.body?.textContent || ''").contains("TLS_FIXTURE_MUST_NOT_LOAD"))
        } finally {
            scenario.close()
            instrumentation.runOnMainSync { PendingProfiles.mark(context, id) }
        }
    }
    private fun mainRequest(root: String) = object : android.webkit.WebResourceRequest {
        override fun getUrl() = android.net.Uri.parse(root)
        override fun isForMainFrame() = true
        override fun isRedirect() = false
        override fun hasGesture() = false
        override fun getMethod() = "GET"
        override fun getRequestHeaders(): MutableMap<String, String> = mutableMapOf()
    }
    private fun withContentView(check: (DshContentActivity, WebView) -> Unit) {
        val id = UUID.randomUUID().toString()
        instrumentation.runOnMainSync { ContentRequests.put(id, ContentRequest("Failure test", "https://dsh-client-test.invalid/")) }
        val intent = android.content.Intent(context, DshContentActivity::class.java).putExtra("connectionId", id)
        val scenario = androidx.test.core.app.ActivityScenario.launch<DshContentActivity>(intent)
        try {
            scenario.onActivity { activity ->
                val view = locateWebView(activity.window.decorView)!!
                view.stopLoading()
                check(activity, view)
            }
        } finally {
            scenario.close()
            instrumentation.runOnMainSync { PendingProfiles.mark(context, id) }
        }
    }
    private fun locateWebView(node: android.view.View): WebView? {
        if (node is WebView) return node
        if (node is android.view.ViewGroup) for (i in 0 until node.childCount) locateWebView(node.getChildAt(i))?.let { return it }
        return null
    }
    private fun texts(node: android.view.View): List<String> {
        if (node is android.widget.TextView) return listOf(node.text.toString())
        if (node !is android.view.ViewGroup) return emptyList()
        return (0 until node.childCount).flatMap { texts(node.getChildAt(it)) }
    }
    @Test
    fun contentActivityIsPrivateAndApplicationBackupIsDisabled() {
        val component = ComponentName(context, DshContentActivity::class.java)
        val info = context.packageManager.getActivityInfo(component, 0)
        assertFalse(info.exported)
        assertEquals(0, context.applicationInfo.flags and ApplicationInfo.FLAG_ALLOW_BACKUP)
    }
}
