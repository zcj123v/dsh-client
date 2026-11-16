package com.zcj.dshclient

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.net.http.SslError
import android.os.Bundle
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.*
import android.widget.*
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import java.lang.ref.WeakReference

/** A plain Android WebView. Never extends TauriActivity or installs a JavaScript bridge. */
class DshContentActivity : AppCompatActivity() {
    companion object {
        private var active = WeakReference<DshContentActivity>(null)
        fun close(id: String) { active.get()?.takeIf { it.connectionId == id }?.finish() }
    }
    private var connectionId = ""
    private var baseUrl = ""
    private var titleText = "DSH"
    private var web: WebView? = null
    private lateinit var body: LinearLayout
    private lateinit var status: TextView
    private var fileCallback: ValueCallback<Array<Uri>>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; fitsSystemWindows = true }
        val bar = LinearLayout(this)
        bar.addView(Button(this).apply { text = "连接列表"; setOnClickListener { finish() } })
        bar.addView(Button(this).apply { text = "重新加载"; setOnClickListener { if (baseUrl.isNotEmpty()) web?.loadUrl(baseUrl) } })
        root.addView(bar)
        status = TextView(this).apply { setPadding(16, 8, 16, 8) }
        root.addView(status)
        body = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        root.addView(body, LinearLayout.LayoutParams(-1, 0, 1f))
        setContentView(root)
        active = WeakReference(this)
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() { if (web?.canGoBack() == true) web?.goBack() else finish() }
        })
        accept(intent, savedInstanceState)
    }
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        accept(intent, null)
    }
    override fun onSaveInstanceState(outState: Bundle) {
        // Restore only the non-secret connection identity and root URL. Never save WebView state/history.
        outState.putString("connectionId", connectionId)
        outState.putString("connectionName", titleText)
        outState.putString("connectionRoot", baseUrl)
        super.onSaveInstanceState(outState)
    }
    private fun accept(intent: Intent, savedState: Bundle?) {
        val id = intent.getStringExtra("connectionId") ?: ""
        val request = ContentRequests.take(id) ?: savedState?.takeIf { it.getString("connectionId") == id }?.let { state ->
            val root = state.getString("connectionRoot") ?: ""
            if (ContentPolicy.validInitialUrl(root) && !root.contains('?')) ContentRequest(state.getString("connectionName") ?: "DSH", root) else null
        }
        if (!ContentPolicy.validId(id) || request == null || !ContentPolicy.validInitialUrl(request.url)) {
            // Process/activity recreation never restores a token-bearing URL from Bundle/Intent.
            message("请返回连接列表重新打开；登录 Cookie 已按连接保留")
            return
        }
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
            message("请更新 Android System WebView；当前版本不支持独立连接存储")
            return
        }
        destroyWeb()
        connectionId = id
        titleText = request.name
        baseUrl = request.url.substringBefore('?')
        try {
            val view = WebView(this)
            // Must precede settings, evaluateJavascript, clients or navigation.
            WebViewCompat.setProfile(view, "dsh-$id")
            web = view
            view.isSaveEnabled = false
            view.settings.apply {
                javaScriptEnabled = true
                domStorageEnabled = true
                allowFileAccess = false
                allowContentAccess = true // SAF content:// attachments only; no filesystem access.
                mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                setSupportMultipleWindows(false)
                javaScriptCanOpenWindowsAutomatically = false
                setGeolocationEnabled(false)
            }
            view.webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                    val target = request.url.toString()
                    if (ContentPolicy.sameOrigin(baseUrl, target)) return false
                    if (request.isForMainFrame && request.hasGesture() && ContentPolicy.externalAllowed(target)) {
                        try { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(target))) } catch (_: Exception) { message("没有可处理外部链接的浏览器") }
                    }
                    return true
                }
                override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) { status.text = "$titleText · 正在加载" }
                override fun onPageFinished(view: WebView, url: String) {
                    if (ContentPolicy.sameOrigin(baseUrl, url)) {
                        view.evaluateJavascript("history.replaceState(null, '', location.pathname + location.hash)", null)
                        view.clearHistory()
                        WebViewCompat.getProfile(view).cookieManager.flush()
                        status.text = "$titleText · 登录状态见 DSH 页面"
                    }
                }
                override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
                    handler.cancel()
                    message("TLS 证书验证失败；请检查目标证书，不提供忽略错误入口")
                }
                override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                    if (request.isForMainFrame) message("连接失败，请检查网络或回到连接列表重试")
                }
                override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, response: WebResourceResponse) {
                    if (request.isForMainFrame && response.statusCode >= 400) {
                        status.text = if (response.statusCode == 401 || response.statusCode == 403) "认证未通过：返回连接列表填写当前 token" else "服务器错误：HTTP ${response.statusCode}"
                    }
                }
                override fun onReceivedHttpAuthRequest(view: WebView, handler: HttpAuthHandler, host: String, realm: String) {
                    val form = LinearLayout(this@DshContentActivity).apply { orientation = LinearLayout.VERTICAL }
                    val user = EditText(this@DshContentActivity).apply { hint = "网关用户" }
                    val password = EditText(this@DshContentActivity).apply { hint = "网关密码"; inputType = 129 }
                    form.addView(user); form.addView(password)
                    AlertDialog.Builder(this@DshContentActivity).setTitle("HTTPS 网关认证").setView(form)
                        .setPositiveButton("登录") { _, _ -> handler.proceed(user.text.toString(), password.text.toString()); password.text.clear() }
                        .setNegativeButton("取消") { _, _ -> handler.cancel() }.setOnCancelListener { handler.cancel() }.show()
                }
            }
            view.webChromeClient = object : WebChromeClient() {
                override fun onPermissionRequest(request: PermissionRequest) { request.deny() }
                override fun onShowFileChooser(view: WebView, callback: ValueCallback<Array<Uri>>, params: FileChooserParams): Boolean {
                    fileCallback?.onReceiveValue(null)
                    fileCallback = callback
                    val picker = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "*/*"
                        putExtra(Intent.EXTRA_ALLOW_MULTIPLE, params.mode == FileChooserParams.MODE_OPEN_MULTIPLE)
                    }
                    return try { startActivityForResult(picker, 201); true } catch (_: Exception) { fileCallback?.onReceiveValue(null); fileCallback = null; false }
                }
            }
            view.setDownloadListener { _, _, _, _, _ -> Toast.makeText(this, "当前 Android 测试版暂不支持下载，请在浏览器打开目标入口", Toast.LENGTH_LONG).show() }
            body.addView(view, LinearLayout.LayoutParams(-1, -1))
            view.loadUrl(request.url)
        } catch (_: Exception) { destroyWeb(); message("无法创建独立连接视图；请更新 Android System WebView") }
    }
    @Deprecated("Android Activity callback compatibility")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != 201) return
        val uris = mutableListOf<Uri>()
        if (resultCode == Activity.RESULT_OK) {
            data?.data?.let { if (it.scheme == "content") uris.add(it) }
            data?.clipData?.let { clip -> for (i in 0 until clip.itemCount) { val uri = clip.getItemAt(i).uri; if (uri.scheme == "content") uris.add(uri) } }
        }
        fileCallback?.onReceiveValue(uris.toTypedArray().takeIf { it.isNotEmpty() })
        fileCallback = null
    }
    private fun message(text: String) { if (::status.isInitialized) status.text = text }
    private fun destroyWeb() {
        fileCallback?.onReceiveValue(null); fileCallback = null
        web?.let { (it.parent as? ViewGroup)?.removeView(it); it.stopLoading(); it.destroy() }
        web = null
    }
    override fun onDestroy() {
        destroyWeb()
        if (active.get() === this) active.clear()
        super.onDestroy()
    }
}
