use crate::policy::same_origin;
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::{CONTENT_RANGE, COOKIE, RANGE},
    redirect::Policy,
};
use serde::Deserialize;
use std::{
    ffi::CString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Manager, Url, Webview};
use tauri_plugin_dialog::DialogExt;
use tempfile::NamedTempFile;

const CHUNK_SIZE: u64 = 1024 * 1024;
const READ_BUFFER_SIZE: usize = 64 * 1024;
const MAX_REDIRECTS: usize = 5;
const RENAME_EXCL: u32 = 0x0000_0004;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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

pub fn save(
    app: &AppHandle,
    webview: &Webview,
    request: DownloadRequest,
) -> Result<SaveOutcome, String> {
    let id = webview
        .label()
        .strip_prefix("dsh-")
        .ok_or("仅已登记节点可下载文件")?;
    let origin = app
        .state::<super::Connections>()
        .urls
        .lock()
        .map_err(|_| "连接状态不可用")?
        .get(id)
        .cloned()
        .ok_or("未知节点或连接已关闭")?;
    let source = validate_source(&request.url, Some(&origin))?;
    let current = webview.url().map_err(|_| "无法验证节点页面来源")?;
    if !same_origin(&origin, &current) {
        return Err("节点页面来源已变化，请重试".into());
    }
    let name = safe_file_name(request.name.as_deref().unwrap_or("download"));
    let (destination, overwrite) = match request.mode.as_str() {
        "download" => {
            let directory = app
                .path()
                .download_dir()
                .map_err(|_| "无法定位系统下载目录")?;
            (next_available(&directory, &name)?, false)
        }
        "saveAs" => {
            let window = webview.window();
            let Some(path) = app
                .dialog()
                .file()
                .set_parent(&window)
                .set_file_name(&name)
                .blocking_save_file()
            else {
                return Ok(SaveOutcome::Cancelled);
            };
            (path.into_path().map_err(|_| "另存为路径无效")?, true)
        }
        _ => return Err("下载模式无效".into()),
    };
    let cookies = webview
        .cookies_for_url(source.clone())
        .map_err(|_| "无法读取节点会话 Cookie")?;
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
    download_to_path(&source, &origin, cookie.as_deref(), &destination, overwrite)?;
    Ok(SaveOutcome::Saved(destination))
}

fn validate_source(raw: &str, registered_origin: Option<&Url>) -> Result<Url, String> {
    let origin = registered_origin.ok_or("未知节点或连接已关闭")?;
    let source = Url::parse(raw).map_err(|_| "下载 URL 无效")?;
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
        ["token", "password", "secret", "authorization"]
            .contains(&key.to_ascii_lowercase().as_str())
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
    let source = validate_source(source.as_str(), Some(origin))?;
    let directory = destination.parent().ok_or("下载目标目录无效")?;
    let mut temporary = NamedTempFile::with_prefix_in(".dsh-download-", directory)
        .map_err(|_| "无法在下载目录创建临时文件")?;
    let result = stream_download(&source, origin, cookie, temporary.as_file_mut());
    if result.is_ok() {
        temporary
            .as_file_mut()
            .sync_all()
            .map_err(|_| "无法同步下载临时文件")?;
        atomic_rename(temporary.path(), destination, overwrite)?;
    }
    result
}

fn stream_download(
    source: &Url,
    origin: &Url,
    cookie: Option<&str>,
    output: &mut fs::File,
) -> Result<(), String> {
    let redirect_origin = origin.clone();
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(Policy::custom(move |attempt| {
            if !same_origin(&redirect_origin, attempt.url()) {
                attempt.error("下载重定向不得跨 origin")
            } else if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("下载重定向次数过多")
            } else {
                attempt.follow()
            }
        }))
        .no_proxy()
        .build()
        .map_err(|_| "无法创建下载请求")?;

    let mut offset = 0_u64;
    let mut expected_total = None;
    loop {
        let block_end = offset.checked_add(CHUNK_SIZE - 1).ok_or("下载文件过大")?;
        let end = expected_total.map_or(block_end, |total: u64| block_end.min(total - 1));
        let mut request = client
            .get(source.clone())
            .header(RANGE, format!("bytes={offset}-{end}"));
        if let Some(cookie) = cookie {
            request = request.header(COOKIE, cookie);
        }
        let response = request.send().map_err(|_| "下载请求失败")?;
        match response.status() {
            StatusCode::OK if offset == 0 => {
                copy_bounded(response, output, None)?;
                return Ok(());
            }
            StatusCode::PARTIAL_CONTENT => {
                let content_range = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .ok_or("206 响应缺少 Content-Range")?
                    .to_str()
                    .map_err(|_| "Content-Range 无效")?;
                let total = parse_content_range(content_range, offset, end)?;
                if expected_total.is_some_and(|expected| expected != total) {
                    return Err("下载文件总长度在分块间变化".into());
                }
                expected_total = Some(total);
                copy_bounded(response, output, Some(end - offset + 1))?;
                if end + 1 == total {
                    return Ok(());
                }
                offset = end + 1;
            }
            _ => return Err("下载服务返回了不符合请求范围的响应".into()),
        }
    }
}

fn parse_content_range(value: &str, expected_start: u64, expected_end: u64) -> Result<u64, String> {
    let range = value
        .strip_prefix("bytes ")
        .ok_or("Content-Range 单位无效")?;
    let (range, total) = range.split_once('/').ok_or("Content-Range 无效")?;
    let (start, end) = range.split_once('-').ok_or("Content-Range 无效")?;
    let start = start.parse::<u64>().map_err(|_| "Content-Range 起点无效")?;
    let end = end.parse::<u64>().map_err(|_| "Content-Range 终点无效")?;
    let total = total.parse::<u64>().map_err(|_| "Content-Range 总长无效")?;
    if start != expected_start || end != expected_end || end < start {
        return Err("Content-Range 与请求范围不一致".into());
    }
    if total <= end {
        return Err("Content-Range 总长无效".into());
    }
    Ok(total)
}

fn copy_bounded(
    mut response: Response,
    output: &mut fs::File,
    expected_length: Option<u64>,
) -> Result<(), String> {
    let mut buffer = [0_u8; READ_BUFFER_SIZE];
    let mut written = 0_u64;
    loop {
        let count = response
            .read(&mut buffer)
            .map_err(|error| format!("下载读取失败：{error}"))?;
        if count == 0 {
            break;
        }
        written = written.checked_add(count as u64).ok_or("下载文件过大")?;
        if expected_length.is_some_and(|expected| written > expected) {
            return Err("服务返回了超出 Range 的数据".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| "下载写入失败")?;
    }
    if expected_length.is_some_and(|expected| expected != written) {
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
