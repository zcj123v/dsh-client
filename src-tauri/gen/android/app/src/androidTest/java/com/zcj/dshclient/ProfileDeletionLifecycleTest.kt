package com.zcj.dshclient

import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Run prepare -> force-stop -> verify in separate processes. Uses a dedicated synthetic UUID. */
@RunWith(AndroidJUnit4::class)
class ProfileDeletionLifecycleTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    @Test
    fun durableDeletionRunsBeforeProfileLoadingInNextProcess() {
        val arguments = InstrumentationRegistry.getArguments()
        val phase = arguments.getString("deletionPhase")
        org.junit.Assume.assumeTrue("explicit two-process lifecycle runner required", phase == "prepare" || phase == "verify")
        val id = arguments.getString("deletionId") ?: error("dedicated UUID required")
        assertTrue(ContentPolicy.validId(id))
        assertTrue(WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE))
        val profile = "dsh-$id"
        if (phase == "prepare") {
            val ready = CountDownLatch(1)
            lateinit var view: WebView
            instrumentation.runOnMainSync {
                assertFalse("fresh fixture identity", ProfileStore.getInstance().allProfileNames.contains(profile))
                view = WebView(context)
                WebViewCompat.setProfile(view, profile)
                view.settings.javaScriptEnabled = true
                view.settings.domStorageEnabled = true
                view.webViewClient = object : WebViewClient() {
                    override fun onPageFinished(v: WebView, url: String) { ready.countDown() }
                }
                view.loadDataWithBaseURL("https://dsh-client-test.invalid/", "<body>synthetic profile</body>", "text/html", "UTF-8", null)
            }
            assertTrue(ready.await(20, TimeUnit.SECONDS))
            val stored = CountDownLatch(1)
            instrumentation.runOnMainSync {
                view.evaluateJavascript("localStorage.setItem('deleted-proof','synthetic'); document.cookie='deletedproof=synthetic;Secure;Path=/'; localStorage.getItem('deleted-proof')") { result ->
                    assertEquals("\"synthetic\"", result)
                    stored.countDown()
                }
            }
            assertTrue(stored.await(10, TimeUnit.SECONDS))
            instrumentation.runOnMainSync {
                WebViewCompat.getProfile(view).cookieManager.flush()
                view.destroy()
                assertTrue(PendingProfiles.mark(context, id))
                PendingProfiles.cleanup(context)
                assertTrue("loaded profile keeps durable deletion marker", PendingProfiles.contains(context, id))
                assertTrue("loaded profile remains until next process", ProfileStore.getInstance().allProfileNames.contains(profile))
            }
        } else {
            instrumentation.runOnMainSync {
                assertTrue("marker survived process death", PendingProfiles.contains(context, id))
                // Match MainActivity startup: do not load/getOrCreate this Profile before cleanup.
                PendingProfiles.cleanup(context)
                assertFalse("startup consumed deletion marker", PendingProfiles.contains(context, id))
                assertFalse("deleted profile removed from store", ProfileStore.getInstance().allProfileNames.contains(profile))
            }
        }
    }
}
