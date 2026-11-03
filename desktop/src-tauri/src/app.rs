use cokernel_desktop::host_client::{ClientError, DesktopClient, Observation};
use cokernel_protocol::ResponseEnvelope;
use serde_json::Value;
use tauri::{
    Manager, State,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

#[tauri::command]
pub async fn host_call(
    client: State<'_, DesktopClient>,
    method: String,
    params: Value,
) -> Result<ResponseEnvelope, ClientError> {
    client.call(&method, params).await
}
#[tauri::command]
pub fn host_observation(client: State<'_, DesktopClient>) -> Observation {
    client.observation()
}

/// Credentials never pass through WebView arguments, results or frontend state.
#[tauri::command]
pub async fn configure_remote() -> Result<serde_json::Value, String> {
    use cokernel_host::{
        ipc_pipe::{PipeClient, default_name},
        remote_setup::prompt_credentials,
    };
    static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = BUSY
        .try_lock()
        .map_err(|_| "接続設定画面は既に開いています。")?;
    let mut client = PipeClient::connect(&default_name().map_err(|_| "Hostを確認できません。")?)
        .await
        .map_err(|_| "Hostに接続できません。")?;
    let credentials = tokio::task::spawn_blocking(prompt_credentials)
        .await
        .map_err(|_| "設定画面を開けません。")?
        .map_err(|_| "設定を読み取れません。Tunnel IDとキーを確認してください。")?;
    let Some(credentials) = credentials else {
        return Ok(serde_json::json!({"cancelled":true}));
    };
    client
        .configure_remote(&credentials)
        .await
        .map_err(|_| "保存結果が不明です。設定状態を確認してください。")?;
    drop(credentials);
    let started = client
        .call("remote_access.start", serde_json::json!({}))
        .await
        .map_err(|_| "保存後の接続結果が不明です。接続状態を確認してください。")?;
    if !started.ok {
        return Err(
            "設定を保存しましたが、接続開始に失敗しました。状態を確認してください。".into(),
        );
    }
    Ok(serde_json::json!({"saved":true,"connection_requested":true}))
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            host_call,
            host_observation,
            configure_remote
        ])
        .setup(|app| {
            let client = tauri::async_runtime::block_on(async { DesktopClient::start() })?;
            app.manage(client);
            let open = MenuItem::with_id(app, "open", "CoKernelを開く", true, None::<&str>)?;
            let quit =
                MenuItem::with_id(app, "quit", "UIを終了（Sessionは継続）", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let icon = tauri::image::Image::new_owned([37, 99, 235, 255].repeat(32 * 32), 32, 32);
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("CoKernel — SessionはHostで継続")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
