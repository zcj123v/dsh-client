use crate::{Connections, policy::same_origin, tabs::TabSession};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use std::{
    ffi::CString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Weak, atomic::Ordering},
};
use tauri::{AppHandle, Manager, Url, Webview};
use tauri_plugin_dialog::DialogExt;
use tempfile::NamedTempFile;

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
        Url::parse(&request.url).map_err(|_| "下载 URL 无效".into()),
        "source-rejected",
    )?;
    let remote_path = noted(remote_path_from_url(&source), "source-rejected")?;
    let current = noted(
        webview.url().map_err(|_| "无法验证节点页面来源".into()),
        "save-page-url-error",
    )?;
    if !same_origin(&origin, &current) {
        diagnostic("save-page-origin-changed");
        return Err("节点页面来源已变化，请重试".into());
    }
    let target = noted(
        app.state::<super::Connections>()
            .ssh
            .lock()
            .map_err(|_| "连接状态不可用".into())
            .and_then(|ssh| {
                ssh.get(id)
                    .cloned()
                    .ok_or("当前连接不是 SSH 转发，无法下载".into())
            }),
        "ssh-target-missing",
    )?;
    let args = noted(
        ssh_read_args(
            &target.profile,
            target.user.as_deref(),
            target.config.as_deref().and_then(|path| path.to_str()),
            &remote_path,
        ),
        "ssh-args-rejected",
    )?;
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
    diagnostic("transfer-start");
    let directory = noted(
        destination.parent().ok_or("下载目标目录无效".into()),
        "target-invalid",
    )?;
    let mut temporary = noted(
        NamedTempFile::with_prefix_in(".dsh-download-", directory)
            .map_err(|_| "无法在下载目录创建临时文件".into()),
        "temp-error",
    )?;
    noted(
        stream_ssh_to_file(&args, temporary.as_file_mut()),
        "transfer-error",
    )?;
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
            atomic_rename(temporary.path(), &destination, overwrite),
            "rename-error",
        )
    })?;
    diagnostic("commit-complete");
    Ok(SaveOutcome::Saved(destination))
}

/// Per-node SSH endpoint recorded when its forwarding tunnel is created.
/// `save_download` reuses it to stream remote files over the same connection.
#[derive(Clone, Debug)]
pub struct SshTarget {
    pub profile: String,
    pub user: Option<String>,
    pub config: Option<PathBuf>,
}

pub fn resolve_ssh_config(
    app: &AppHandle,
    ssh_config: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    map_ssh_config(app.path().home_dir().ok(), ssh_config)
}

fn map_ssh_config(
    app_home: Option<PathBuf>,
    option: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    match option {
        None | Some("default") => Ok(None),
        Some("none") => Ok(Some(PathBuf::from("/dev/null"))),
        Some("fleet") => match app_home {
            Some(home) => Ok(Some(home.join(".ssh/dsh-fleet-config"))),
            None => Err("无法定位 SSH 配置".into()),
        },
        _ => Err("SSH 配置无效".into()),
    }
}

