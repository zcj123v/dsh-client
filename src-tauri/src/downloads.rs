use crate::{Connections, policy::same_origin, tabs::TabSession};
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::{CONTENT_RANGE, COOKIE, RANGE},
    redirect::Policy,
};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use std::{
    ffi::CString,
    fs,
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Weak, atomic::Ordering},
    time::Duration,
};
use tauri::{AppHandle, Manager, Url, Webview, webview::Cookie};
use tauri_plugin_dialog::DialogExt;
use tempfile::NamedTempFile;

const CHUNK_SIZE: u64 = 1024 * 1024;
const READ_BUFFER_SIZE: usize = 64 * 1024;
const MAX_REDIRECTS: usize = 5;
const RENAME_EXCL: u32 = 0x0000_0004;

fn diagnostic_message(stage: &'static str) -> String {
    format!("[dsh-download] {stage}")
}

fn diagnostic(stage: &'static str) {
    eprintln!("{}", diagnostic_message(stage));
}

fn diagnostic_title_stage(title: &str) -> Option<&'static str> {
    const STAGES: &[&str] = &[
        "bridge-existing",
        "bridge-ipc-unavailable",
        "bridge-install",
        "save-request",
        "save-request-rejected",
        "save-url-rejected",
        "save-mode-rejected",
        "save-field-rejected",
        "save-ipc-start",
        "save-ipc-saved",
        "save-ipc-cancelled",
        "save-ipc-invalid",
        "save-ipc-acl-rejected",
        "save-ipc-error",
        "bridge-installed",
        "bridge-missing",
        "ipc-unavailable",
    ];
    let stage = title.strip_prefix("[dsh-download] ")?;
    STAGES.iter().copied().find(|candidate| *candidate == stage)
}

pub fn diagnose_title(title: &str) {
    if let Some(stage) = diagnostic_title_stage(title) {
        diagnostic(stage);
    }
}

fn noted<T>(result: Result<T, String>, stage: &'static str) -> Result<T, String> {
    result.map_err(|error| {
        diagnostic(stage);
        error
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DownloadRequest {
    pub url: String,
    pub name: Option<String>,
    pub media_type: Option<String>,
    pub mode: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SaveOutcome {
    Saved(PathBuf),
    Cancelled,
}

impl Serialize for SaveOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Saved(path) => {
                let mut value = serializer.serialize_struct("SaveOutcome", 2)?;
                value.serialize_field("status", "saved")?;
                value.serialize_field("path", path)?;
                value.end()
            }
            Self::Cancelled => {
                let mut value = serializer.serialize_struct("SaveOutcome", 1)?;
                value.serialize_field("status", "cancelled")?;
                value.end()
            }
        }
    }
}

struct NodeContent {
    id: String,
    origin: Url,
    session: Weak<TabSession>,
    generation: u64,
}

impl NodeContent {
    fn with_current(
        &self,
        state: &Connections,
        action: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        let session = noted(
            self.session.upgrade().ok_or("节点已关闭或已替换".into()),
            "owner-expired",
        )?;
        // Keep this registry lock through action. tabs::close uses the same lock
        // before cancellation/removal, so close linearizes before or after commit.
        let tabs = noted(
            state.tabs.lock().map_err(|_| "标签状态不可用".into()),
            "owner-registry-error",
        )?;
        if session.cancelled.load(Ordering::SeqCst)
            || session.generation.load(Ordering::SeqCst) != self.generation
            || !tabs
                .get(&self.id)
                .is_some_and(|current| Arc::ptr_eq(&current, &session))
        {
            diagnostic("owner-stale");
            return Err("节点已关闭或已替换".into());
        }
        let origin = noted(
            state
                .urls
                .lock()
                .map_err(|_| "连接状态不可用".into())
                .and_then(|urls| {
                    urls.get(&self.id)
                        .cloned()
                        .ok_or("未知节点或连接已关闭".into())
                }),
            "owner-origin-missing",
        )?;
        if origin != self.origin {
            diagnostic("owner-origin-changed");
            return Err("节点入口已变化，请重试".into());
        }
        action()
    }

    fn is_current(&self, state: &Connections) -> Result<(), String> {
        self.with_current(state, &mut || Ok(()))
    }
}

fn require_node_content_state(
    state: &Connections,
    webview_label: &str,
    window_label: &str,
    current: &Url,
) -> Result<NodeContent, String> {
    if window_label != "main" && window_label != webview_label {
        diagnostic("identity-window-rejected");
        return Err("仅已登记节点内容页可下载文件".into());
    }
    let id = noted(
        webview_label
            .strip_prefix("dsh-")
            .ok_or("仅已登记节点内容页可下载文件".into()),
        "identity-label-rejected",
    )?;
    noted(crate::policy::connection_id(id), "identity-id-rejected")?;
    let session = noted(
        state
            .tabs
            .lock()
            .map_err(|_| "标签状态不可用".into())
            .and_then(|tabs| tabs.get(id).ok_or("未知节点或连接已关闭".into())),
        "identity-unregistered",
    )?;
    if session.cancelled.load(Ordering::SeqCst) {
        diagnostic("identity-cancelled");
        return Err("节点已关闭或已替换".into());
    }
    let origin = noted(
        state
            .urls
            .lock()
            .map_err(|_| "连接状态不可用".into())
            .and_then(|urls| urls.get(id).cloned().ok_or("未知节点或连接已关闭".into())),
        "identity-origin-missing",
    )?;
    if !same_origin(&origin, current) {
        diagnostic("identity-origin-mismatch");
        return Err("节点页面来源已变化，请重试".into());
    }
    diagnostic("identity-accepted");
    Ok(NodeContent {
        id: id.into(),
        origin,
        generation: session.generation.load(Ordering::SeqCst),
        session: Arc::downgrade(&session),
    })
}

fn require_node_content(app: &AppHandle, webview: &Webview) -> Result<NodeContent, String> {
    let current = noted(
        webview.url().map_err(|_| "无法验证节点页面来源".into()),
        "identity-page-url-error",
    )?;
    require_node_content_state(
        &app.state::<Connections>(),
        webview.label(),
        webview.window().label(),
        &current,
    )
}

pub fn authorize_save_download(authority: &mut tauri::ipc::RuntimeAuthority) {
    // Node origins are runtime data, so transport ACL accepts remote origins;
    // save_download independently restricts the caller to a registered node
    // content WebView and the exact registered origin before doing any I/O.
    authority.__allow_command(
        "save_download".into(),
        tauri::utils::acl::ExecutionContext::Remote {
            url: "http{s}?://*:*/*"
                .parse()
                .expect("HTTP(S) remote URL pattern"),
        },
    );
}

pub fn file_actions_initialization_script() -> String {
    let source = include_str!("../../src/file-actions.ts").replace(
        "export function installFileActionsBridge",
        "function installFileActionsBridge",
    );
    format!("(() => {{\n{source}\ninstallFileActionsBridge(window);\n}})();")
}

pub fn file_actions_probe_script() -> &'static str {
    r#"(() => {
  const report = () => {
    const title = document.title
    const installed = typeof window.dshDesktop?.fileActions?.save === 'function'
    const ipc = typeof window.__TAURI_INTERNALS__?.invoke === 'function'
    const stage = installed ? 'bridge-installed' : ipc ? 'bridge-missing' : 'ipc-unavailable'
    const diagnostic = `[dsh-download] ${stage}`
    document.title = diagnostic
    setTimeout(() => {
      if (document.title === diagnostic) document.title = title
    }, 50)
  }
  report()
  setTimeout(report, 100)
})();"#
}

