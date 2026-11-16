#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod endpoints;
mod policy;
#[cfg(not(target_os = "android"))]
mod recovery;
use policy::*;
use serde::Deserialize;
#[cfg(not(target_os = "android"))]
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::WebviewWindow;
#[cfg(not(target_os = "android"))]
use tauri::{Emitter, Manager};
#[cfg(not(target_os = "android"))]
use tauri::{WebviewUrl, WebviewWindowBuilder};
#[cfg(not(target_os = "android"))]
use tauri_plugin_opener::OpenerExt;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    id: String,
    name: String,
    mode: String,
    #[cfg(not(target_os = "android"))]
    endpoints: Option<Vec<endpoints::Endpoint>>,
    url: Option<String>,
    #[cfg(not(target_os = "android"))]
    ssh_profile: Option<String>,
    #[cfg(not(target_os = "android"))]
    ssh_user: Option<String>,
    #[cfg(not(target_os = "android"))]
    remote_port: u16,
    #[cfg(not(target_os = "android"))]
    ssh_config: Option<String>,
}
#[cfg(not(target_os = "android"))]
struct Tunnel {
    generation: u64,
    process: Child,
    command: Command,
    stopped: bool,
    activated: bool,
    exhausted: bool,
    callback: Option<Arc<dyn Fn(&str, u8) + Send + Sync>>,
}
#[cfg(not(target_os = "android"))]
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
#[cfg(not(target_os = "android"))]
type WindowTunnel = Arc<Mutex<Option<Weak<Mutex<Tunnel>>>>>;
#[cfg(not(target_os = "android"))]
#[derive(Default)]
struct Connections {
    window_tunnels: Mutex<HashMap<String, WindowTunnel>>,
    tunnels: Mutex<HashMap<String, Arc<Mutex<Tunnel>>>>,
    exiting: AtomicBool,
    urls: Mutex<HashMap<String, tauri::Url>>,
    selected_entries: Mutex<HashMap<String, usize>>,
    opening_nodes: Mutex<std::collections::HashSet<String>>,
}

fn require_shell(window: &WebviewWindow) -> Result<(), String> {
    let url = window.url().map_err(|_| "无法验证客户端来源")?;
    let local = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (["http", "https"].contains(&url.scheme()) && url.host_str() == Some("tauri.localhost"))
        || (cfg!(debug_assertions)
            && url.scheme() == "http"
            && url.host_str() == Some("localhost")
            && url.port() == Some(1420));
    if window.label() != "main" || !local {
        return Err("仅本地客户端壳可调用此操作".into());
    }
    Ok(())
}
#[cfg(not(target_os = "android"))]
fn kill_tunnel(child: &Arc<Mutex<Tunnel>>) {
    if let Ok(mut tunnel) = child.lock() {
        tunnel.stopped = true;
        let _ = tunnel.process.kill();
        let _ = tunnel.process.wait();
    }
}
#[cfg(not(target_os = "android"))]
fn stop_tunnel(app: &tauri::AppHandle, id: &str) {
    stop_tunnel_state(&app.state::<Connections>(), id, None);
}
#[cfg(not(target_os = "android"))]
fn stop_tunnel_state(state: &Connections, id: &str, expected: Option<&Arc<Mutex<Tunnel>>>) {
    if let Ok(mut tunnels) = state.tunnels.lock() {
        if expected.is_some_and(|expected| {
            !tunnels
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, expected))
        }) {
            return;
        }
        if let Some(child) = tunnels.remove(id) {
            kill_tunnel(&child);
        }
    }
}

