use super::*;
#[cfg(not(target_os = "android"))]
use std::future::Future;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Endpoint {
    pub label: String,
    mode: String,
    url: Option<String>,
    // Deserialized for schema round-trips on Android; only desktop reads them.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    ssh_profile: Option<String>,
    #[cfg_attr(target_os = "android", allow(dead_code))]
    ssh_user: Option<String>,
    #[cfg_attr(target_os = "android", allow(dead_code))]
    ssh_config: Option<String>,
    #[serde(default = "default_port")]
    #[cfg_attr(target_os = "android", allow(dead_code))]
    remote_port: u16,
}
fn default_port() -> u16 {
    3080
}
impl Endpoint {
    #[cfg(not(target_os = "android"))]
    pub fn connection(&self, id: &str, name: &str) -> Connection {
        Connection {
            id: id.into(),
            name: name.into(),
            mode: self.mode.clone(),
            url: self.url.clone(),
            ssh_profile: self.ssh_profile.clone(),
            ssh_user: self.ssh_user.clone(),
            ssh_config: self.ssh_config.clone(),
            remote_port: self.remote_port,
            endpoints: None,
        }
    }
    #[cfg(not(target_os = "android"))]
    pub fn probe(&self) -> Result<(), String> {
        if self.mode == "direct" {
            probe_url(&direct_url(self.url.as_deref().ok_or("缺少 DSH 地址")?)?)
        } else {
            Ok(())
        }
    }
}
#[cfg(not(target_os = "android"))]
pub fn validate_entries(record: &Connection) -> Result<Vec<Endpoint>, String> {
    connection_id(&record.id)?;
    if record.name.trim().is_empty() || record.name.len() > 160 {
        return Err("节点名称无效".into());
    }
    let entries = record.endpoints.clone().unwrap_or_else(|| {
        vec![Endpoint {
            label: "默认入口".into(),
            mode: record.mode.clone(),
            url: record.url.clone(),
            ssh_profile: record.ssh_profile.clone(),
            ssh_user: record.ssh_user.clone(),
            ssh_config: record.ssh_config.clone(),
            remote_port: record.remote_port,
        }]
    });
    if entries.is_empty() || entries.len() > 8 {
        return Err("每个节点需要 1–8 个入口".into());
    }
    for entry in &entries {
        if entry.label.trim().is_empty() || entry.label.chars().count() > 40 {
            return Err("入口名称无效".into());
        }
        match entry.mode.as_str() {
            "direct" => {
                direct_url(entry.url.as_deref().ok_or("缺少 DSH 地址")?)?;
            }
            "ssh-forward" => {
                if ![None, Some("default"), Some("fleet"), Some("none")]
                    .contains(&entry.ssh_config.as_deref())
                {
                    return Err("SSH 配置无效".into());
                }
                ssh_args(
                    entry.ssh_profile.as_deref().ok_or("缺少 SSH Host")?,
                    entry.ssh_user.as_deref().filter(|s| !s.is_empty()),
                    entry.remote_port,
                    45000,
                    None,
                )?;
            }
            _ => return Err("连接模式无效".into()),
        }
    }
    Ok(entries)
}
#[cfg(not(target_os = "android"))]
pub fn probe_url(url: &tauri::Url) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .connect_timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| "无法创建连接探测")?;
    let response = client
        .get(url.clone())
        .send()
        .map_err(|_| "入口不可达或 TLS 验证失败")?;
    let code = response.status().as_u16();
    if (200..400).contains(&code) || [401, 403].contains(&code) {
        Ok(())
    } else {
        Err("入口没有返回可用响应".into())
    }
}
#[cfg(not(target_os = "android"))]
pub async fn try_in_order<T, F, Fut>(count: usize, mut attempt: F) -> Result<(usize, T), String>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    for index in 0..count {
        if let Ok(value) = attempt(index).await {
            return Ok((index, value));
        }
    }
    Err("所有入口均未连接成功；请检查 LAN/WG 网络、HTTPS 证书或 SSH 密钥与转发权限".into())
}

