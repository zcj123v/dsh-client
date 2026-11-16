#[cfg(target_os = "android")]
mod android;
mod policy;
use policy::*;
use serde::Deserialize;
#[cfg(not(target_os = "android"))]
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[cfg(not(target_os = "android"))]
use tauri::Manager;
use tauri::WebviewWindow;
#[cfg(not(target_os = "android"))]
use tauri::{WebviewUrl, WebviewWindowBuilder};
#[cfg(not(target_os = "android"))]
use tauri_plugin_opener::OpenerExt;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    id: String,
    name: String,
    mode: String,
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
struct Tunnel(Child);
#[cfg(not(target_os = "android"))]
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(not(target_os = "android"))]
#[derive(Default)]
struct Connections {
    tunnels: Mutex<HashMap<String, Arc<Mutex<Tunnel>>>>,
    exiting: AtomicBool,
    urls: Mutex<HashMap<String, tauri::Url>>,
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
        let _ = tunnel.0.kill();
        let _ = tunnel.0.wait();
    }
}
#[cfg(not(target_os = "android"))]
fn stop_tunnel(app: &tauri::AppHandle, id: &str) {
    let state = app.state::<Connections>();
    if let Ok(mut tunnels) = state.tunnels.lock() {
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
fn register_tunnel(
    state: &Connections,
    id: &str,
    mut command: Command,
) -> Result<Arc<Mutex<Tunnel>>, String> {
    let mut tunnels = state.tunnels.lock().map_err(|_| "SSH 状态不可用")?;
    if state.exiting.load(Ordering::SeqCst) {
        return Err("客户端正在退出".into());
    }
    let child = Arc::new(Mutex::new(Tunnel(
        command.spawn().map_err(|_| "无法启动系统 OpenSSH")?,
    )));
    tunnels.insert(id.to_owned(), child.clone());
    Ok(child)
}
#[cfg(not(target_os = "android"))]
fn ssh_ready(child: Arc<Mutex<Tunnel>>, port: u16) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(12) {
        if child
            .lock()
            .map_err(|_| "SSH 状态不可用")?
            .0
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
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("SSH 转发未就绪：远端 DSH 端口没有响应；请检查端口或重试".into())
}

#[tauri::command]
async fn open_connection(
    window: WebviewWindow,
    app: tauri::AppHandle,
    record: Connection,
    token: Option<String>,
) -> Result<(), String> {
    require_shell(&window)?;
    connection_id(&record.id)?;
    if record.name.trim().is_empty() || record.name.len() > 160 {
        return Err("连接名称无效".into());
    }
    #[cfg(target_os = "android")]
    {
        return android::open(&app, record, token).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let label = format!("dsh-{}", record.id);
        if let Some(existing) = app.get_webview_window(&label) {
            if token.as_ref().is_some_and(|t| !t.is_empty()) {
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
            return Ok(());
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
                    let child = register_tunnel(&app.state::<Connections>(), &record.id, command)?;
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
            .insert(record.id.clone(), base);
        let close_app = app.clone();
        let close_id = record.id;
        view.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                stop_tunnel(&close_app, &close_id);
            }
        });
        Ok(())
    }
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
        let pending = register_tunnel(&state, "live-air", command).unwrap();
        let ready = ssh_ready(pending.clone(), port);
        stop_all(&state);
        assert!(
            ready.is_ok(),
            "authorized SSH route did not reach remote HTTP"
        );
        assert!(pending.lock().unwrap().0.try_wait().unwrap().is_some());
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
    #[test]
    fn exiting_kills_a_tunnel_even_while_readiness_holds_an_arc() {
        let state = Connections::default();
        let mut command = Command::new("sleep");
        command.arg("30");
        let pending = register_tunnel(&state, "test", command).unwrap();
        stop_all(&state);
        assert!(pending.lock().unwrap().0.try_wait().unwrap().is_some());
        assert!(state.tunnels.lock().unwrap().is_empty());
        assert!(register_tunnel(&state, "late", Command::new("sleep")).is_err());
    }
}
