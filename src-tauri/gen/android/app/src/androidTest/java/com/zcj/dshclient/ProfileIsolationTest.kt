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
    fun contentActivityIsPrivateAndApplicationBackupIsDisabled() {
        val component = ComponentName(context, DshContentActivity::class.java)
        val info = context.packageManager.getActivityInfo(component, 0)
        assertFalse(info.exported)
        assertEquals(0, context.applicationInfo.flags and ApplicationInfo.FLAG_ALLOW_BACKUP)
    }
}
