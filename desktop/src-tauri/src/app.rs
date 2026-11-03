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

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![host_call, host_observation])
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