/// One HTTPS entry candidate for Android, tagged with its ORIGINAL index in
/// `record.endpoints` so the UI can address the exact entry the shell opened.
#[cfg(any(target_os = "android", test))]
#[derive(Debug)]
pub(crate) struct HttpsCandidate {
    pub index: usize,
    /// Base URL without any token, used only for reachability probing.
    pub probe_url: String,
    /// Login URL (token already applied by `login_url`) handed to the isolated WebView.
    pub open_url: String,
}

#[cfg(test)]
fn fallback_entry(record: &Connection) -> Endpoint {
    Endpoint {
        label: "默认入口".into(),
        mode: record.mode.clone(),
        url: record.url.clone(),
        ssh_profile: record.ssh_profile.clone(),
        ssh_user: record.ssh_user.clone(),
        ssh_config: record.ssh_config.clone(),
        remote_port: record.remote_port,
    }
}
#[cfg(target_os = "android")]
fn fallback_entry(record: &Connection) -> Endpoint {
    Endpoint {
        label: "默认入口".into(),
        mode: record.mode.clone(),
        url: record.url.clone(),
        ssh_profile: None,
        ssh_user: None,
        ssh_config: None,
        remote_port: default_port(),
    }
}

/// Android selection from the ORIGINAL ordered endpoint list: SSH entries are
/// unsupported and skipped (never rewritten to HTTP), loopback HTTP is skipped
/// because Android only accepts HTTPS, and every surviving entry is validated
/// by the same native policy (`direct_url` + `login_url`) used elsewhere.
/// Order and original indices are preserved for real ordered fallback.
#[cfg(any(target_os = "android", test))]
pub(crate) fn android_https_entries(
    record: &Connection,
    token: Option<&str>,
) -> Result<Vec<HttpsCandidate>, String> {
    connection_id(&record.id)?;
    if record.name.trim().is_empty() || record.name.len() > 160 {
        return Err("节点名称无效".into());
    }
    let entries = record
        .endpoints
        .clone()
        .unwrap_or_else(|| vec![fallback_entry(record)]);
    if entries.is_empty() || entries.len() > 8 {
        return Err("每个节点需要 1–8 个入口".into());
    }
    for entry in &entries {
        if entry.label.trim().is_empty() || entry.label.chars().count() > 40 {
            return Err("入口名称无效".into());
        }
    }
    let mut candidates = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.mode != "direct" {
            continue;
        }
        let Some(raw) = entry.url.as_deref() else {
            continue;
        };
        let Ok(base) = direct_url(raw) else { continue };
        if base.scheme() != "https" {
            continue;
        }
        // Validate the transient login before anything is opened; a malformed
        // token is a user error and must not be silently skipped.
        let open = login_url(base, token)?;
        candidates.push(HttpsCandidate {
            index,
            probe_url: raw.to_owned(),
            open_url: open.to_string(),
        });
    }
    if candidates.is_empty() {
        return Err(
            "该节点没有 Android 可用的 HTTPS 入口；请先新增 HTTPS 入口（Android 不支持 SSH 转发）"
                .into(),
        );
    }
    Ok(candidates)
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;
    fn connection(value: serde_json::Value) -> Connection {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn validates_every_entry_before_trying_even_if_first_is_healthy() {
        let good = serde_json::json!({"id":"170ce412-3cbb-4467-a35a-b49fce65e8ef","name":"Test","mode":"direct","url":"http://127.0.0.1:3080/","remotePort":3080});
        let mut value = good.clone();
        value["endpoints"] = serde_json::json!([{"label":"LAN","mode":"direct","url":"https://good.example.test/","remotePort":3080},{"label":"bad","mode":"direct","url":"https://bad.example.test/?token=SECRET","remotePort":3080}]);
        assert!(validate_entries(&connection(value)).is_err());
        assert_eq!(
            validate_entries(&connection(good)).unwrap()[0].label,
            "默认入口"
        );
    }
    fn server(status: u16) -> (tauri::Url, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url =
            tauri::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buf = [0; 2048];
            let len = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..len]).to_ascii_lowercase();
            assert!(
                !request.contains("token=")
                    && !request.contains("authorization:")
                    && !request.contains("cookie:")
            );
            write!(stream,"HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nLocation: http://127.0.0.1:1/\r\nConnection: close\r\n\r\n").unwrap();
        });
        (url, handle)
    }
    #[test]
    fn probe_accepts_login_gate_without_following_redirects_and_rejects_gateway_errors() {
        for (status, expected) in [
            (200, true),
            (302, true),
            (401, true),
            (403, true),
            (404, false),
            (502, false),
        ] {
            let (url, thread) = server(status);
            assert_eq!(probe_url(&url).is_ok(), expected, "status {status}");
            thread.join().unwrap();
        }
    }
    #[test]
    fn actual_probe_order_falls_back_and_stops_at_login_gate() {
        let (bad, t1) = server(502);
        let (login, t2) = server(401);
        let urls = Arc::new(vec![
            bad,
            login,
            tauri::Url::parse("http://127.0.0.1:1/").unwrap(),
        ]);
        let calls = Arc::new(Mutex::new(vec![]));
        let observed = calls.clone();
        let result = tauri::async_runtime::block_on(try_in_order(3, move |index| {
            let url = urls[index].clone();
            observed.lock().unwrap().push(index);
            async move {
                tauri::async_runtime::spawn_blocking(move || probe_url(&url))
                    .await
                    .unwrap()
            }
        }))
        .unwrap();
        assert_eq!(result.0, 1);
        assert_eq!(*calls.lock().unwrap(), vec![0, 1]);
        t1.join().unwrap();
        t2.join().unwrap();
    }
    #[test]
    fn native_limits_and_later_ssh_injection_are_rejected() {
        let mut value = serde_json::json!({"id":"170ce412-3cbb-4467-a35a-b49fce65e8ef","name":"Test","mode":"direct","url":"http://127.0.0.1:3080/","remotePort":3080});
        let good =
            serde_json::json!({"label":"LAN","mode":"direct","url":"https://good.example.test/"});
        value["endpoints"] = serde_json::json!([good.clone(),{"label":"WG","mode":"ssh-forward","sshProfile":"-oProxyCommand=bad","remotePort":3080}]);
        assert!(validate_entries(&connection(value.clone())).is_err());
        value["endpoints"] = serde_json::json!(vec![good; 9]);
        assert!(validate_entries(&connection(value)).is_err());
    }
    #[test]
    fn ordered_attempts_stop_after_first_success_and_fail_with_no_success() {
        let calls = Arc::new(Mutex::new(vec![]));
        let calls2 = calls.clone();
        let result = tauri::async_runtime::block_on(try_in_order(3, move |index| {
            calls2.lock().unwrap().push(index);
            std::future::ready(if index == 1 {
                Ok(77)
            } else {
                Err("failed".into())
            })
        }))
        .unwrap();
        assert_eq!(result, (1, 77));
        assert_eq!(*calls.lock().unwrap(), vec![0, 1]);
        assert!(
            tauri::async_runtime::block_on(try_in_order::<u8, _, _>(2, |_| std::future::ready(
                Err("failed".into())
            )))
            .is_err()
        );
    }
}