#[cfg(not(target_os = "android"))]
fn stop_all(state: &Connections) {
    state.exiting.store(true, Ordering::SeqCst);
    if let Ok(mut tunnels) = state.tunnels.lock() {
        for (_, child) in tunnels.drain() {
            kill_tunnel(&child);
        }
    }
}
#[cfg(not(target_os = "android"))]
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
#[cfg(not(target_os = "android"))]
fn register_tunnel(
    state: &Connections,
    id: &str,
    mut command: Command,
    port: u16,
) -> Result<Arc<Mutex<Tunnel>>, String> {
    let mut tunnels = state.tunnels.lock().map_err(|_| "SSH 状态不可用")?;
    if state.exiting.load(Ordering::SeqCst) {
        return Err("客户端正在退出".into());
    }
    if tunnels.contains_key(id) {
        return Err("该连接已有转发任务，请等待或关闭旧窗口".into());
    }
    let child = Arc::new(Mutex::new(Tunnel {
        generation: NEXT_GENERATION.fetch_add(1, Ordering::SeqCst),
        process: command.spawn().map_err(|_| "无法启动系统 OpenSSH")?,
        command,
        stopped: false,
        activated: false,
        exhausted: false,
        callback: None,
    }));
    tunnels.insert(id.to_owned(), child.clone());
    drop(tunnels);
    let monitored = child.clone();
    std::thread::spawn(move || recovery::supervise(monitored, port));
    Ok(child)
}
#[cfg(not(target_os = "android"))]
fn ssh_ready(child: Arc<Mutex<Tunnel>>, port: u16) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if child.lock().map(|t| t.stopped).unwrap_or(true) {
            return Err("连接已关闭".into());
        }
        if child
            .lock()
            .map_err(|_| "SSH 状态不可用")?
            .process
            .try_wait()
            .map_err(|_| "无法检查 SSH 状态")?
            .is_some()
        {
            return Err("SSH 已退出：请在终端验证密钥、Host 别名、known_hosts 和转发权限".into());
        }
        if let Ok(mut stream) =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(150))
        {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
            let _ = stream.write_all(
                format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            );
            let mut response = [0_u8; 12];
            if stream.read_exact(&mut response).is_ok() && transport_response(&response) {
                // Reachability only: 401 is expected before login, never reported as authenticated.
                let mut tunnel = child.lock().map_err(|_| "SSH 状态不可用")?;
                if tunnel.stopped {
                    return Err("连接已关闭".into());
                }
                tunnel.activated = true;
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("SSH 转发未就绪：远端 DSH 端口没有响应；请检查端口或重试".into())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenNodeResult {
    generation: u64,
    endpoint_index: usize,
}

#[cfg(not(target_os = "android"))]
struct OpeningNode {
    app: tauri::AppHandle,
    id: String,
}
#[cfg(not(target_os = "android"))]
impl Drop for OpeningNode {
    fn drop(&mut self) {
        if let Ok(mut ids) = self.app.state::<Connections>().opening_nodes.lock() {
            ids.remove(&self.id);
        }
    }
}
#[tauri::command]
async fn open_node(
    window: WebviewWindow,
    app: tauri::AppHandle,
    record: Connection,
    token: Option<String>,
    request_id: String,
) -> Result<OpenNodeResult, String> {
    require_shell(&window)?;
    connection_id(&request_id)?;
    #[cfg(target_os = "android")]
    {
        return open_connection(window, app, record, token)
            .await
            .map(|generation| OpenNodeResult {
                generation,
                endpoint_index: 0,
            });
    }
    #[cfg(not(target_os = "android"))]
    {
        let entries = endpoints::validate_entries(&record)?;
        // Validate before any transport side effects; never send the token during discovery.
        login_url(
            tauri::Url::parse("http://127.0.0.1/").unwrap(),
            token.as_deref(),
        )?;
        {
            let state = app.state::<Connections>();
            let mut ids = state.opening_nodes.lock().map_err(|_| "节点状态不可用")?;
            if !ids.insert(record.id.clone()) {
                return Err("节点正在连接，请等待".into());
            }
        }
        let _opening = OpeningNode {
            app: app.clone(),
            id: record.id.clone(),
        };
        if app
            .get_webview_window(&format!("dsh-{}", record.id))
            .is_some()
        {
            let index = app
                .state::<Connections>()
                .selected_entries
                .lock()
                .map_err(|_| "入口状态不可用")?
                .get(&record.id)
                .copied()
                .ok_or("请关闭旧连接窗口后重试")?;
            let entry = entries.get(index).ok_or("请保存修改并关闭旧窗口后重试")?;
            let generation = open_connection(
                window,
                app.clone(),
                entry.connection(&record.id, &record.name),
                token,
            )
            .await?;
            return Ok(OpenNodeResult {
                generation,
                endpoint_index: index,
            });
        }
        let (index, generation) = endpoints::try_in_order(entries.len(), |index| {
            let entry = entries[index].clone();
            let window = window.clone();
            let app = app.clone();
            let token = token.clone();
            let id = record.id.clone();
            let name = record.name.clone();
            let request_id = request_id.clone();
            async move {
                if app.state::<Connections>().exiting.load(Ordering::SeqCst) {
                    return Err("客户端正在退出".into());
                }
                if let Some(shell) = app.get_webview_window("main") {
                    let _ = shell.emit(
                        "node-attempt",
                        serde_json::json!({"id":id,"requestId":request_id,"index":index}),
                    );
                }
                let probe = entry.clone();
                tauri::async_runtime::spawn_blocking(move || probe.probe())
                    .await
                    .map_err(|_| "连接探测未完成")??;
                if app.state::<Connections>().exiting.load(Ordering::SeqCst) {
                    return Err("客户端正在退出".into());
                }
                open_connection(window, app, entry.connection(&id, &name), token).await
            }
        })
        .await?;
        app.state::<Connections>()
            .selected_entries
            .lock()
            .map_err(|_| "入口状态不可用")?
            .insert(record.id, index);
        Ok(OpenNodeResult {
            generation,
            endpoint_index: index,
        })
    }
}

#[tauri::command]
async fn open_connection(
    window: WebviewWindow,
    app: tauri::AppHandle,
    record: Connection,
    token: Option<String>,
) -> Result<u64, String> {
    require_shell(&window)?;
    connection_id(&record.id)?;
    if record.name.trim().is_empty() || record.name.len() > 160 {
        return Err("连接名称无效".into());
    }
    #[cfg(target_os = "android")]
    {
        return android::open(&app, record, token).await.map(|_| 0);
    }
    #[cfg(not(target_os = "android"))]
    {
        let label = format!("dsh-{}", record.id);
        let existing = app.get_webview_window(&label);
        let needs_retry = record.mode == "ssh-forward" && {
            let state = app.state::<Connections>();
            let tunnels = state.tunnels.lock().map_err(|_| "SSH 状态不可用")?;
            tunnels
                .get(&record.id)
                .map(|t| t.lock().map(|t| t.exhausted || t.stopped).unwrap_or(true))
                .unwrap_or(true)
        };
        if needs_retry {
            stop_tunnel(&app, &record.id);
        }
        if let Some(existing) = existing.as_ref().filter(|_| !needs_retry) {
            if record.mode == "direct" || token.as_ref().is_some_and(|t| !t.is_empty()) {
                let state = app.state::<Connections>();
                let base = state
                    .urls
                    .lock()
                    .map_err(|_| "连接状态不可用")?
                    .get(&record.id)
                    .cloned()
                    .ok_or("请关闭连接窗口后重新打开")?;
                existing
                    .navigate(login_url(base, token.as_deref())?)
                    .map_err(|_| "重新认证失败")?;
            }
            existing.set_focus().map_err(|_| "无法聚焦连接窗口")?;
            return Ok(tunnel_generation(&app, &record.id));
        }
        let base = match record.mode.as_str() {
            "direct" => direct_url(record.url.as_deref().ok_or("缺少 DSH 地址")?)?,
            "ssh-forward" => {
                #[cfg(mobile)]
                {
                    return Err("Android 暂不支持 SSH 转发；请使用已有 HTTPS 入口".into());
                }
                #[cfg(desktop)]
                {
                    let port = local_port(&record.id);
                    let reservation = TcpListener::bind(("127.0.0.1", port))
                        .map_err(|_| "本地转发端口被占用；请关闭旧连接或新建连接记录")?;
                    let config = match record.ssh_config.as_deref() {
                        None | Some("default") => None,
                        Some("none") => Some(std::path::PathBuf::from(if cfg!(windows) {
                            "NUL"
                        } else {
                            "/dev/null"
                        })),
                        Some("fleet") => Some(
                            app.path()
                                .home_dir()
                                .map_err(|_| "无法定位 SSH 配置")?
                                .join(".ssh/dsh-fleet-config"),
                        ),
                        _ => return Err("SSH 配置选项无效".into()),
                    };
                    let args = ssh_args(
                        record.ssh_profile.as_deref().ok_or("缺少 SSH Host")?,
                        record.ssh_user.as_deref().filter(|u| !u.is_empty()),
                        record.remote_port,
                        port,
                        config.as_ref().and_then(|p| p.to_str()),
                    )?;
                    // Validate transient login before starting anything.
                    let base = tauri::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
                    login_url(base.clone(), token.as_deref())?;
                    drop(reservation);
                    let mut command = Command::new("ssh");
                    command
                        .args(args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    let child =
                        register_tunnel(&app.state::<Connections>(), &record.id, command, port)?;
                    let result =
                        tauri::async_runtime::spawn_blocking(move || ssh_ready(child, port)).await;
                    match result {
                        Ok(Ok(())) => (),
                        _ => {
                            stop_tunnel(&app, &record.id);
                            return Err(
                                "SSH 转发未就绪；请检查 Host、密钥、转发权限和远端端口".into()
                            );
                        }
                    }
                    base
                }
            }
            _ => return Err("连接模式无效".into()),
        };
        let target = login_url(base.clone(), token.as_deref())?;
        if let Some(existing) = existing {
            install_recovery_events(&app, &record.id, &base);
            existing
                .navigate(target)
                .map_err(|_| "无法重新加载连接窗口")?;
            existing.set_focus().map_err(|_| "无法聚焦连接窗口")?;
            return Ok(tunnel_generation(&app, &record.id));
        }
        let nav_base = base.clone();
        let nav_app = app.clone();
        let popup_base = base.clone();
        let popup_app = app.clone();
        let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(target))
            .title(format!("DSH · {}", record.name))
            .inner_size(1440.0, 920.0)
            .disable_drag_drop_handler()
            .on_page_load(|view, payload| {
                if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                    && payload.url().query_pairs().any(|(key, _)| key == "token")
                {
                    let _ = view
                        .eval("history.replaceState(null, '', location.pathname + location.hash)");
                }
            })
            .on_navigation(move |url| {
                if same_origin(&nav_base, url) {
                    true
                } else {
                    open_external(&nav_app, url);
                    false
                }
            })
            .on_new_window(move |url, _| {
                if same_origin(&popup_base, &url) {
                    // Keep same-origin actions in the owned view, never create an unguarded WebView.
                    if let Some(view) = popup_app.get_webview_window(&label) {
                        let _ = view.navigate(url);
                    }
                } else {
                    open_external(&popup_app, &url);
                }
                tauri::webview::NewWindowResponse::Deny
            });
        #[cfg(target_os = "macos")]
        {
            builder = builder.data_store_identifier(partition_id(&record.id));
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            builder = builder.data_directory(
                app.path()
                    .app_data_dir()
                    .map_err(|_| "无法定位分区目录")?
                    .join("partitions")
                    .join(&record.id),
            );
        }
        let view = match builder.build() {
            Ok(view) => view,
            Err(_) => {
                stop_tunnel(&app, &record.id);
                return Err("无法创建 DSH 窗口".into());
            }
        };
        app.state::<Connections>()
            .urls
            .lock()
            .map_err(|_| "连接状态不可用")?
            .insert(record.id.clone(), base.clone());
        let slot = Arc::new(Mutex::new(None));
        app.state::<Connections>()
            .window_tunnels
            .lock()
            .map_err(|_| "窗口状态不可用")?
            .insert(record.id.clone(), slot.clone());
        install_recovery_events(&app, &record.id, &base);
        let generation = tunnel_generation(&app, &record.id);
        let close_app = app.clone();
        let close_id = record.id;
        view.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let expected = slot.lock().ok().and_then(|slot| slot.as_ref().and_then(Weak::upgrade));
                if let Some(expected) = expected {
                    stop_tunnel_state(&close_app.state::<Connections>(), &close_id, Some(&expected));
                }
                if let Some(shell) = close_app.get_webview_window("main") {
                    let _ = shell.emit("connection-status", serde_json::json!({"id": close_id, "state": "disconnected", "attempt": 0, "generation": generation}));
                }
            }
        });
        Ok(generation)
    }
}
#[cfg(not(target_os = "android"))]
fn install_recovery_events(app: &tauri::AppHandle, id: &str, base: &tauri::Url) {
    let state = app.state::<Connections>();
    let Ok(tunnels) = state.tunnels.lock() else {
        return;
    };
    let Some(child) = tunnels.get(id) else { return };
    let expected = Arc::downgrade(child);
    if let Ok(slots) = state.window_tunnels.lock() {
        if let Some(slot) = slots.get(id) {
            if let Ok(mut bound) = slot.lock() {
                *bound = Some(expected.clone());
            }
        }
    }
    let Some(view) = app.get_webview_window(&format!("dsh-{id}")) else {
        return;
    };
    let app = app.clone();
    let id = id.to_owned();
    let base = base.clone();
    if let Ok(mut tunnel) = child.lock() {
        let generation = tunnel.generation;
        tunnel.callback = Some(Arc::new(move |status, attempt| {
            let Some(expected) = expected.upgrade() else {
                return;
            };
            let active = app
                .state::<Connections>()
                .tunnels
                .lock()
                .map(|tasks| {
                    tasks
                        .get(&id)
                        .is_some_and(|current| Arc::ptr_eq(current, &expected))
                })
                .unwrap_or(false);
            if !active {
                return;
            }
            if let Some(shell) = app.get_webview_window("main") {
                let _ = shell.emit("connection-status", serde_json::json!({"id": id, "state": status, "attempt": attempt, "generation": generation}));
            }
            if status == "ready" {
                // Capture the exact old window handle: never look up a replacement window by label.
                let _ = view.navigate(base.clone());
            }
        }));
    }
}
#[cfg(not(target_os = "android"))]
fn tunnel_generation(app: &tauri::AppHandle, id: &str) -> u64 {
    app.state::<Connections>()
        .tunnels
        .lock()
        .ok()
        .and_then(|tasks| {
            tasks
                .get(id)
                .and_then(|t| t.lock().ok().map(|t| t.generation))
        })
        .unwrap_or(0)
}
#[cfg(not(target_os = "android"))]
fn open_external(app: &tauri::AppHandle, url: &tauri::Url) {
    if ["https", "http"].contains(&url.scheme())
        && url.username().is_empty()
        && url.password().is_none()
        && !url.query_pairs().any(|(k, _)| {
            ["token", "password", "secret", "authorization"].contains(&k.to_lowercase().as_str())
        })
    {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
    }
}
#[tauri::command]
async fn close_connection(
    window: WebviewWindow,
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    require_shell(&window)?;
    connection_id(&id)?;
    #[cfg(target_os = "android")]
    {
        return android::call(&app, "close", serde_json::json!({ "id": id })).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        if let Some(view) = app.get_webview_window(&format!("dsh-{id}")) {
            view.close().map_err(|_| "无法关闭旧连接窗口")?;
        }
        stop_tunnel(&app, &id);
        app.state::<Connections>()
            .urls
            .lock()
            .map_err(|_| "连接状态不可用")?
            .remove(&id);
        Ok(())
    }
}
#[tauri::command]
async fn delete_connection(
    window: WebviewWindow,
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    require_shell(&window)?;
    connection_id(&id)?;
    #[cfg(target_os = "android")]
    {
        return android::call(&app, "delete", serde_json::json!({ "id": id })).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let label = format!("dsh-{id}");
        let view = if let Some(view) = app.get_webview_window(&label) {
            view
        } else {
            let mut builder = WebviewWindowBuilder::new(
                &app,
                &label,
                WebviewUrl::External("about:blank".parse().unwrap()),
            )
            .visible(false);
            #[cfg(target_os = "macos")]
            {
                builder = builder.data_store_identifier(partition_id(&id));
            }
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                builder = builder.data_directory(
                    app.path()
                        .app_data_dir()
                        .map_err(|_| "无法定位分区")?
                        .join("partitions")
                        .join(&id),
                );
            }
            builder.build().map_err(|_| "无法打开待清理分区")?
        };
        let cleared = view
            .clear_all_browsing_data()
            .map_err(|_| "无法清理连接分区");
        let closed = view.close().map_err(|_| "无法关闭连接窗口");
        stop_tunnel(&app, &id);
        cleared?;
        closed?;
        app.state::<Connections>()
            .urls
            .lock()
            .map_err(|_| "连接状态不可用")?
            .remove(&id);
        Ok(())
    }
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder.plugin(android::init());
    #[cfg(not(target_os = "android"))]
    let builder = builder.manage(Connections::default());
    builder
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            open_node,
            open_connection,
            delete_connection,
            close_connection
        ])
        .build(tauri::generate_context!())
        .expect("无法启动 DSH Client")
        .run(|_app, _event| {
            #[cfg(not(target_os = "android"))]
            if matches!(_event, tauri::RunEvent::Exit) {
                stop_all(&_app.state::<Connections>());
            }
        });
}

