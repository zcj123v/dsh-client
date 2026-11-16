use sha2::{Digest, Sha256};
use tauri::Url;

pub fn connection_id(id: &str) -> Result<(), String> {
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err("连接 ID 无效".into());
    }
    Ok(())
}
pub fn partition_id(id: &str) -> [u8; 16] {
    let digest = Sha256::digest(id.as_bytes());
    digest[..16].try_into().unwrap()
}
pub fn local_port(id: &str) -> u16 {
    let bytes = partition_id(id);
    40000 + u16::from_be_bytes([bytes[0], bytes[1]]) % 20000
}
pub fn direct_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|_| "DSH 地址无效".to_owned())?;
    let loopback = [Some("127.0.0.1"), Some("localhost"), Some("[::1]")].contains(&url.host_str());
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || url.host_str().is_none()
    {
        return Err("远程地址需要 HTTPS；HTTP 仅允许本机 loopback".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("保存地址不能含凭据或查询参数".into());
    }
    Ok(url)
}
pub fn same_origin(base: &Url, target: &Url) -> bool {
    base.origin() == target.origin()
}
pub fn login_url(mut base: Url, token: Option<&str>) -> Result<Url, String> {
    if let Some(value) = token.filter(|t| !t.is_empty()) {
        if value.len() > 4096
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b))
        {
            return Err("请仅粘贴 token 字符串，不要粘贴完整 URL".into());
        }
        base.set_path("/");
        base.query_pairs_mut().append_pair("token", value);
    }
    Ok(base)
}
pub fn ssh_args(
    profile: &str,
    user: Option<&str>,
    remote: u16,
    local: u16,
    config: Option<&str>,
) -> Result<Vec<String>, String> {
    let safe = |s: &str| {
        !s.is_empty()
            && s.len() < 256
            && s.as_bytes()[0].is_ascii_alphanumeric()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._@:-".contains(&b))
    };
    if !safe(profile)
        || user.is_some_and(|u| !safe(u) || u.contains('@') || u.contains(':'))
        || remote == 0
        || local == 0
    {
        return Err("SSH Host、用户或端口无效".into());
    }
    let mut args = vec![
        "-N",
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ConnectTimeout=8",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    if let Some(path) = config {
        args.extend(["-F".into(), path.into()]);
    }
    args.extend([
        "-L".into(),
        format!("127.0.0.1:{local}:127.0.0.1:{remote}"),
        user.map(|u| format!("{u}@{profile}"))
            .unwrap_or_else(|| profile.into()),
    ]);
    Ok(args)
}

pub fn transport_response(response: &[u8]) -> bool {
    response.starts_with(b"HTTP/")
        && matches!(
            response.get(9..12),
            Some(b"200" | b"301" | b"302" | b"303" | b"307" | b"308" | b"401" | b"403")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_gateway_errors_without_mistaking_login_gate_for_health() {
        assert!(transport_response(b"HTTP/1.1 401"));
        assert!(transport_response(b"HTTP/1.1 200"));
        assert!(!transport_response(b"HTTP/1.1 502"));
        assert!(!transport_response(b"HTTP/1.1 404"));
        assert!(!transport_response(b"garbage"));
    }
    #[test]
    fn rejects_credentials_and_insecure_remote_urls() {
        for raw in [
            "https://user:pass@dsh.test/",
            "https://dsh.test/?token=secret",
            "file:///tmp/test",
            "http://10.250.0.13:3080/",
        ] {
            assert!(direct_url(raw).is_err());
        }
        assert!(direct_url("http://127.0.0.1:3080/").is_ok());
        assert!(direct_url("https://dsh.test/").is_ok());
    }
    #[test]
    fn native_ssh_validation_rejects_option_and_path_injection() {
        for profile in ["-oProxyCommand=evil", "bad;host", "../key", "a b"] {
            assert!(ssh_args(profile, None, 3080, 45001, None).is_err());
        }
        let args = ssh_args("dsh-air", Some("zcj"), 3080, 45001, None).unwrap();
        assert_eq!(args.last().unwrap(), "zcj@dsh-air");
        assert!(args.contains(&"127.0.0.1:45001:127.0.0.1:3080".to_owned()));
        assert!(args.contains(&"StrictHostKeyChecking=yes".to_owned()));
        assert!(ssh_args("dsh-air", None, 0, 45001, None).is_err());
    }
    #[test]
    fn origin_guard_and_partition_are_exact() {
        let base = tauri::Url::parse("https://dsh.test/").unwrap();
        assert!(same_origin(
            &base,
            &tauri::Url::parse("https://dsh.test/session/1").unwrap()
        ));
        for target in [
            "https://evil.test/",
            "https://dsh.test:444/",
            "http://dsh.test/",
            "tauri://localhost/",
        ] {
            assert!(!same_origin(&base, &tauri::Url::parse(target).unwrap()));
        }
        assert!(connection_id("../other").is_err());
        assert!(connection_id("170ce412-3cbb-4467-a35a-b49fce65e8ef").is_ok());
        assert_ne!(
            partition_id("170ce412-3cbb-4467-a35a-b49fce65e8ef"),
            partition_id("170ce412-3cbb-4467-a35a-b49fce65e8e0")
        );
    }
    #[test]
    fn token_only_enters_transient_root_url() {
        let url = login_url(
            direct_url("https://dsh.test/").unwrap(),
            Some("private-value"),
        )
        .unwrap();
        assert_eq!(url.query(), Some("token=private-value"));
        assert!(
            login_url(
                direct_url("https://dsh.test/").unwrap(),
                Some("https://evil.test/?token=x")
            )
            .is_err()
        );
    }
}