#[cfg(test)]
mod android_selection_tests {
    use super::*;

    fn record(value: serde_json::Value) -> Connection {
        serde_json::from_value(value).unwrap()
    }
    fn base_record() -> serde_json::Value {
        serde_json::json!({"id":"170ce412-3cbb-4467-a35a-b49fce65e8ef","name":"Test","mode":"direct","url":"https://fallback.example.test/","remotePort":3080})
    }
    fn entry(label: &str, mode: &str, url: &str) -> serde_json::Value {
        serde_json::json!({"label":label,"mode":mode,"url":url})
    }

    #[test]
    fn android_selection_keeps_original_index_and_skips_ssh_and_loopback_http() {
        let mut value = base_record();
        value["endpoints"] = serde_json::json!([
            entry("LAN SSH", "ssh-forward", "https://unused.example.test/"),
            entry("本机", "direct", "http://127.0.0.1:3080/"),
            entry("WG HTTPS", "direct", "https://wg.example.test/"),
            entry("WG HTTPS 2", "direct", "https://wg2.example.test/")
        ]);
        let candidates = android_https_entries(&record(value), None).unwrap();
        assert_eq!(
            candidates.iter().map(|c| c.index).collect::<Vec<_>>(),
            vec![2, 3],
            "SSH and loopback HTTP must be skipped with original indices preserved"
        );
        assert_eq!(candidates[0].probe_url, "https://wg.example.test/");
        assert_eq!(candidates[0].open_url, "https://wg.example.test/");
    }

