//! Native export I/O for the local trusted shell only.
//!
//! Export text leaves through a native save dialog or the system clipboard.
//! No arbitrary filesystem path crosses the IPC boundary: the desktop save
//! path comes exclusively from the OS dialog, and Android uses SAF. The
//! config source opener has a fixed URL constant — the webview can never
//! choose a URL. No export text is ever logged.

use tauri::Webview;

/// Fixed download/source site for the shared node JSON. Not an IPC parameter.
pub const CONFIG_SOURCE_URL: &str = "https://dsh-token.zcj123v.online/";
/// Hard upper bound for export text, in bytes.
pub const MAX_EXPORT_BYTES: usize = 1024 * 1024;

pub(crate) fn validate_export_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("导出内容为空".into());
    }
    if text.len() > MAX_EXPORT_BYTES {
        return Err("导出内容超过 1MiB 上限".into());
    }
    Ok(())
}

fn config_source_url() -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(CONFIG_SOURCE_URL).map_err(|_| "无法打开配置来源".to_owned())?;
    // Defence in depth: even if the constant is edited later, it must stay a
    // plain HTTPS root URL with no credentials, query, or fragment.
    if url.scheme() != "https"
        || url.host_str() != Some("dsh-token.zcj123v.online")
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("配置来源地址无效".into());
    }
    Ok(url)
}

/// Opens the fixed token/config website in the system browser.
/// Accepts no URL from the caller; requires the local shell webview.
#[tauri::command]
pub async fn open_config_source(window: Webview, app: tauri::AppHandle) -> Result<(), String> {
    super::require_shell(&window)?;
    let url = config_source_url()?;
    #[cfg(not(target_os = "android"))]
    {
        // Same guarded opener used for external links from connection windows.
        super::open_external(&app, &url);
        Ok(())
    }
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|_| "无法打开配置来源".to_owned())
    }
}

/// Saves `text` through a native save dialog.
/// `Ok(true)` = file written; `Ok(false)` = the user cancelled the dialog.
#[tauri::command]
pub async fn save_config_file(
    window: Webview,
    app: tauri::AppHandle,
    text: String,
) -> Result<bool, String> {
    super::require_shell(&window)?;
    validate_export_text(&text)?;
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_dialog::DialogExt;
        let picked = app
            .dialog()
            .file()
            .add_filter("JSON", &["json"])
            .set_file_name("dsh-config.json")
            .blocking_save_file();
        let Some(picked) = picked else {
            return Ok(false);
        };
        // Only paths that the OS dialog itself returned; remote/URL picks are refused.
        let path = picked
            .into_path()
            .map_err(|_| "所选保存位置不受支持".to_owned())?;
        std::fs::write(&path, text.as_bytes()).map_err(|_| "无法写入所选文件".to_owned())?;
        Ok(true)
    }
    #[cfg(target_os = "android")]
    {
        // SAF ACTION_CREATE_DOCUMENT via the content plugin; the UI never sees a path.
        let value =
            super::android::call_value(&app, "saveConfig", serde_json::json!({ "text": text }))
                .await?;
        Ok(value
            .get("saved")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false))
    }
}

/// Copies `text` to the system clipboard. Returns nothing on success.
#[tauri::command]
pub async fn copy_config_text(
    window: Webview,
    app: tauri::AppHandle,
    text: String,
) -> Result<(), String> {
    super::require_shell(&window)?;
    validate_export_text(&text)?;
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        app.clipboard()
            .write_text(text)
            .map_err(|_| "无法写入系统剪贴板".to_owned())
    }
    #[cfg(target_os = "android")]
    {
        super::android::call(&app, "copyConfig", serde_json::json!({ "text": text })).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_text_bound_is_exactly_one_mib_and_rejects_empty() {
        assert!(validate_export_text("{}").is_ok());
        assert!(validate_export_text("").is_err());
        assert!(validate_export_text(&"x".repeat(MAX_EXPORT_BYTES)).is_ok());
        assert!(validate_export_text(&"x".repeat(MAX_EXPORT_BYTES + 1)).is_err());
        // Byte bound, not char count: multi-byte UTF-8 counts per byte.
        assert!(validate_export_text(&"é".repeat(MAX_EXPORT_BYTES / 2 + 1)).is_err());
    }

    #[test]
    fn config_source_is_the_exact_https_root_and_nothing_else() {
        assert_eq!(CONFIG_SOURCE_URL, "https://dsh-token.zcj123v.online/");
        let url = config_source_url().unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("dsh-token.zcj123v.online"));
        assert_eq!(url.path(), "/");
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        assert!(url.username().is_empty());
        assert!(url.password().is_none());
    }
}
