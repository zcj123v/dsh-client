use super::*;
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub id: String,
    pub name: String,
    pub state: String,
    pub generation: u64,
}
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TabSnapshot {
    pub tabs: Vec<TabInfo>,
    pub active_id: Option<String>,
}
pub struct TabSession {
    pub id: String,
    pub name: String,
    pub generation: AtomicU64,
    pub cancelled: AtomicBool,
    state: Mutex<String>,
}
#[derive(Default)]
pub struct TabRegistry {
    sessions: Vec<Arc<TabSession>>,
    active: Option<String>,
}
impl TabRegistry {
    pub fn begin(&mut self, id: &str, name: &str, generation: u64) -> Arc<TabSession> {
        if let Some(session) = self.get(id) {
            if session.state.lock().is_ok_and(|state| *state == "failed") {
                session.generation.store(generation, Ordering::SeqCst);
                *session.state.lock().unwrap() = "opening".into();
                self.active = Some(id.into());
            }
            return session;
        }
        let session = Arc::new(TabSession {
            id: id.into(),
            name: name.into(),
            generation: AtomicU64::new(generation),
            cancelled: AtomicBool::new(false),
            state: Mutex::new("opening".into()),
        });
        self.sessions.push(session.clone());
        self.active = Some(id.into());
        session
    }
    pub fn get(&self, id: &str) -> Option<Arc<TabSession>> {
        self.sessions.iter().find(|s| s.id == id).cloned()
    }
    pub fn activate(&mut self, id: Option<&str>) -> Result<(), String> {
        if let Some(id) = id {
            if self.get(id).is_none() {
                return Err("标签已关闭".into());
            }
        }
        self.active = id.map(str::to_owned);
        Ok(())
    }
    pub fn close(&mut self, id: &str) -> Option<Arc<TabSession>> {
        let index = self.sessions.iter().position(|s| s.id == id)?;
        let removed = self.sessions.remove(index);
        removed.cancelled.store(true, Ordering::SeqCst);
        if self.active.as_deref() == Some(id) {
            self.active = self
                .sessions
                .get(index.saturating_sub(1))
                .or(self.sessions.first())
                .map(|s| s.id.clone());
        }
        Some(removed)
    }
    pub fn finish(&mut self, session: &Arc<TabSession>, generation: u64, success: bool) -> bool {
        if session.cancelled.load(Ordering::SeqCst)
            || session.generation.load(Ordering::SeqCst) != generation
            || !self
                .get(&session.id)
                .is_some_and(|s| Arc::ptr_eq(&s, session))
        {
            return false;
        }
        *session.state.lock().unwrap() = if success { "ready" } else { "failed" }.into();
        if !success && self.active.as_deref() == Some(&session.id) {
            self.active = None;
        }
        true
    }
    pub fn mark_failed(&mut self, session: &Arc<TabSession>, generation: u64) -> bool {
        if session.cancelled.load(Ordering::SeqCst)
            || session.generation.load(Ordering::SeqCst) != generation
            || !self
                .get(&session.id)
                .is_some_and(|current| Arc::ptr_eq(&current, session))
        {
            return false;
        }
        *session.state.lock().unwrap() = "failed".into();
        if self.active.as_deref() == Some(&session.id) {
            self.active = None;
        }
        true
    }
    pub fn snapshot(&self) -> TabSnapshot {
        TabSnapshot {
            tabs: self
                .sessions
                .iter()
                .map(|s| TabInfo {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    state: s.state.lock().unwrap().clone(),
                    generation: s.generation.load(Ordering::SeqCst),
                })
                .collect(),
            active_id: self.active.clone(),
        }
    }
}
pub fn content_layout(w: f64, h: f64) -> (f64, f64, f64, f64) {
    (0., 56., w.max(0.), (h - 56.).max(0.))
}
pub fn snapshot(app: &tauri::AppHandle) -> Result<TabSnapshot, String> {
    Ok(app
        .state::<Connections>()
        .tabs
        .lock()
        .map_err(|_| "标签状态不可用")?
        .snapshot())
}
pub fn publish(app: &tauri::AppHandle) {
    if let (Some(shell), Ok(snapshot)) = (app.get_webview("main"), snapshot(app)) {
        let _ = shell.emit("tabs-changed", snapshot);
    }
}
pub fn layout(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window("main") {
        if let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
            let size = size.to_logical::<f64>(scale);
            let (x, y, w, h) = content_layout(size.width, size.height);
            for view in window.webviews().iter().filter(|v| v.label() != "main") {
                let _ = view.set_bounds(tauri::Rect {
                    position: tauri::LogicalPosition::new(x, y).into(),
                    size: tauri::LogicalSize::new(w, h).into(),
                });
            }
        }
    }
}
pub fn apply_visibility(app: &tauri::AppHandle) -> Result<(), String> {
    let hidden = app
        .state::<Connections>()
        .window_hidden
        .load(Ordering::SeqCst);
    let active = snapshot(app)?.active_id;
    if let Some(window) = app.get_window("main") {
        for view in window.webviews().iter().filter(|v| v.label() != "main") {
            if active
                .as_ref()
                .is_some_and(|id| view.label() == format!("dsh-{id}"))
            {
                view.show().map_err(|_| "无法显示标签")?;
                if !hidden {
                    let _ = view.set_focus();
                }
            } else {
                view.hide().map_err(|_| "无法隐藏标签")?;
            }
        }
        if active.is_none() && !hidden {
            if let Some(shell) = app.get_webview("main") {
                let _ = shell.set_focus();
            }
        }
        if !hidden {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    publish(app);
    Ok(())
}
pub fn close(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let generation = super::tunnel_generation(app, id);
    app.state::<Connections>()
        .tabs
        .lock()
        .map_err(|_| "标签状态不可用")?
        .close(id);
    // Mark ownership cancelled before closing any native view or pending process.
    if let Some(slot) = app
        .state::<Connections>()
        .window_tunnels
        .lock()
        .map_err(|_| "标签状态不可用")?
        .get(id)
    {
        let _ = close_binding(slot);
    }
    let close_result = if let Some(view) = app.get_webview(&format!("dsh-{id}")) {
        view.close().map_err(|_| "无法关闭标签")
    } else {
        Ok(())
    };
    stop_tunnel(app, id);
    app.state::<Connections>()
        .window_tunnels
        .lock()
        .map_err(|_| "标签状态不可用")?
        .remove(id);
    app.state::<Connections>()
        .urls
        .lock()
        .map_err(|_| "标签状态不可用")?
        .remove(id);
    super::emit_shell_event(
        app,
        "connection-status",
        serde_json::json!({"id":id,"state":"disconnected","attempt":0,"generation":generation}),
    );
    let visibility = apply_visibility(app);
    close_result?;
    visibility
}

pub fn begin(app: &tauri::AppHandle, id: &str, name: &str) -> Result<Arc<TabSession>, String> {
    let state = app.state::<Connections>();
    let session = state.tabs.lock().map_err(|_| "标签状态不可用")?.begin(
        id,
        name,
        NEXT_GENERATION.fetch_add(1, Ordering::SeqCst),
    );
    if !state
        .window_tunnels
        .lock()
        .map_err(|_| "标签状态不可用")?
        .contains_key(id)
    {
        state
            .window_tunnels
            .lock()
            .map_err(|_| "标签状态不可用")?
            .insert(
                id.into(),
                Arc::new(Mutex::new(WindowBinding {
                    closed: false,
                    child: None,
                    generation: session.generation.load(Ordering::SeqCst),
                })),
            );
    }
    let _ = apply_visibility(app);
    publish(app);
    Ok(session)
}
pub struct OpenAttempt {
    app: tauri::AppHandle,
    session: Arc<TabSession>,
    generation: u64,
    completed: bool,
}
impl OpenAttempt {
    pub fn new(app: &tauri::AppHandle, session: Arc<TabSession>) -> Self {
        let generation = session.generation.load(Ordering::SeqCst);
        Self {
            app: app.clone(),
            session,
            generation,
            completed: false,
        }
    }
    pub fn success(&mut self) -> Result<(), String> {
        if !self
            .app
            .state::<Connections>()
            .tabs
            .lock()
            .map_err(|_| "标签状态不可用")?
            .finish(&self.session, self.generation, true)
        {
            return Err("标签已关闭".into());
        }
        self.completed = true;
        apply_visibility(&self.app)
    }
}
impl Drop for OpenAttempt {
    fn drop(&mut self) {
        if !self.completed {
            if let Ok(mut registry) = self.app.state::<Connections>().tabs.lock() {
                registry.finish(&self.session, self.generation, false);
            }
            let _ = apply_visibility(&self.app);
            publish(&self.app);
        }
    }
}
fn ensure_active(session: &Arc<TabSession>) -> Result<(), String> {
    if session.cancelled.load(Ordering::SeqCst) {
        Err("标签已关闭".into())
    } else {
        Ok(())
    }
}

pub async fn open_connection(
    app: &tauri::AppHandle,
    record: Connection,
    token: Option<String>,
    session: Arc<TabSession>,
) -> Result<u64, String> {
    ensure_active(&session)?;
    let label = format!("dsh-{}", record.id);
    let existing = app.get_webview(&label);
    let slot = app
        .state::<Connections>()
        .window_tunnels
        .lock()
        .map_err(|_| "标签状态不可用")?
        .get(&record.id)
        .cloned()
        .ok_or("标签已关闭")?;
    let needs_retry = record.mode == "ssh-forward"
        && app
            .state::<Connections>()
            .tunnels
            .lock()
            .map_err(|_| "SSH 状态不可用")?
            .get(&record.id)
            .map(|t| {
                t.lock()
                    .map(|t| t.stopped || t.exhausted || !t.activated)
                    .unwrap_or(true)
            })
            .unwrap_or(true);
    if let Some(view) = existing.as_ref() {
        if !needs_retry {
            ensure_active(&session)?;
            if token.as_deref().is_some_and(|value| !value.is_empty()) {
                let base = app
                    .state::<Connections>()
                    .urls
                    .lock()
                    .map_err(|_| "连接状态不可用")?
                    .get(&record.id)
                    .cloned()
                    .ok_or("入口状态缺失")?;
                view.navigate(login_url(base, token.as_deref())?)
                    .map_err(|_| "无法重新认证")?;
            }
            app.state::<Connections>()
                .tabs
                .lock()
                .map_err(|_| "标签状态不可用")?
                .activate(Some(&record.id))?;
            apply_visibility(app)?;
            return Ok(tunnel_generation(app, &record.id));
        }
        stop_tunnel(app, &record.id);
    }
    let mut pending: Option<TunnelAttempt> = None;
    let base = match record.mode.as_str() {
        "direct" => direct_url(record.url.as_deref().ok_or("缺少 DSH 地址")?)?,
        "ssh-forward" => {
            let port = local_port(&record.id);
            let reservation =
                TcpListener::bind(("127.0.0.1", port)).map_err(|_| "本地转发端口被占用")?;
            let config = match record.ssh_config.as_deref() {
                None | Some("default") => None,
                Some("none") => Some(std::path::PathBuf::from("/dev/null")),
                Some("fleet") => Some(
                    app.path()
                        .home_dir()
                        .map_err(|_| "无法定位 SSH 配置")?
                        .join(".ssh/dsh-fleet-config"),
                ),
                _ => return Err("SSH 配置无效".into()),
            };
            let args = ssh_args(
                record.ssh_profile.as_deref().ok_or("缺少 SSH Host")?,
                record.ssh_user.as_deref().filter(|u| !u.is_empty()),
                record.remote_port,
                port,
                config.as_ref().and_then(|p| p.to_str()),
            )?;
            let base = tauri::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
            login_url(base.clone(), token.as_deref())?;
            ensure_active(&session)?;
            drop(reservation);
            let mut command = Command::new("ssh");
            command
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = register_tunnel(&app.state::<Connections>(), &record.id, command, port)?;
            let rollback_app = app.clone();
            let id = record.id.clone();
            let expected = child.clone();
            pending = Some(TunnelAttempt {
                rollback: Some(Box::new(move || {
                    stop_tunnel_state(&rollback_app.state::<Connections>(), &id, Some(&expected))
                })),
            });
            bind_pending_slot(&slot, &child)?;
            ensure_active(&session)?;
            tauri::async_runtime::spawn_blocking(move || {
                ssh_ready_with_timeout(child, port, Duration::from_secs(5))
            })
            .await
            .map_err(|_| "SSH 检查失败")??;
            ensure_active(&session)?;
            base
        }
        _ => return Err("连接模式无效".into()),
    };
    let target = login_url(base.clone(), token.as_deref())?;
    ensure_active(&session)?;
    let view = if let Some(view) = existing {
        view.navigate(target).map_err(|_| "无法重新加载标签")?;
        view
    } else {
        let nav_base = base.clone();
        let nav_app = app.clone();
        let popup_app = app.clone();
        let popup_base = base.clone();
        let popup_session = Arc::downgrade(&session);
        let popup_label = label.clone();
        let builder = tauri::webview::WebviewBuilder::new(&label, WebviewUrl::External(target))
            .data_store_identifier(partition_id(&record.id))
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
                    if popup_session
                        .upgrade()
                        .is_some_and(|s| !s.cancelled.load(Ordering::SeqCst))
                    {
                        if let Some(view) = popup_app.get_webview(&popup_label) {
                            let _ = view.navigate(url);
                        }
                    }
                } else {
                    open_external(&popup_app, &url);
                }
                tauri::webview::NewWindowResponse::Deny
            });
        let window = app.get_window("main").ok_or("主窗口不可用")?;
        let size = window
            .inner_size()
            .map_err(|_| "窗口尺寸不可用")?
            .to_logical::<f64>(window.scale_factor().map_err(|_| "窗口缩放不可用")?);
        let (_, _, w, h) = content_layout(size.width, size.height);
        let view = window
            .add_child(
                builder,
                tauri::LogicalPosition::new(0., 56.),
                tauri::LogicalSize::new(w, h),
            )
            .map_err(|_| "无法创建标签内容")?;
        if ensure_active(&session).is_err() {
            let _ = view.close();
            return Err("标签已关闭".into());
        }
        view
    };
    {
        let binding = slot.lock().map_err(|_| "标签状态不可用")?;
        if binding.closed || session.cancelled.load(Ordering::SeqCst) {
            let _ = view.close();
            return Err("标签已关闭".into());
        }
        app.state::<Connections>()
            .urls
            .lock()
            .map_err(|_| "连接状态不可用")?
            .insert(record.id.clone(), base.clone());
        if let Some(guard) = pending.as_mut() {
            guard.commit();
        }
    }
    install_recovery_events(app, &record.id, &base);
    app.state::<Connections>()
        .tabs
        .lock()
        .map_err(|_| "标签状态不可用")?
        .activate(Some(&record.id))?;
    apply_visibility(app)?;
    Ok(tunnel_generation(app, &record.id))
}
pub fn delete(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    if let Some(view) = app.get_webview(&format!("dsh-{id}")) {
        view.clear_all_browsing_data()
            .map_err(|_| "无法清理节点浏览数据")?;
        close(app, id)
    } else {
        // A temporary hidden child requests system clearing for the same per-node data store.
        let window = app.get_window("main").ok_or("主窗口不可用")?;
        let view = window
            .add_child(
                tauri::webview::WebviewBuilder::new(
                    format!("clear-{id}"),
                    WebviewUrl::External("about:blank".parse().unwrap()),
                )
                .data_store_identifier(partition_id(id)),
                tauri::LogicalPosition::new(0., 56.),
                tauri::LogicalSize::new(1., 1.),
            )
            .map_err(|_| "无法打开待清理分区")?;
        let _ = view.hide();
        let cleared = view
            .clear_all_browsing_data()
            .map_err(|_| "无法清理节点浏览数据");
        let _ = view.close();
        cleared?;
        close(app, id)
    }
}
pub fn mark_transport_failed(app: &tauri::AppHandle, session: &Arc<TabSession>, generation: u64) {
    let marked = app
        .state::<Connections>()
        .tabs
        .lock()
        .map(|mut tabs| tabs.mark_failed(session, generation))
        .unwrap_or(false);
    if marked {
        let _ = apply_visibility(app);
        publish(app);
    }
}
pub fn reveal(app: &tauri::AppHandle) {
    app.state::<Connections>()
        .window_hidden
        .store(false, Ordering::SeqCst);
    let _ = apply_visibility(app);
}
pub fn install(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let manager = MenuItem::with_id(app, "manager", "节点管理", true, Some("CmdOrCtrl+1"))?;
    let new = MenuItem::with_id(app, "new", "新建连接标签", true, Some("CmdOrCtrl+T"))?;
    let close = MenuItem::with_id(app, "close", "关闭当前标签", true, Some("CmdOrCtrl+W"))?;
    let file = Submenu::with_items(app, "文件", true, &[&manager, &new, &close])?;
    let about = PredefinedMenuItem::about(app, None, None)?;
    let quit = PredefinedMenuItem::quit(app, None)?;
    let application = Submenu::with_items(app, "DSH Client", true, &[&about, &quit])?;
    let edit = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&application, &file, &edit])?)?;
    app.on_menu_event(|app, event| {
        let action = event.id().as_ref();
        if ["manager", "new", "close"].contains(&action) {
            if let Some(shell) = app.get_webview("main") {
                let _ = shell.emit("tab-action", serde_json::json!({"action":action}));
            }
            reveal(app);
        }
    });
    if let Some(window) = app.get_window("main") {
        let handle = app.handle().clone();
        let owned = window.clone();
        window.on_window_event(move |event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                handle
                    .state::<Connections>()
                    .window_hidden
                    .store(true, Ordering::SeqCst);
                let _ = owned.hide();
            }
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                layout(&handle)
            }
            _ => {}
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tabs_keep_identity_choose_neighbor_and_cancel_pending_on_close() {
        let mut tabs = TabRegistry::default();
        let a = tabs.begin("a", "A", 10);
        let b = tabs.begin("b", "B", 11);
        assert_eq!(
            tabs.begin("a", "A", 12).generation.load(Ordering::SeqCst),
            10
        );
        tabs.activate(Some("b")).unwrap();
        let closed = tabs.close("b").unwrap();
        assert!(closed.cancelled.load(Ordering::SeqCst));
        assert_eq!(tabs.snapshot().active_id.as_deref(), Some("a"));
        tabs.close("a");
        assert!(a.cancelled.load(Ordering::SeqCst));
        assert!(tabs.snapshot().active_id.is_none());
        assert!(b.cancelled.load(Ordering::SeqCst));
    }
    #[test]
    fn stale_completion_cannot_restore_closed_or_replaced_tab() {
        let mut tabs = TabRegistry::default();
        let a = tabs.begin("a", "A", 1);
        tabs.close("a");
        let b = tabs.begin("a", "A", 2);
        assert!(!tabs.finish(&a, 1, true));
        assert!(tabs.finish(&b, 2, true));
        assert_eq!(tabs.snapshot().tabs[0].generation, 2);
        assert_eq!(tabs.snapshot().tabs[0].state, "ready");
    }
    #[test]
    fn failed_tabs_reopen_on_the_same_webview_session_with_a_new_generation() {
        let mut tabs = TabRegistry::default();
        let first = tabs.begin("a", "A", 1);
        assert!(tabs.finish(&first, 1, true));
        assert!(tabs.mark_failed(&first, 1));
        assert_eq!(tabs.snapshot().active_id, None);
        let retry = tabs.begin("a", "A", 2);
        assert_eq!(retry.state.lock().unwrap().as_str(), "opening");
        assert_eq!(retry.generation.load(Ordering::SeqCst), 2);
        assert!(Arc::ptr_eq(&first, &retry));
        assert!(!tabs.finish(&first, 1, true));
        assert_eq!(tabs.snapshot().tabs[0].state, "opening");
        assert!(ensure_active(&retry).is_ok());
        assert!(!tabs.mark_failed(&retry, 1));
        tabs.close("a");
        assert!(ensure_active(&retry).is_err());
    }
    #[test]
    fn layout_keeps_header_constant_across_retina_and_small_window() {
        assert_eq!(content_layout(1440., 900.), (0., 56., 1440., 844.));
        assert_eq!(content_layout(500., 40.), (0., 56., 500., 0.));
    }
}