fn remote_path_from_url(url: &Url) -> Result<String, String> {
    let path = url
        .query_pairs()
        .find_map(|(key, value)| {
            if key == "path" {
                Some(value.into_owned())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if path.is_empty() || !path.starts_with('/') {
        return Err("远端路径无效".into());
    }
    Ok(path)
}

fn ssh_read_args(
    profile: &str,
    user: Option<&str>,
    config: Option<&str>,
    path: &str,
) -> Result<Vec<String>, String> {
    // Mirror the `safe` closure rules of policy::ssh_args for profile/user;
    // ssh_args itself is not reused because it assembles tunnel arguments.
    let safe = |s: &str| {
        !s.is_empty()
            && s.len() < 256
            && s.as_bytes()[0].is_ascii_alphanumeric()
            && s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._@:-".contains(&b))
    };
    if !safe(profile) || user.is_some_and(|u| !safe(u) || u.contains('@') || u.contains(':')) {
        return Err("SSH Host 或用户无效".into());
    }
    let mut args = Vec::new();
    if let Some(config) = config {
        args.extend(["-F".into(), config.into()]);
    }
    args.extend(["-o".into(), "BatchMode=yes".into()]);
    args.extend(["-o".into(), "StrictHostKeyChecking=yes".into()]);
    if let Some(user) = user {
        args.extend(["-l".into(), user.into()]);
    }
    args.push(profile.into());
    // The first `--` ends ssh option parsing; `cat --` keeps a leading `-`
    // out of cat's option parsing. The remote path stays a single argument.
    args.extend(["--".into(), "cat".into(), "--".into(), path.into()]);
    Ok(args)
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

fn stream_command_to_file(
    program: &str,
    args: &[String],
    output: &mut fs::File,
) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| format!("无法启动 {program}"))?;
    // Drain stderr on a side thread and discard it: a full stderr pipe would
    // block the remote side mid-stream, and its contents must never reach the
    // returned error messages.
    let stderr = child.stderr.take();
    let drain_stderr = std::thread::spawn(move || {
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_end(&mut Vec::new());
        }
    });
    let mut stdout = child.stdout.take().ok_or("远端读取失败".to_owned())?;
    std::io::copy(&mut stdout, output).map_err(|_| "远端读取失败".to_owned())?;
    let _ = drain_stderr.join();
    let status = child.wait().map_err(|_| "远端读取失败".to_owned())?;
    if !status.success() {
        return Err("远端读取失败".to_owned());
    }
    Ok(())
}

fn stream_ssh_to_file(args: &[String], output: &mut fs::File) -> Result<(), String> {
    stream_command_to_file("ssh", args, output)
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
    use std::{
        fs,
        io::Write,
        sync::{Arc, atomic::Ordering},
        thread,
    };
    use tempfile::TempDir;

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
    fn resolve_ssh_config_maps_each_option() {
        // tauri::test::mock_app requires the `test` feature; the mapping is a
        // pure function, so the contract stays hermetic without an app handle.
        let home = Some(PathBuf::from("/Users/dsh"));
        assert_eq!(map_ssh_config(home.clone(), None).unwrap(), None);
        assert_eq!(map_ssh_config(home.clone(), Some("default")).unwrap(), None);
        assert_eq!(
            map_ssh_config(home.clone(), Some("none")).unwrap(),
            Some(PathBuf::from("/dev/null"))
        );
        let fleet = map_ssh_config(home.clone(), Some("fleet"))
            .unwrap()
            .unwrap();
        assert_eq!(fleet, PathBuf::from("/Users/dsh/.ssh/dsh-fleet-config"));
        assert!(fleet.ends_with(".ssh/dsh-fleet-config"));
        assert!(map_ssh_config(home, Some("other")).is_err());
        assert!(map_ssh_config(None, Some("fleet")).is_err());
    }

    #[test]
    fn remote_path_from_url_extracts_absolute_path() {
        let url = Url::parse("http://127.0.0.1:40286/api/file?path=/Users/x/Workspace/a%20b.apk").unwrap();
        assert_eq!(remote_path_from_url(&url).unwrap(), "/Users/x/Workspace/a b.apk");
        for bad in ["http://127.0.0.1:40286/api/file", "http://127.0.0.1:40286/api/file?path=", "http://127.0.0.1:40286/api/file?path=relative/x.apk"] {
            assert!(remote_path_from_url(&Url::parse(bad).unwrap()).is_err(), "{bad}");
        }
    }

    #[test]
    fn ssh_read_args_passes_path_as_single_argument_after_double_dash() {
        let args = ssh_read_args("dsh-mac", Some("u"), Some("/tmp/ssh_config"), "/p/a b/--x.apk").unwrap();
        let dash = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[dash + 1], "cat");
        assert_eq!(args[dash + 2], "--");
        assert_eq!(args[dash + 3], "/p/a b/--x.apk");
        assert_eq!(args.iter().filter(|a| *a == "--").count(), 2);
        assert!(args.contains(&"-F".to_string()) && args.contains(&"/tmp/ssh_config".to_string()));
        assert!(args.contains(&"-l".to_string()) && args.contains(&"u".to_string()));
        assert!(args.contains(&"BatchMode=yes".to_string()));
        let no_config = ssh_read_args("dsh-mac", None, None, "/x").unwrap();
        assert!(!no_config.contains(&"-F".to_string()) && !no_config.contains(&"-l".to_string()));
    }

    #[test]
    fn stream_command_to_file_copies_stdout_and_reports_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("源 文件.bin");
        let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&src, &payload).unwrap();
        let mut out = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let path = src.to_str().unwrap();
        stream_command_to_file("sh", &["-c".into(), format!("cat -- '{}'", path)], out.as_file_mut()).unwrap();
        assert_eq!(fs::read(out.path()).unwrap(), payload);

        let mut out2 = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let err = stream_command_to_file("sh", &["-c".into(), "cat -- /nonexistent/file; exit 1".into()], out2.as_file_mut()).unwrap_err();
        assert!(!err.contains("No such file"), "remote stderr leaked: {err}");
        assert_eq!(fs::read(out2.path()).unwrap().len(), 0);
    }
}
