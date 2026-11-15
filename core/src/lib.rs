mod alerts;
mod commands;
mod error;
mod labels;
mod metrics;
mod settings;
mod smartthings;
mod state;
mod tray;

use tauri::{Emitter, Manager, WindowEvent};

use crate::state::AppState;

const MAIN_WINDOW: &str = "main";

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_settings,
            commands::save_settings,
            commands::save_token,
            commands::list_devices,
            commands::send_command,
            commands::get_history,
            commands::get_labels,
            commands::open_token_page,
        ])
        .setup(|app| {
            // Menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let state = AppState::init(&handle)?;
            let needs_setup = state.needs_setup();
            let appearance = state.settings().appearance;
            app.manage(state);
            apply_appearance(&handle, appearance);
            tray::create(&handle)?;
            tauri::async_runtime::spawn(state::poll(handle.clone()));
            tauri::async_runtime::spawn(tray::animate(handle.clone()));
            if needs_setup {
                show_window(&handle, Some("settings"));
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app alive in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.hide() {
                    eprintln!("falha ao esconder a janela: {e}");
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o WindFree");
}

/// Shows the main window, optionally switching it to a tab (`control`, `metrics`, `settings`).
pub(crate) fn show_window(app: &tauri::AppHandle, tab: Option<&str>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return };
    let mut result = window.show().and_then(|()| window.unminimize()).and_then(|()| window.set_focus());
    if let Some(tab) = tab {
        result = result.and_then(|()| window.emit("navigate", tab));
    }
    if let Err(e) = result {
        eprintln!("falha ao abrir a janela: {e}");
    }
}

/// Sets the native window theme; the webview's `prefers-color-scheme` and the traffic lights follow it.
pub(crate) fn apply_appearance(app: &tauri::AppHandle, appearance: settings::Appearance) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return };
    if let Err(e) = window.set_theme(appearance.theme()) {
        eprintln!("falha ao aplicar o tema: {e}");
    }
}