    #[test]
    fn android_selection_rejects_ssh_only_nodes_instead_of_falling_back_to_http() {
        let mut value = base_record();
        value["endpoints"] = serde_json::json!([
            entry("LAN SSH", "ssh-forward", "https://unused.example.test/"),
            entry("LAN HTTP", "direct", "http://127.0.0.1:3080/")
        ]);
        let error = android_https_entries(&record(value), None).unwrap_err();
        assert!(
            error.contains("HTTPS"),
            "error must ask for an HTTPS entry: {error}"
        );
    }

    #[test]
    fn android_selection_puts_token_only_on_the_open_url_not_the_probe_url() {
        let mut value = base_record();
        value["endpoints"] =
            serde_json::json!([entry("WG HTTPS", "direct", "https://wg.example.test/")]);
        let candidates = android_https_entries(&record(value.clone()), Some("fake-token")).unwrap();
        assert_eq!(candidates[0].probe_url, "https://wg.example.test/");
        assert_eq!(
            candidates[0].open_url,
            "https://wg.example.test/?token=fake-token"
        );
        // A malformed token (full URL pasted instead of the token string) is a hard error.
        assert!(android_https_entries(&record(value), Some("https://evil.test/?token=x")).is_err());
    }

    #[test]
    fn android_selection_rejects_injected_or_out_of_range_entries() {
        let mut value = base_record();
        value["endpoints"] =
            serde_json::json!(vec![
                entry("WG HTTPS", "direct", "https://wg.example.test/");
                9
            ]);
        assert!(android_https_entries(&record(value), None).is_err());
        let mut value = base_record();
        value["endpoints"] = serde_json::json!([
            {"label":"bad url","mode":"direct","url":"https://user:pass@wg.example.test/"},
            {"label":"WG HTTPS","mode":"direct","url":"https://wg.example.test/"}
        ]);
        // Invalid entries are skipped, valid ones still selected.
        let candidates = android_https_entries(&record(value), None).unwrap();
        assert_eq!(
            candidates.iter().map(|c| c.index).collect::<Vec<_>>(),
            vec![1]
        );
        let mut value = base_record();
        value["name"] = serde_json::json!("");
        assert!(android_https_entries(&record(value), None).is_err());
    }

    #[test]
    fn android_selection_falls_back_to_the_single_legacy_url_record() {
        let candidates = android_https_entries(&record(base_record()), None).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].index, 0);
        assert_eq!(candidates[0].probe_url, "https://fallback.example.test/");
        // Legacy desktop-style loopback record has no Android candidate.
        let mut value = base_record();
        value["url"] = serde_json::json!("http://127.0.0.1:3080/");
        assert!(android_https_entries(&record(value), None).is_err());
    }
}
