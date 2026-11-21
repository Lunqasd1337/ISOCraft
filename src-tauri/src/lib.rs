mod commands;
mod pipeline;
mod state;

pub use state::AppState;

use isocraft_core::logger::SessionLogger;
use isocraft_core::process::ProcessGuard;
use isocraft_core::settings::{load_settings, webview_user_data_dir};
use state::{apply_window_theme, log_and_emit};
use std::sync::Mutex;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_settings = load_settings();
    let initial_theme = initial_settings.theme.clone();
    let app_state = AppState {
        settings: Mutex::new(initial_settings),
        logger: SessionLogger::new(),
        process_guard: ProcessGuard::new(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .setup(move |app| {
            let webview_dir = webview_user_data_dir();
            std::fs::create_dir_all(&webview_dir)?;

            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("ISOCraft")
                .inner_size(1120.0, 740.0)
                .min_inner_size(920.0, 620.0)
                .decorations(false)
                .center()
                .data_directory(webview_dir)
                .build()?;

            apply_window_theme(app.handle(), &initial_theme);

            let app_handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let state = app_handle.state::<AppState>();
                    if state.process_guard.is_busy() {
                        api.prevent_close();
                        log_and_emit(
                            &state,
                            &app_handle,
                            "warn",
                            "Close requested while an image operation is running; prompting confirmation.",
                        );
                        let _ = app_handle.emit("close-requested-while-busy", ());
                    }
                }
            });

            let state = app.state::<AppState>();
            log_and_emit(&state, app.handle(), "info", "ISOCraft initialized.");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::check_environment,
            commands::get_settings,
            commands::save_settings,
            commands::set_app_theme,
            commands::get_logs,
            commands::export_diagnostics,
            commands::inspect_image,
            commands::load_image_catalog,
            commands::scan_appx_dependencies,
            commands::scan_update_path,
            commands::search_update_catalog,
            commands::download_catalog_update,
            commands::cancel_catalog_download,
            commands::scan_driver_folder,
            commands::scan_custom_file_path,
            commands::preview_reg_file,
            commands::validate_autounattend,
            commands::export_preset,
            commands::import_preset,
            commands::open_external_url,
            commands::start_build,
            commands::cancel_build,
            commands::recover_workspace,
            commands::force_cancel_and_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
