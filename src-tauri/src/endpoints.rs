use super::*;
use std::future::Future;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Endpoint {
    pub label: String,
    mode: String,
    url: Option<String>,
    ssh_profile: Option<String>,
    ssh_user: Option<String>,
    ssh_config: Option<String>,
    #[serde(default = "default_port")]
    remote_port: u16,
}
fn default_port() -> u16 {
    3080
}
impl Endpoint {
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
    pub fn probe(&self) -> Result<(), String> {
        if self.mode == "direct" {
            probe_url(&direct_url(self.url.as_deref().ok_or("缺少 DSH 地址")?)?)
        } else {
            Ok(())
        }
    }
}
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

#[cfg(test)]
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
