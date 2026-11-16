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
    let response = app
        .state::<Content<tauri::Wry>>()
        .0
        .run_mobile_plugin_async::<Value>(command, payload)
        .await;
    match response {
        Ok(_) => Ok(()),
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
