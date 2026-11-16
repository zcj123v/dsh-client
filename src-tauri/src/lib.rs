use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::{Manager, State};
struct SshProcesses(Mutex<Vec<Child>>);

#[tauri::command]
fn allocate_local_port() -> Result<u16, String> {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| format!("无法分配本地端口：{error}"))
}

#[tauri::command]
fn start_ssh_forward(args: Vec<String>, state: State<'_, SshProcesses>) -> Result<(), String> {
    if args.iter().any(|arg| arg.contains('\n') || arg.contains('\r')) {
        return Err("SSH 参数不能包含换行".to_owned());
    }
    let child = Command::new("ssh")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 SSH：{error}"))?;
    state.inner().0.lock().map_err(|_| "SSH 进程状态不可用".to_owned())?.push(child);
    Ok(())
}

#[tauri::command]
fn stop_ssh_forwards(state: State<'_, SshProcesses>) -> Result<(), String> {
    let mut processes = state.inner().0.lock().map_err(|_| "SSH 进程状态不可用".to_owned())?;
    for mut child in processes.drain(..) {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SshProcesses(Mutex::new(Vec::new())))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![allocate_local_port, start_ssh_forward, stop_ssh_forwards])
        .build(tauri::generate_context!())
        .expect("error while building DSH Client")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<SshProcesses>() {
                    let _ = stop_ssh_forwards(state);
                }
            }
        });
}