#[tauri::command]
pub async fn save_download(
    webview: Webview,
    app: AppHandle,
    request: DownloadRequest,
) -> Result<SaveOutcome, String> {
    diagnostic("command-start");
    let owner = noted(
        require_node_content(&app, &webview),
        "command-identity-rejected",
    )?;
    let result = match tauri::async_runtime::spawn_blocking(move || {
        save_guarded(&app, &webview, request, |action| {
            owner.with_current(&app.state::<Connections>(), action)
        })
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            diagnostic("command-task-error");
            return Err("下载任务未完成".into());
        }
    };
    match &result {
        Ok(SaveOutcome::Saved(_)) => diagnostic("command-saved"),
        Ok(SaveOutcome::Cancelled) => diagnostic("command-cancelled"),
        Err(_) => diagnostic("command-error"),
    }
    result
}

pub fn save(
    app: &AppHandle,
    webview: &Webview,
    request: DownloadRequest,
) -> Result<SaveOutcome, String> {
    save_guarded(app, webview, request, |action| action())
}

fn save_guarded<F>(
    app: &AppHandle,
    webview: &Webview,
    request: DownloadRequest,
    with_current: F,
) -> Result<SaveOutcome, String>
where
    F: Fn(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
{
    diagnostic("save-start");
    with_current(&mut || Ok(()))?;
    let id = noted(
        webview
            .label()
            .strip_prefix("dsh-")
            .ok_or("仅已登记节点可下载文件".into()),
        "save-label-rejected",
    )?;
    let origin = noted(
        app.state::<super::Connections>()
            .urls
            .lock()
            .map_err(|_| "连接状态不可用".into())
            .and_then(|urls| urls.get(id).cloned().ok_or("未知节点或连接已关闭".into())),
        "save-origin-missing",
    )?;
    let source = noted(
        validate_source(&request.url, Some(&origin)),
        "source-rejected",
    )?;
    let current = noted(
        webview.url().map_err(|_| "无法验证节点页面来源".into()),
        "save-page-url-error",
    )?;
    if !same_origin(&origin, &current) {
        diagnostic("save-page-origin-changed");
        return Err("节点页面来源已变化，请重试".into());
    }
    let name = safe_file_name(request.name.as_deref().unwrap_or("download"));
    let (destination, overwrite) = match request.mode.as_str() {
        "download" => {
            let directory = noted(
                app.path()
                    .download_dir()
                    .map_err(|_| "无法定位系统下载目录".into()),
                "download-directory-error",
            )?;
            diagnostic("download-directory-ready");
            let destination = noted(next_available(&directory, &name), "download-name-error")?;
            (destination, false)
        }
        "saveAs" => {
            diagnostic("dialog-start");
            let window = webview.window();
            let Some(path) = app
                .dialog()
                .file()
                .set_parent(&window)
                .set_file_name(&name)
                .blocking_save_file()
            else {
                diagnostic("dialog-cancelled");
                return Ok(SaveOutcome::Cancelled);
            };
            diagnostic("dialog-selected");
            (
                noted(
                    path.into_path().map_err(|_| "另存为路径无效".into()),
                    "dialog-path-invalid",
                )?,
                true,
            )
        }
        _ => {
            diagnostic("mode-rejected");
            return Err("下载模式无效".into());
        }
    };
    let cookies = noted(
        webview
            .cookies()
            .map_err(|_| "无法读取节点会话 Cookie".into()),
        "cookie-error",
    )?;
    let cookies = filter_cookies_for_source(cookies, &source);
    diagnostic(if cookies.is_empty() {
        "cookie-empty"
    } else {
        "cookie-ready"
    });
    let cookie = if cookies.is_empty() {
        None
    } else {
        Some(
            cookies
                .iter()
                .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
                .collect::<Vec<_>>()
                .join("; "),
        )
    };
    diagnostic("transfer-start");
    download_to_path_guarded(
        &source,
        &origin,
        cookie.as_deref(),
        &destination,
        overwrite,
        &with_current,
    )?;
    Ok(SaveOutcome::Saved(destination))
}

fn filter_cookies_for_source(cookies: Vec<Cookie<'static>>, source: &Url) -> Vec<Cookie<'static>> {
    cookies
        .into_iter()
        .filter(|cookie| {
            cookie_domain_matches(cookie, source)
                && cookie_secure_matches(cookie, source)
                && cookie_path_matches(cookie, source)
        })
        .collect()
}

fn normalized_source_host(source: &Url) -> Option<String> {
    let host = source
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn cookie_domain_matches(cookie: &Cookie<'_>, source: &Url) -> bool {
    let Some(mut source_host) = normalized_source_host(source) else {
        return false;
    };
    // A missing Domain cannot be tied back to its host after reading the whole store.
    let Some(raw_domain) = cookie.domain() else {
        return false;
    };
    if raw_domain.is_empty() || raw_domain.starts_with('.') {
        return false;
    }
    let domain = raw_domain.trim_end_matches('.').to_ascii_lowercase();
    source_host = source_host.trim_end_matches('.').to_ascii_lowercase();
    if let Ok(source_ip) = source_host.parse::<IpAddr>() {
        return domain
            .parse::<IpAddr>()
            .is_ok_and(|cookie_ip| cookie_ip == source_ip);
    }
    if domain.parse::<IpAddr>().is_ok() {
        return false;
    }
    // The Cookie API strips a leading Domain dot, so subdomain scope cannot be
    // distinguished safely from host-only scope. Fail closed to an exact host.
    source_host == domain
}

fn cookie_secure_matches(cookie: &Cookie<'_>, source: &Url) -> bool {
    if !cookie.secure().unwrap_or(false) || source.scheme() == "https" {
        return true;
    }
    if source.scheme() != "http" {
        return false;
    }
    normalized_source_host(source).is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn cookie_path_matches(cookie: &Cookie<'_>, source: &Url) -> bool {
    let request_path = source.path();
    let cookie_path = cookie.path().unwrap_or("/");
    if cookie_path == "/" {
        return true;
    }
    if !cookie_path.starts_with('/') {
        return false;
    }
    request_path == cookie_path
        || request_path
            .strip_prefix(cookie_path)
            .is_some_and(|remainder| cookie_path.ends_with('/') || remainder.starts_with('/'))
}

fn validate_source(raw: &str, registered_origin: Option<&Url>) -> Result<Url, String> {
    let origin = registered_origin.ok_or("未知节点或连接已关闭")?;
    let source = Url::parse(raw).map_err(|_| "下载 URL 无效")?;
    if source.fragment().is_some() {
        return Err("下载 URL 不得包含 fragment".into());
    }
    for url in [origin, &source] {
        let loopback =
            [Some("127.0.0.1"), Some("localhost"), Some("[::1]")].contains(&url.host_str());
        if url.host_str().is_none()
            || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        {
            return Err("下载仅允许同源 HTTPS 或 loopback HTTP".into());
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err("下载 URL 不得包含用户名或密码".into());
        }
    }
    if !same_origin(origin, &source) {
        return Err("下载 URL 必须与当前节点同源".into());
    }
    if source.query_pairs().any(|(key, _)| {
        let normalized = key
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .map(|character| character.to_ascii_lowercase())
            .collect::<String>();
        [
            "token",
            "accesstoken",
            "refreshtoken",
            "idtoken",
            "authtoken",
            "password",
            "passwd",
            "pwd",
            "secret",
            "clientsecret",
            "authorization",
            "auth",
            "apikey",
            "credential",
            "credentials",
            "session",
            "sessionid",
            "cookie",
            "signature",
            "sig",
            "privatekey",
        ]
        .contains(&normalized.as_str())
    }) {
        return Err("下载 URL 不得携带凭据".into());
    }
    Ok(source)
}

fn safe_file_name(suggested: &str) -> String {
    if suggested
        .chars()
        .all(|character| character.is_whitespace() || character.is_control())
    {
        return "download".into();
    }
    let cleaned = suggested
        .chars()
        .map(|character| {
            if character == '/' || character == '\\' || character.is_control() {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "download".into()
    } else {
        cleaned.into()
    }
}

fn next_available(directory: &Path, name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 0..10_000_u32 {
        let candidate_name = if index == 0 {
            name.to_owned()
        } else {
            match extension {
                Some(extension) => format!("{stem} ({index}).{extension}"),
                None => format!("{stem} ({index})"),
            }
        };
        let candidate = directory.join(candidate_name);
        match fs::symlink_metadata(&candidate) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Err(_) => return Err("无法检查下载目标是否已存在".into()),
        }
    }
    Err("无法分配不重复的下载文件名".into())
}

fn download(
    source: &Url,
    origin: &Url,
    cookie: Option<&str>,
    directory: &Path,
    suggested_name: &str,
) -> Result<PathBuf, String> {
    let destination = next_available(directory, &safe_file_name(suggested_name))?;
    download_to_path(source, origin, cookie, &destination, false)?;
    Ok(destination)
}

fn download_to_path(
    source: &Url,
    origin: &Url,
    cookie: Option<&str>,
    destination: &Path,
    overwrite: bool,
) -> Result<(), String> {
    download_to_path_guarded(source, origin, cookie, destination, overwrite, &|action| {
        action()
    })
}

fn download_to_path_guarded(
    source: &Url,
    origin: &Url,
    cookie: Option<&str>,
    destination: &Path,
    overwrite: bool,
    with_current: &dyn Fn(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
) -> Result<(), String> {
    with_current(&mut || Ok(()))?;
    let source = noted(
        validate_source(source.as_str(), Some(origin)),
        "source-rejected",
    )?;
    let directory = noted(
        destination.parent().ok_or("下载目标目录无效".into()),
        "target-invalid",
    )?;
    let mut temporary = noted(
        NamedTempFile::with_prefix_in(".dsh-download-", directory)
            .map_err(|_| "无法在下载目录创建临时文件".into()),
        "temp-error",
    )?;
    let result = stream_download(
        &source,
        origin,
        cookie,
        temporary.as_file_mut(),
        with_current,
    );
    if result.is_ok() {
        noted(
            temporary
                .as_file_mut()
                .sync_all()
                .map_err(|_| "无法同步下载临时文件".into()),
            "sync-error",
        )?;
        // Owner validation and the visible write share one registry critical section.
        with_current(&mut || {
            noted(
                atomic_rename(temporary.path(), destination, overwrite),
                "rename-error",
            )
        })?;
        diagnostic("commit-complete");
    }
    result
}

fn stream_download(
    source: &Url,
    origin: &Url,
    cookie: Option<&str>,
    output: &mut fs::File,
    with_current: &dyn Fn(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
) -> Result<(), String> {
    let redirect_origin = origin.clone();
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(Policy::custom(move |attempt| {
            if !same_origin(&redirect_origin, attempt.url()) {
                diagnostic("redirect-origin-rejected");
                attempt.error("下载重定向不得跨 origin")
            } else if attempt.previous().len() >= MAX_REDIRECTS {
                diagnostic("redirect-limit");
                attempt.error("下载重定向次数过多")
            } else {
                attempt.follow()
            }
        }))
        .no_proxy()
        .build();
    let client = noted(
        client.map_err(|_| "无法创建下载请求".into()),
        "client-error",
    )?;

    let mut offset = 0_u64;
    let mut expected_total = None;
    loop {
        with_current(&mut || Ok(()))?;
        let block_end = offset.checked_add(CHUNK_SIZE - 1).ok_or("下载文件过大")?;
        let requested_end = expected_total.map_or(block_end, |total: u64| block_end.min(total - 1));
        let mut request = client
            .get(source.clone())
            .header(RANGE, format!("bytes={offset}-{requested_end}"));
        if let Some(cookie) = cookie {
            request = request.header(COOKIE, cookie);
        }
        let response = noted(
            request.send().map_err(|_| "下载请求失败".into()),
            "response-request-error",
        )?;
        match response.status() {
            StatusCode::OK if offset == 0 => {
                copy_bounded(response, output, None, with_current)?;
                return Ok(());
            }
            StatusCode::PARTIAL_CONTENT => {
                let content_range = noted(
                    response
                        .headers()
                        .get(CONTENT_RANGE)
                        .ok_or("206 响应缺少 Content-Range".into())
                        .and_then(|value| value.to_str().map_err(|_| "Content-Range 无效".into())),
                    "response-range-error",
                )?;
                let (actual_end, total) = noted(
                    parse_content_range(content_range, offset, requested_end),
                    "response-range-error",
                )?;
                if expected_total.is_some_and(|expected| expected != total) {
                    diagnostic("response-total-changed");
                    return Err("下载文件总长度在分块间变化".into());
                }
                expected_total = Some(total);
                copy_bounded(
                    response,
                    output,
                    Some(actual_end - offset + 1),
                    with_current,
                )?;
                if actual_end + 1 == total {
                    return Ok(());
                }
                offset = actual_end + 1;
            }
            _ => {
                diagnostic("response-status-error");
                return Err("下载服务返回了不符合请求范围的响应".into());
            }
        }
    }
}

fn parse_content_range(
    value: &str,
    expected_start: u64,
    requested_end: u64,
) -> Result<(u64, u64), String> {
    let range = value
        .strip_prefix("bytes ")
        .ok_or("Content-Range 单位无效")?;
    let (range, total) = range.split_once('/').ok_or("Content-Range 无效")?;
    let (start, end) = range.split_once('-').ok_or("Content-Range 无效")?;
    let start = start.parse::<u64>().map_err(|_| "Content-Range 起点无效")?;
    let end = end.parse::<u64>().map_err(|_| "Content-Range 终点无效")?;
    let total = total.parse::<u64>().map_err(|_| "Content-Range 总长无效")?;
    if start != expected_start || end < start || end > requested_end {
        return Err("Content-Range 与请求范围不一致".into());
    }
    if total <= end {
        return Err("Content-Range 总长无效".into());
    }
    Ok((end, total))
}

fn copy_bounded(
    mut response: Response,
    output: &mut fs::File,
    expected_length: Option<u64>,
    with_current: &dyn Fn(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
) -> Result<(), String> {
    let mut buffer = [0_u8; READ_BUFFER_SIZE];
    let mut written = 0_u64;
    loop {
        with_current(&mut || Ok(()))?;
        let count = noted(
            response
                .read(&mut buffer)
                .map_err(|error| format!("下载读取失败：{error}")),
            "response-read-error",
        )?;
        if count == 0 {
            break;
        }
        written = written.checked_add(count as u64).ok_or("下载文件过大")?;
        if expected_length.is_some_and(|expected| written > expected) {
            diagnostic("response-length-error");
            return Err("服务返回了超出 Range 的数据".into());
        }
        noted(
            output
                .write_all(&buffer[..count])
                .map_err(|_| "下载写入失败".into()),
            "write-error",
        )?;
    }
    if expected_length.is_some_and(|expected| expected != written) {
        diagnostic("response-short-read");
        return Err("下载响应短读".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn atomic_rename(source: &Path, destination: &Path, overwrite: bool) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn renamex_np(from: *const i8, to: *const i8, flags: u32) -> i32;
    }
    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| "临时路径无效")?;
    let destination =
        CString::new(destination.as_os_str().as_bytes()).map_err(|_| "下载路径无效")?;
    let flags = if overwrite { 0 } else { RENAME_EXCL };
    if unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err("无法原子保存下载文件；目标可能已存在".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        collections::HashMap,
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        path::Path,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };
    use tempfile::TempDir;

    const TEST_COOKIE: &str = "dsh_test_session=synthetic-only";

    #[derive(Clone, Debug)]
    struct FixtureRequest {
        headers: HashMap<String, String>,
    }

    impl FixtureRequest {
        fn range(&self) -> Option<(u64, u64)> {
            let value = self.headers.get("range")?;
            let value = value.strip_prefix("bytes=")?;
            let (start, end) = value.split_once('-')?;
            Some((start.parse().ok()?, end.parse().ok()?))
        }
    }

    struct FixtureResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl FixtureResponse {
        fn new(status: u16, body: Vec<u8>) -> Self {
            Self {
                status,
                headers: vec![],
                body,
            }
        }

        fn header(mut self, name: &str, value: String) -> Self {
            self.headers.push((name.to_owned(), value));
            self
        }
    }

    struct TestServer {
        origin: Url,
        requests: Arc<Mutex<Vec<FixtureRequest>>>,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn start<F>(handler: F) -> Self
        where
            F: Fn(&FixtureRequest) -> FixtureResponse + Send + 'static,
        {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let origin = Url::parse(&format!("http://{address}/")).unwrap();
            let requests = Arc::new(Mutex::new(vec![]));
            let recorded = requests.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let worker = thread::spawn(move || {
                while !stopped.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // Accepted sockets inherit the listener's nonblocking flag on macOS.
                            stream.set_nonblocking(false).unwrap();
                            let request = read_request(&mut stream);
                            recorded.lock().unwrap().push(request.clone());
                            write_response(&mut stream, handler(&request));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                origin,
                requests,
                stop,
                worker: Some(worker),
            }
        }

        fn url(&self) -> Url {
            self.origin.join("artifact.apk").unwrap()
        }

        fn requests(&self) -> Vec<FixtureRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> FixtureRequest {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = vec![];
        let mut buffer = [0_u8; 4096];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0, "client closed before sending request headers");
            bytes.extend_from_slice(&buffer[..count]);
        }
        let text = String::from_utf8(bytes).unwrap();
        let mut headers = HashMap::new();
        for line in text.split("\r\n").skip(1) {
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
            }
        }
        FixtureRequest { headers }
    }

    fn write_response(stream: &mut TcpStream, response: FixtureResponse) {
        let reason = match response.status {
            200 => "OK",
            206 => "Partial Content",
            302 => "Found",
            _ => "Fixture",
        };
        let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, reason);
        for (name, value) in response.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            response.body.len()
        ));
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&response.body).unwrap();
        stream.flush().unwrap();
    }

    fn synthetic_bytes(offset: u64, length: usize) -> Vec<u8> {
        (0..length)
            .map(|index| {
                let position = offset + index as u64;
                ((position * 31 + position / 251 + 17) & 0xff) as u8
            })
            .collect()
    }

    fn partial_response(request: &FixtureRequest, total: u64) -> FixtureResponse {
        let (start, end) = request.range().expect("Range header required");
        let length = (end - start + 1) as usize;
        FixtureResponse::new(206, synthetic_bytes(start, length))
            .header("Content-Range", format!("bytes {start}-{end}/{total}"))
    }

    fn hash_file(path: &Path) -> String {
        let mut file = fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        format!("{:x}", hash.finalize())
    }

    fn synthetic_hash(length: u64) -> String {
        let mut hash = Sha256::new();
        let mut offset = 0;
        while offset < length {
            let count = ((length - offset) as usize).min(64 * 1024);
            hash.update(synthetic_bytes(offset, count));
            offset += count as u64;
        }
        format!("{:x}", hash.finalize())
    }

    fn assert_failure_left_no_files(directory: &Path) {
        let entries = fs::read_dir(directory).unwrap();
        assert_eq!(entries.count(), 0, "failed download left files behind");
    }

    const NODE_ID: &str = "123e4567-e89b-42d3-a456-426614174000";

    fn node_state() -> (Connections, Arc<crate::tabs::TabSession>, Url) {
        let state = Connections::default();
        let origin = Url::parse("https://node.example:8443/").unwrap();
        let session = state.tabs.lock().unwrap().begin(NODE_ID, "Node", 7);
        state
            .urls
            .lock()
            .unwrap()
            .insert(NODE_ID.into(), origin.clone());
        (state, session, origin)
    }

    #[test]
    fn diagnostics_are_fixed_bounded_stage_messages() {
        let message = diagnostic_message("command-start");
        assert_eq!(message, "[dsh-download] command-start");
        assert!(message.len() < 64);
    }

    #[test]
    fn javascript_title_diagnostics_accept_only_recognized_fixed_stages() {
        for stage in [
            "bridge-existing",
            "bridge-ipc-unavailable",
            "bridge-install",
            "save-request",
            "save-request-rejected",
            "save-url-rejected",
            "save-mode-rejected",
            "save-field-rejected",
            "save-ipc-start",
            "save-ipc-saved",
            "save-ipc-cancelled",
            "save-ipc-invalid",
            "save-ipc-acl-rejected",
            "save-ipc-error",
            "bridge-installed",
            "bridge-missing",
            "ipc-unavailable",
        ] {
            let title = format!("[dsh-download] {stage}");
            assert_eq!(diagnostic_title_stage(&title), Some(stage));
            assert!(title.len() < 64);
        }
        for title in [
            "ORIGINAL_TITLE_SENTINEL",
            "[dsh-download] URL_SENTINEL",
            "[dsh-download] bridge-install COOKIE_SENTINEL",
            "[dsh-download] bridge-install\nPATH_SENTINEL",
            "prefix [dsh-download] bridge-install",
        ] {
            assert_eq!(diagnostic_title_stage(title), None);
        }
    }

    #[test]
    fn generated_page_load_probe_checks_only_booleans_and_restores_title() {
        let script = file_actions_probe_script();
        for required in [
            "typeof window.dshDesktop?.fileActions?.save === 'function'",
            "typeof window.__TAURI_INTERNALS__?.invoke === 'function'",
            "'bridge-installed'",
            "'bridge-missing'",
            "'ipc-unavailable'",
            "document.title = diagnostic",
            "if (document.title === diagnostic) document.title = title",
            "}, 50)",
            "setTimeout(report, 100)",
        ] {
            assert!(script.contains(required), "probe missing {required}");
        }
        for forbidden in [
            "location", "cookie", "token", "secret", "password", "console", "JSON", "invoke(",
            "save(",
        ] {
            assert!(!script.contains(forbidden), "probe contains {forbidden}");
        }
    }

    #[test]
    fn remote_node_content_can_invoke_only_the_download_command() {
        let mut authority = tauri::runtime_authority!(
            std::collections::BTreeMap::new(),
            tauri::utils::acl::resolved::Resolved::default()
        );
        authorize_save_download(&mut authority);
        let label = format!("dsh-{NODE_ID}");
        for url in [
            "https://node.example:8443/files",
            "https://node.example/files",
            "http://127.0.0.1:41234/files",
        ] {
            let origin = tauri::ipc::Origin::Remote {
                url: Url::parse(url).unwrap(),
            };
            assert!(
                authority
                    .resolve_access("save_download", "main", &label, &origin)
                    .is_some(),
                "the real remote content WebView must pass the IPC ACL"
            );
            assert!(
                authority
                    .resolve_access("open_node", "main", &label, &origin)
                    .is_none(),
                "remote node content must not gain shell commands"
            );
        }
    }

    #[test]
    fn node_content_identity_requires_a_registered_exact_node_label() {
        let (state, _session, origin) = node_state();
        let label = format!("dsh-{NODE_ID}");
        assert!(
            require_node_content_state(&state, &label, "main", &origin).is_ok(),
            "macOS child content WebView must be accepted"
        );
        assert!(
            require_node_content_state(&state, &label, &label, &origin).is_ok(),
            "an independently owned node window must be accepted"
        );
        for (webview, window) in [
            ("main", "main"),
            (&format!("clear-{NODE_ID}"), "main"),
            (&label, "other"),
            ("dsh-not-a-uuid", "main"),
            ("dsh-123e4567-e89b-42d3-a456-426614174001", "main"),
        ] {
            assert!(
                require_node_content_state(&state, webview, window, &origin).is_err(),
                "accepted unregistered identity {webview}/{window}"
            );
        }
    }

    #[test]
    fn closed_replaced_or_new_generation_owner_invalidates_pending_save() {
        let (state, session, origin) = node_state();
        let label = format!("dsh-{NODE_ID}");
        let owner = require_node_content_state(&state, &label, "main", &origin).unwrap();
        assert!(owner.is_current(&state).is_ok());

        state.tabs.lock().unwrap().mark_failed(&session, 7);
        state.tabs.lock().unwrap().begin(NODE_ID, "Node", 8);
        assert!(
            owner.is_current(&state).is_err(),
            "a previous connection generation must not finish a save"
        );

        let (state, session, origin) = node_state();
        let owner = require_node_content_state(&state, &label, "main", &origin).unwrap();
        state.tabs.lock().unwrap().close(NODE_ID);
        assert!(
            owner.is_current(&state).is_err(),
            "closing a node must cancel its save"
        );

        // Even a reused numeric generation cannot make a different owner current.
        state.tabs.lock().unwrap().begin(NODE_ID, "Node", 7);
        assert!(!Arc::ptr_eq(
            &session,
            &state.tabs.lock().unwrap().get(NODE_ID).unwrap()
        ));
        assert!(owner.is_current(&state).is_err());
    }

    #[test]
    fn native_request_rejects_extra_path_and_credential_fields() {
        for field in ["path", "token", "cookie"] {
            let value = serde_json::json!({
                "url": "https://node.example:8443/file",
                "mode": "download",
                field: "must-not-cross-ipc"
            });
            assert!(serde_json::from_value::<DownloadRequest>(value).is_err());
        }
    }

    #[test]
    fn save_outcome_serializes_the_web_contract() {
        assert_eq!(
            serde_json::to_value(SaveOutcome::Saved(PathBuf::from(
                "/Users/test/Downloads/a.apk"
            )))
            .unwrap(),
            serde_json::json!({"status":"saved","path":"/Users/test/Downloads/a.apk"})
        );
        assert_eq!(
            serde_json::to_value(SaveOutcome::Cancelled).unwrap(),
            serde_json::json!({"status":"cancelled"})
        );
    }

    #[test]
    fn final_commit_holds_registry_lock_while_close_waits_on_the_barrier() {
        let (state, session, origin) = node_state();
        let state = Arc::new(state);
        let label = format!("dsh-{NODE_ID}");
        let owner = require_node_content_state(&state, &label, "main", &origin).unwrap();
        let directory = TempDir::new().unwrap();
        let mut temporary = NamedTempFile::new_in(directory.path()).unwrap();
        temporary.write_all(b"new").unwrap();
        let destination = directory.path().join("save-as.apk");
        fs::write(&destination, b"old").unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let close_state = state.clone();
        let close_barrier = barrier.clone();
        let closer = thread::spawn(move || {
            close_barrier.wait();
            close_state.tabs.lock().unwrap().close(NODE_ID);
        });

        owner
            .with_current(&state, &mut || {
                // The closer is released only while the commit critical section runs.
                barrier.wait();
                assert!(
                    state.tabs.try_lock().is_err(),
                    "with_current released the registry lock before atomic rename"
                );
                assert!(!session.cancelled.load(Ordering::SeqCst));
                atomic_rename(temporary.path(), &destination, true)
            })
            .unwrap();
        closer.join().unwrap();
        assert!(session.cancelled.load(Ordering::SeqCst));
        assert_eq!(fs::read(destination).unwrap(), b"new");
    }

    #[test]
    fn owner_loss_at_final_check_cleans_temp_without_committing_destination() {
        let server = TestServer::start(|_| FixtureResponse::new(200, b"complete".to_vec()));
        let directory = TempDir::new().unwrap();
        let destination = directory.path().join("stale.apk");
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let result = download_to_path_guarded(
            &server.url(),
            &server.origin,
            None,
            &destination,
            false,
            &|action| {
                // 200 response: pre-temp, request, two bounded reads, then final commit.
                if checks.fetch_add(1, Ordering::SeqCst) == 4 {
                    Err("节点已关闭或已替换".into())
                } else {
                    action()
                }
            },
        );
        assert!(result.is_err());
        assert!(!destination.exists());
        assert_failure_left_no_files(directory.path());
    }

    #[test]
    fn validate_source_rejects_all_fragments_including_credential_lookalikes() {
        let origin = Url::parse("https://node.example:8443/").unwrap();
        for suffix in ["#access_token=FRAGMENT_SENTINEL", "#"] {
            let url = format!("https://node.example:8443/file{suffix}");
            assert!(validate_source(&url, Some(&origin)).is_err());
        }
    }

    fn fixture_cookie(
        name: &'static str,
        domain: Option<&'static str>,
        path: Option<&'static str>,
        secure: bool,
    ) -> tauri::webview::Cookie<'static> {
        let mut builder = tauri::webview::Cookie::build((name, "synthetic")).secure(secure);
        if let Some(domain) = domain {
            builder = builder.domain(domain);
        }
        if let Some(path) = path {
            builder = builder.path(path);
        }
        builder.build()
    }

    fn filtered_cookie_names(
        cookies: Vec<tauri::webview::Cookie<'static>>,
        source: &str,
    ) -> Vec<String> {
        filter_cookies_for_source(cookies, &Url::parse(source).unwrap())
            .into_iter()
            .map(|cookie| cookie.name().to_owned())
            .collect()
    }

    #[test]
    fn cookie_filter_accepts_exact_loopback_ip_and_localhost_only() {
        let cookies = vec![
            fixture_cookie("ip", Some("127.0.0.1"), Some("/"), false),
            fixture_cookie("other-ip", Some("127.0.0.2"), Some("/"), false),
            fixture_cookie("localhost", Some("localhost"), Some("/"), false),
            fixture_cookie("deceptive", Some("127.0.0.1.example.com"), Some("/"), false),
            fixture_cookie("host-only", None, Some("/"), false),
        ];
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "http://127.0.0.1:3080/files/a.apk"),
            ["ip"]
        );
        assert_eq!(
            filtered_cookie_names(cookies, "http://localhost:3080/files/a.apk"),
            ["localhost"]
        );
    }

    #[test]
    fn cookie_filter_does_not_suffix_match_unrelated_domains() {
        let cookies = vec![
            fixture_cookie("exact-node", Some("node.example.com"), Some("/"), false),
            fixture_cookie("exact-parent", Some("example.com"), Some("/"), false),
            fixture_cookie("dotted-parent", Some(".example.com"), Some("/"), false),
            fixture_cookie("unrelated", Some("example.com.evil"), Some("/"), false),
            fixture_cookie("dotted-other", Some(".other.example"), Some("/"), false),
            fixture_cookie("public-suffix", Some(".com"), Some("/"), false),
            fixture_cookie("dotted-localhost", Some(".localhost"), Some("/"), false),
            fixture_cookie("host-only", None, Some("/"), false),
        ];
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "https://node.example.com/files/a.apk"),
            ["exact-node"]
        );
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "https://example.com/files/a.apk"),
            ["exact-parent", "dotted-parent"]
        );
        assert!(
            filtered_cookie_names(cookies.clone(), "https://badexample.com/files/a.apk").is_empty()
        );
        assert!(filtered_cookie_names(cookies, "http://sub.localhost/files/a.apk").is_empty());
    }

    #[test]
    fn cookie_filter_enforces_secure_and_preserves_loopback_exception() {
        let cookies = vec![
            fixture_cookie("secure", Some("node.example.com"), Some("/"), true),
            fixture_cookie("plain", Some("node.example.com"), Some("/"), false),
            fixture_cookie("secure-ip", Some("127.0.0.1"), Some("/"), true),
            fixture_cookie("secure-local", Some("localhost"), Some("/"), true),
        ];
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "https://node.example.com/files/a.apk"),
            ["secure", "plain"]
        );
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "http://node.example.com/files/a.apk"),
            ["plain"]
        );
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "http://127.0.0.1:3080/files/a.apk"),
            ["secure-ip"]
        );
        assert_eq!(
            filtered_cookie_names(cookies, "http://localhost:3080/files/a.apk"),
            ["secure-local"]
        );
    }

    #[test]
    fn cookie_filter_enforces_rfc_path_boundaries() {
        let cookies = vec![
            fixture_cookie("default", Some("node.example.com"), None, false),
            fixture_cookie("root", Some("node.example.com"), Some("/"), false),
            fixture_cookie("files", Some("node.example.com"), Some("/files"), false),
            fixture_cookie(
                "files-slash",
                Some("node.example.com"),
                Some("/files/"),
                false,
            ),
            fixture_cookie("file", Some("node.example.com"), Some("/file"), false),
            fixture_cookie("other", Some("node.example.com"), Some("/other"), false),
            fixture_cookie("relative", Some("node.example.com"), Some("files"), false),
        ];
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "https://node.example.com/files/a.apk"),
            ["default", "root", "files", "files-slash"]
        );
        assert_eq!(
            filtered_cookie_names(cookies.clone(), "https://node.example.com/files2/a.apk"),
            ["default", "root"]
        );
        assert_eq!(
            filtered_cookie_names(cookies, "https://node.example.com/files"),
            ["default", "root", "files"]
        );
    }

    #[test]
    fn validate_source_accepts_only_registered_safe_origins() {
        let https = Url::parse("https://node.example:8443/root/").unwrap();
        assert_eq!(
            validate_source("https://node.example:8443/files/a.apk", Some(&https))
                .unwrap()
                .as_str(),
            "https://node.example:8443/files/a.apk"
        );
        let loopback = Url::parse("http://127.0.0.1:3080/").unwrap();
        assert!(validate_source("http://127.0.0.1:3080/file", Some(&loopback)).is_ok());
        assert!(validate_source("https://other.example/file", Some(&https)).is_err());
        assert!(validate_source("https://node.example:9443/file", Some(&https)).is_err());
        assert!(validate_source("https://user@node.example:8443/file", Some(&https)).is_err());
        assert!(validate_source("https://user:pw@node.example:8443/file", Some(&https)).is_err());
        assert!(validate_source("file:///tmp/a.apk", Some(&https)).is_err());
        assert!(
            validate_source("https://node.example:8443/file?token=secret", Some(&https)).is_err()
        );
        assert!(validate_source("https://node.example:8443/file", None).is_err());
        let remote_http = Url::parse("http://node.example/").unwrap();
        assert!(validate_source("http://node.example/file", Some(&remote_http)).is_err());
    }

    #[test]
    fn validate_source_rejects_normalized_common_credential_query_names() {
        let origin = Url::parse("https://node.example:8443/").unwrap();
        for key in [
            "access_token",
            "ACCESS-TOKEN",
            "Access.Token",
            "api_key",
            "API-KEY",
            "Api.Key",
            "auth",
            "AUTH",
            "client_secret",
            "refresh_token",
            "id_token",
            "auth_token",
            "credential",
            "session_id",
            "cookie",
            "signature",
            "private_key",
            "access%5Ftoken",
        ] {
            let url = format!("https://node.example:8443/file?{key}=synthetic");
            assert!(
                validate_source(&url, Some(&origin)).is_err(),
                "accepted {key}"
            );
        }
        assert!(
            validate_source(
                "https://node.example:8443/file?name=a.apk&download=1",
                Some(&origin)
            )
            .is_ok()
        );
    }

    #[test]
    fn safe_file_name_removes_paths_controls_and_special_names() {
        assert_eq!(safe_file_name("a.apk"), "a.apk");
        assert_eq!(safe_file_name(".apk"), ".apk");
        assert_eq!(safe_file_name("folder/a\\b.apk"), "folder_a_b.apk");
        assert_eq!(safe_file_name("bad\u{0}\n.apk"), "bad__.apk");
        assert_eq!(safe_file_name("."), "download");
        assert_eq!(safe_file_name(".."), "download");
        assert_eq!(safe_file_name(" \t\r\n"), "download");
        assert_eq!(safe_file_name(""), "download");
    }

    #[test]
    fn next_available_never_selects_an_existing_name_or_symlink() {
        let directory = TempDir::new().unwrap();
        let first = next_available(directory.path(), "a.apk").unwrap();
        assert_eq!(first, directory.path().join("a.apk"));
        fs::write(&first, b"original").unwrap();
        let second = next_available(directory.path(), "a.apk").unwrap();
        assert_eq!(second, directory.path().join("a (1).apk"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("missing-target", directory.path().join("link.apk"))
                .unwrap();
            assert_eq!(
                next_available(directory.path(), "link.apk").unwrap(),
                directory.path().join("link (1).apk")
            );
        }
        assert_eq!(fs::read(&first).unwrap(), b"original");
    }

    #[test]
    fn range_download_streams_over_32_mib_with_exact_hash_offsets_and_cookie() {
        let length = 32 * 1024 * 1024 + 12_345;
        let server = TestServer::start(move |request| partial_response(request, length));
        let directory = TempDir::new().unwrap();
        let saved = download(
            &server.url(),
            &server.origin,
            Some(TEST_COOKIE),
            directory.path(),
            "large.apk",
        )
        .unwrap();
        assert_eq!(fs::metadata(&saved).unwrap().len(), length);
        assert_eq!(hash_file(&saved), synthetic_hash(length));
        let requests = server.requests();
        let expected_ranges: Vec<_> = (0..length)
            .step_by(1024 * 1024)
            .map(|start| (start, (start + 1024 * 1024 - 1).min(length - 1)))
            .collect();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.range())
                .collect::<Vec<_>>(),
            expected_ranges.into_iter().map(Some).collect::<Vec<_>>()
        );
        assert_eq!(requests.last().unwrap().range().unwrap().1, length - 1);
        assert!(requests.iter().all(|request| {
            request.headers.get("cookie").map(String::as_str) == Some(TEST_COOKIE)
        }));
    }

    #[test]
    fn small_legal_206_response_uses_total_bounded_end_and_finishes() {
        let server = TestServer::start(|request| {
            assert_eq!(request.range(), Some((0, 1_048_575)));
            FixtureResponse::new(206, synthetic_bytes(0, 1000))
                .header("Content-Range", "bytes 0-999/1000".into())
        });
        let directory = TempDir::new().unwrap();
        let saved = download(
            &server.url(),
            &server.origin,
            None,
            directory.path(),
            "small.apk",
        )
        .unwrap();
        assert_eq!(fs::read(saved).unwrap(), synthetic_bytes(0, 1000));
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn shorter_than_requested_ranges_advance_by_actual_end() {
        let length = 2500;
        let server = TestServer::start(move |request| {
            let (start, _) = request.range().unwrap();
            let actual_end = (start + 99).min(length - 1);
            FixtureResponse::new(
                206,
                synthetic_bytes(start, (actual_end - start + 1) as usize),
            )
            .header(
                "Content-Range",
                format!("bytes {start}-{actual_end}/{length}"),
            )
        });
        let directory = TempDir::new().unwrap();
        let saved = download(
            &server.url(),
            &server.origin,
            None,
            directory.path(),
            "short-ranges.apk",
        )
        .unwrap();
        assert_eq!(hash_file(&saved), synthetic_hash(length));
        let requests = server.requests();
        let expected: Vec<_> = (0..length)
            .step_by(100)
            .map(|start| {
                let requested_end = if start == 0 { 1_048_575 } else { length - 1 };
                Some((start, requested_end))
            })
            .collect();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.range())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(requests.len(), 25);
    }

    #[test]
    fn initial_200_is_accepted_when_server_ignores_range() {
        let server = TestServer::start(|_| FixtureResponse::new(200, b"complete".to_vec()));
        let directory = TempDir::new().unwrap();
        let saved = download(
            &server.url(),
            &server.origin,
            None,
            directory.path(),
            "complete.apk",
        )
        .unwrap();
        assert_eq!(fs::read(saved).unwrap(), b"complete");
        assert_eq!(server.requests()[0].range(), Some((0, 1_048_575)));
    }

    #[test]
    fn later_200_fails_and_removes_partial_file() {
        let length = 1024 * 1024 + 1;
        let server = TestServer::start(move |request| {
            if request.range().unwrap().0 == 0 {
                partial_response(request, length)
            } else {
                FixtureResponse::new(200, b"restarted".to_vec())
            }
        });
        let directory = TempDir::new().unwrap();
        assert!(
            download(
                &server.url(),
                &server.origin,
                None,
                directory.path(),
                "bad.apk",
            )
            .is_err()
        );
        assert_failure_left_no_files(directory.path());
    }

    #[test]
    fn wrong_content_range_fails_and_removes_partial_file() {
        let server = TestServer::start(|_| {
            FixtureResponse::new(206, b"x".to_vec()).header("Content-Range", "bytes 1-1/2".into())
        });
        let directory = TempDir::new().unwrap();
        assert!(
            download(
                &server.url(),
                &server.origin,
                None,
                directory.path(),
                "bad.apk",
            )
            .is_err()
        );
        assert_failure_left_no_files(directory.path());
    }

    #[test]
    fn short_range_read_fails_and_removes_partial_file() {
        let server = TestServer::start(|_| {
            FixtureResponse::new(206, b"ab".to_vec()).header("Content-Range", "bytes 0-3/4".into())
        });
        let directory = TempDir::new().unwrap();
        assert!(
            download(
                &server.url(),
                &server.origin,
                None,
                directory.path(),
                "bad.apk",
            )
            .is_err()
        );
        assert_failure_left_no_files(directory.path());
    }

    #[test]
    fn cross_origin_redirect_is_rejected_without_leaking_cookie() {
        let target = TestServer::start(|_| FixtureResponse::new(200, b"unexpected".to_vec()));
        let target_url = target.url();
        let source = TestServer::start(move |_| {
            FixtureResponse::new(302, vec![]).header("Location", target_url.to_string())
        });
        let directory = TempDir::new().unwrap();
        assert!(
            download(
                &source.url(),
                &source.origin,
                Some(TEST_COOKIE),
                directory.path(),
                "bad.apk",
            )
            .is_err()
        );
        assert!(target.requests().is_empty());
        assert_failure_left_no_files(directory.path());
    }

    #[test]
    fn redirects_are_bounded_and_clean_up() {
        let server = TestServer::start(|request| {
            let _ = request;
            FixtureResponse::new(302, vec![]).header("Location", "/artifact.apk".into())
        });
        let directory = TempDir::new().unwrap();
        assert!(
            download(
                &server.url(),
                &server.origin,
                None,
                directory.path(),
                "bad.apk",
            )
            .is_err()
        );
        assert!((2..=6).contains(&server.requests().len()));
        assert_failure_left_no_files(directory.path());
    }
}
