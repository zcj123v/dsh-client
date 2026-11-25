//! Android content views deliberately do not use Tauri WebViews or IPC.
use serde_json::{Value, json};
use tauri::{
    Manager, Runtime,
    plugin::{Builder, PluginHandle, TauriPlugin},
};

struct Content<R: Runtime>(PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("dsh-content")
        .setup(|app, api| {
            let handle = api.register_android_plugin("com.zcj.dshclient", "DshContentPlugin")?;
            app.manage(Content(handle));
            Ok(())
        })
        .build()
}

pub async fn call(app: &tauri::AppHandle, command: &str, payload: Value) -> Result<(), String> {
    call_value(app, command, payload).await.map(|_| ())
}

/// Like [`call`] but returns the plugin's resolved JSON payload.
pub async fn call_value(
    app: &tauri::AppHandle,
    command: &str,
    payload: Value,
) -> Result<Value, String> {
    let response = app
        .state::<Content<tauri::Wry>>()
        .0
        .run_mobile_plugin_async::<Value>(command, payload)
        .await;
    match response {
        Ok(value) => Ok(value),
        Err(_) => Err("Android 操作失败：请更新系统 WebView，并确认 HTTPS 入口和存储空间".into()),
    }
}

pub async fn open(
    app: &tauri::AppHandle,
    record: super::Connection,
    token: Option<String>,
) -> Result<(), String> {
    if record.mode != "direct" {
        return Err("Android 请使用已有 HTTPS 入口；不支持系统 SSH".into());
    }
    let url = super::policy::android_url(
        record.url.as_deref().ok_or("缺少 HTTPS 地址")?,
        token.as_deref(),
    )?;
    call(
        app,
        "open",
        json!({ "id": record.id, "name": record.name, "url": url.as_str() }),
    )
    .await
}

/// Opens the first HTTPS entry that is actually reachable, probing the ordered
/// candidate list with the platform HTTPS stack (never sending the token).
/// Returns the candidate's ORIGINAL endpoint index for the shell UI.
pub async fn open_ordered(
    app: &tauri::AppHandle,
    record: &super::Connection,
    candidates: Vec<super::endpoints::HttpsCandidate>,
) -> Result<usize, String> {
    let payload = json!({
        "id": record.id,
        "name": record.name,
        "candidates": candidates.iter().map(|candidate| json!({
            "index": candidate.index,
            "probeUrl": candidate.probe_url,
            "openUrl": candidate.open_url,
        })).collect::<Vec<_>>(),
    });
    let value = call_value(app, "openOrdered", payload).await?;
    if !value
        .get("reached")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("所有 HTTPS 入口均未连接成功；请检查网络、HTTPS 证书或新增可用入口".into());
    }
    value
        .get("index")
        .and_then(Value::as_u64)
        .map(|index| index as usize)
        .ok_or_else(|| "Android 打开失败：未返回所选入口".to_owned())
}