#[cfg(all(test, unix))]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn stale_window_cleanup_cannot_kill_a_replacement_forward() {
        let state = Connections::default();
        let mut first = Command::new("sleep");
        first.arg("30");
        let old = register_tunnel(&state, "same-id", first, 0).unwrap();
        stop_tunnel_state(&state, "same-id", None);
        let mut second = Command::new("sleep");
        second.arg("30");
        let replacement = register_tunnel(&state, "same-id", second, 0).unwrap();
        stop_tunnel_state(&state, "same-id", Some(&old));
        let alive = replacement
            .lock()
            .unwrap()
            .process
            .try_wait()
            .unwrap()
            .is_none();
        stop_all(&state);
        assert!(alive, "old window teardown must not kill replacement task");
    }
    #[test]
    fn lost_forward_recovers_in_place_without_changing_its_origin() {
        struct Cleanup(Connections);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                stop_all(&self.0);
            }
        }
        let cleanup = Cleanup(Connections::default());
        let reservation = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let command = fixture_command(port);
        let child = register_tunnel(&cleanup.0, "test-recovery", command, port).unwrap();
        ssh_ready(child.clone(), port).unwrap();
        let first_pid = {
            let mut tunnel = child.lock().unwrap();
            let pid = tunnel.process.id();
            tunnel.process.kill().unwrap();
            pid
        };
        let until = Instant::now() + Duration::from_secs(6);
        let mut recovered = false;
        while Instant::now() < until {
            if child.lock().unwrap().process.id() != first_pid
                && ssh_ready(child.clone(), port).is_ok()
            {
                recovered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            recovered,
            "a dropped forward must be recreated on the same origin"
        );
        stop_all(&cleanup.0);
        assert!(child.lock().unwrap().process.try_wait().unwrap().is_some());
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
    #[test]
    #[ignore = "Subprocess HTTP fixture invoked by lifecycle tests only"]
    fn http_fixture_child() {
        let port: u16 = std::env::var("DSH_CLIENT_HTTP_FIXTURE_PORT")
            .expect("fixture port required")
            .parse()
            .unwrap();
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        for incoming in listener.incoming() {
            let mut stream = incoming.unwrap();
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK");
        }
    }
    fn fixture_command(port: u16) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "lifecycle_tests::http_fixture_child",
            ])
            .env("DSH_CLIENT_HTTP_FIXTURE_PORT", port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    struct Fixture(Connections);
    impl Drop for Fixture {
        fn drop(&mut self) {
            stop_all(&self.0);
        }
    }
    fn forward_fixture() -> (Fixture, Arc<Mutex<Tunnel>>, u16) {
        let fixture = Fixture(Connections::default());
        let reserved = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let command = fixture_command(port);
        let child = register_tunnel(&fixture.0, "test", command, port).unwrap();
        ssh_ready(child.clone(), port).unwrap();
        (fixture, child, port)
    }
    fn recovery_events(child: &Arc<Mutex<Tunnel>>) -> std::sync::mpsc::Receiver<(String, u8)> {
        let (send, receive) = std::sync::mpsc::channel();
        child.lock().unwrap().callback = Some(Arc::new(move |status, attempt| {
            let _ = send.send((status.to_owned(), attempt));
        }));
        receive
    }
    #[test]
    fn failed_recovery_has_a_finite_budget_and_leaves_no_child() {
        let (_fixture, child, _) = forward_fixture();
        let events = recovery_events(&child);
        {
            let mut tunnel = child.lock().unwrap();
            tunnel.command = Command::new("false");
            tunnel.process.kill().unwrap();
        }
        let mut observed = vec![];
        loop {
            let event = events.recv_timeout(Duration::from_secs(6)).unwrap();
            observed.push(event.clone());
            if event.0 == "failed" {
                break;
            }
        }
        assert_eq!(
            observed,
            [
                ("reconnecting".into(), 1),
                ("reconnecting".into(), 2),
                ("reconnecting".into(), 3),
                ("failed".into(), 3)
            ]
        );
        let last_pid = child.lock().unwrap().process.id();
        std::thread::sleep(Duration::from_millis(200));
        let mut tunnel = child.lock().unwrap();
        assert!(tunnel.exhausted);
        assert_eq!(tunnel.process.id(), last_pid);
        assert!(tunnel.process.try_wait().unwrap().is_some());
    }
    #[test]
    fn closing_during_backoff_cancels_all_future_spawns() {
        let (fixture, child, port) = forward_fixture();
        let events = recovery_events(&child);
        let first_pid = {
            let mut tunnel = child.lock().unwrap();
            let pid = tunnel.process.id();
            tunnel.process.kill().unwrap();
            pid
        };
        assert_eq!(
            events.recv_timeout(Duration::from_secs(6)).unwrap(),
            ("reconnecting".into(), 1)
        );
        stop_all(&fixture.0);
        std::thread::sleep(Duration::from_millis(250));
        let mut tunnel = child.lock().unwrap();
        assert!(tunnel.stopped);
        assert_eq!(tunnel.process.id(), first_pid);
        assert!(tunnel.process.try_wait().unwrap().is_some());
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
    #[test]
    fn recovery_refuses_a_port_taken_by_another_service() {
        let (_fixture, child, port) = forward_fixture();
        let events = recovery_events(&child);
        let (reservation, first_pid) = {
            let mut tunnel = child.lock().unwrap();
            let pid = tunnel.process.id();
            tunnel.process.kill().unwrap();
            tunnel.process.wait().unwrap();
            (TcpListener::bind(("127.0.0.1", port)).unwrap(), pid)
        };
        loop {
            let event = events.recv_timeout(Duration::from_secs(6)).unwrap();
            assert_ne!(event.0, "ready");
            if event.0 == "failed" {
                break;
            }
        }
        assert_eq!(child.lock().unwrap().process.id(), first_pid);
        assert_eq!(reservation.local_addr().unwrap().port(), port);
    }
    #[test]
    #[ignore = "Opt-in test against the user-authorized .13 SSH route"]
    fn live_air_forward_ready_and_cleanup() {
        assert_eq!(std::env::var("DSH_CLIENT_LIVE_SSH").as_deref(), Ok("1"));
        let state = Connections::default();
        let reservation = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = reservation.local_addr().unwrap().port();
        let args = ssh_args(
            "10.250.0.13",
            Some("zcj123v"),
            3080,
            port,
            Some("/dev/null"),
        )
        .unwrap();
        drop(reservation);
        let mut command = Command::new("ssh");
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let pending = register_tunnel(&state, "live-air", command, port).unwrap();
        let ready = ssh_ready(pending.clone(), port);
        assert!(ready.is_ok(), "authorized initial forward failed");
        let events = recovery_events(&pending);
        let first_pid = {
            let mut tunnel = pending.lock().unwrap();
            let pid = tunnel.process.id();
            tunnel.process.kill().unwrap();
            pid
        };
        loop {
            let event = events.recv_timeout(Duration::from_secs(25)).unwrap();
            assert_ne!(event.0, "failed", "authorized SSH route did not recover");
            if event.0 == "ready" {
                break;
            }
        }
        assert_ne!(pending.lock().unwrap().process.id(), first_pid);
        assert!(ssh_ready(pending.clone(), port).is_ok());
        stop_all(&state);
        assert!(
            ready.is_ok(),
            "authorized SSH route did not reach remote HTTP"
        );
        assert!(
            pending
                .lock()
                .unwrap()
                .process
                .try_wait()
                .unwrap()
                .is_some()
        );
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
    #[test]
    fn exiting_kills_a_tunnel_even_while_readiness_holds_an_arc() {
        let state = Connections::default();
        let mut command = Command::new("sleep");
        command.arg("30");
        let pending = register_tunnel(&state, "test", command, 0).unwrap();
        stop_all(&state);
        assert!(
            pending
                .lock()
                .unwrap()
                .process
                .try_wait()
                .unwrap()
                .is_some()
        );
        assert!(state.tunnels.lock().unwrap().is_empty());
        assert!(register_tunnel(&state, "late", Command::new("sleep"), 0).is_err());
    }
}
