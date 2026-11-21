use crate::state::{
    apply_window_theme, cleanup_session_dir_if_clean, log_and_emit, AppState, BuildConfig,
};
use isocraft_core::apps::{scan_appx_dependencies as core_scan_deps, DependencyScanResult};
use isocraft_core::dism::{load_image_catalog as core_load_catalog, ImageCatalog};
use isocraft_core::drivers::{scan_driver_path as core_scan_driver_path, DriverScanResult};
use isocraft_core::env::{
    check_environment_status, find_bundled_or_path, find_curl, find_dism, find_explorer,
    EnvironmentStatus,
};
use isocraft_core::files::{scan_custom_file_path as core_scan_custom_file_path, FileScanResult};
use isocraft_core::image::{
    inspect_image as core_inspect_image, ImageInspectionResult, SourceKind,
};
use isocraft_core::iso::{
    validate_autounattend as core_validate_autounattend, AutounattendValidation,
};
use isocraft_core::logger::LogEntry;
use isocraft_core::preset::{
    export_preset_to_file, import_preset_from_file, PresetImportResult, StudioPreset,
};
use isocraft_core::reg_parser::{preview_reg_file as core_preview_reg_file, RegFilePreview};
use isocraft_core::settings::{default_workspace_dir, save_settings_to_disk, AppSettings};
use isocraft_core::updates::{
    download_catalog_update as core_download_catalog_update,
    inspect_update_path as core_inspect_update_path,
    search_microsoft_catalog as core_search_microsoft_catalog, CatalogDownloadResult,
    CatalogUpdateEntry, UpdateScanInfo,
};
use isocraft_core::wim::{read_workspace_state, recover_workspace_async};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub(crate) fn check_environment(
    app: AppHandle,
    state: State<AppState>,
) -> Result<EnvironmentStatus, String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock poisoned".to_string())?
        .clone();
    let res_dir = app.path().resource_dir().ok();
    let status = check_environment_status(res_dir.as_deref(), &settings);
    log_and_emit(
        &state,
        &app,
        "info",
        format!(
            "Environment check: admin={}, dism={}, wimlib={}, xorriso={}, oscdimg={}, free={:.1} GB",
            status.is_admin,
            status.dism.found,
            status.wimlib.found,
            status.xorriso.found,
            status.oscdimg.found,
            status.free_space_gb
        ),
    );
    Ok(status)
}

#[tauri::command]
pub(crate) fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "Settings lock poisoned".to_string())
}

#[tauri::command]
pub(crate) fn save_settings(
    app: AppHandle,
    state: State<AppState>,
    new_settings: AppSettings,
) -> Result<AppSettings, String> {
    save_settings_to_disk(&new_settings)?;
    if let Ok(mut guard) = state.settings.lock() {
        *guard = new_settings.clone();
    }
    apply_window_theme(&app, &new_settings.theme);
    log_and_emit(
        &state,
        &app,
        "info",
        format!(
            "Settings saved (workspace: {}, theme: {}, lang: {})",
            new_settings.workspace_dir, new_settings.theme, new_settings.language
        ),
    );
    Ok(new_settings)
}

#[tauri::command]
pub(crate) fn set_app_theme(
    app: AppHandle,
    state: State<AppState>,
    theme: String,
) -> Result<(), String> {
    apply_window_theme(&app, &theme);
    if let Ok(mut guard) = state.settings.lock() {
        guard.theme = theme;
        let _ = save_settings_to_disk(&guard);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn get_logs(state: State<AppState>) -> Vec<LogEntry> {
    state.logger.snapshot()
}

#[tauri::command]
pub(crate) fn export_diagnostics(
    state: State<AppState>,
    dest_path: String,
) -> Result<String, String> {
    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .unwrap_or_else(|_| default_workspace_dir());
    state
        .logger
        .export_diagnostics(&PathBuf::from(dest_path), &workspace_dir)
}

#[tauri::command]
pub(crate) async fn inspect_image(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ImageInspectionResult, String> {
    let res_dir = app.path().resource_dir().ok();
    let wimlib = find_bundled_or_path(
        res_dir.as_deref(),
        &["wimlib/wimlib-imagex.exe", "wimlib-imagex.exe"],
        "wimlib-imagex.exe",
    );
    let wimlib_path = wimlib
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "wimlib-imagex.exe not found. Run scripts/fetch-tools.ps1.".to_string())?;

    log_and_emit(&state, &app, "info", format!("Inspecting image: {path}"));
    let result = core_inspect_image(&PathBuf::from(&path), &wimlib_path).await?;
    log_and_emit(
        &state,
        &app,
        if result.is_valid_win11_client_x64 {
            "info"
        } else {
            "warn"
        },
        format!(
            "Inspection finished: kind={:?}, editions={}, valid_win11_x64={}, reason={:?}",
            result.source_kind,
            result.editions.len(),
            result.is_valid_win11_client_x64,
            result.blocking_reason
        ),
    );
    Ok(result)
}

#[tauri::command]
pub(crate) async fn load_image_catalog(
    app: AppHandle,
    state: State<'_, AppState>,
    source_path: String,
    source_kind: SourceKind,
    index: u32,
    force_refresh: Option<bool>,
) -> Result<ImageCatalog, String> {
    let Some(_busy) = state.process_guard.try_enter_busy() else {
        return Err("Another image operation is currently running.".to_string());
    };

    let settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock poisoned".to_string())?
        .clone();
    let workspace_dir = PathBuf::from(&settings.workspace_dir);

    let dism_path = find_dism()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "dism.exe not found.".to_string())?;

    let res_dir = app.path().resource_dir().ok();
    let wimlib_path = find_bundled_or_path(
        res_dir.as_deref(),
        &["wimlib/wimlib-imagex.exe", "wimlib-imagex.exe"],
        "wimlib-imagex.exe",
    )
    .path
    .map(PathBuf::from)
    .ok_or_else(|| "wimlib-imagex.exe not found.".to_string())?;

    let res = core_load_catalog(
        &dism_path,
        &wimlib_path,
        &PathBuf::from(&source_path),
        &source_kind,
        index,
        &workspace_dir,
        force_refresh.unwrap_or(false),
        Some(&state.process_guard),
        |msg| log_and_emit(&state, &app, "info", msg),
    )
    .await;
    drop(_busy);

    if let Err(ref e) = res {
        log_and_emit(&state, &app, "error", format!("Catalog load failed: {e}"));
    }
    res
}

#[tauri::command]
pub(crate) fn scan_appx_dependencies(paths: Vec<String>) -> DependencyScanResult {
    core_scan_deps(&paths)
}

#[tauri::command]
pub(crate) fn scan_update_path(path: String) -> Result<UpdateScanInfo, String> {
    core_inspect_update_path(&PathBuf::from(path))
}

#[tauri::command]
pub(crate) async fn search_update_catalog(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    only_win11_x64: Option<bool>,
) -> Result<Vec<CatalogUpdateEntry>, String> {
    let curl_path = find_curl()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "curl.exe not found on this Windows installation.".to_string())?;
    let filter = only_win11_x64.unwrap_or(true);
    log_and_emit(
        &state,
        &app,
        "info",
        format!("Searching Microsoft Update Catalog for '{query}' (win11_x64={filter})..."),
    );

    let res = core_search_microsoft_catalog(&curl_path, &query, filter, None).await;
    match &res {
        Ok(entries) => log_and_emit(
            &state,
            &app,
            "info",
            format!(
                "Microsoft Update Catalog returned {} result(s).",
                entries.len()
            ),
        ),
        Err(e) => log_and_emit(
            &state,
            &app,
            "error",
            format!("Microsoft Update Catalog search failed: {e}"),
        ),
    }
    res
}

#[tauri::command]
pub(crate) async fn download_catalog_update(
    app: AppHandle,
    state: State<'_, AppState>,
    update_id: String,
    title: String,
    expected_size_bytes: Option<u64>,
) -> Result<CatalogDownloadResult, String> {
    let Some(_busy) = state.process_guard.try_enter_busy() else {
        return Err("Another image or download operation is currently running.".to_string());
    };

    let curl_path = find_curl()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "curl.exe not found on this Windows installation.".to_string())?;

    let workspace_dir = state
        .settings
        .lock()
        .map_err(|_| "Settings lock poisoned".to_string())?
        .workspace_dir
        .clone();
    let workspace_path = PathBuf::from(workspace_dir);

    log_and_emit(
        &state,
        &app,
        "info",
        format!("Starting Microsoft Update Catalog download: {title} ({update_id})"),
    );

    let res = core_download_catalog_update(
        &curl_path,
        &update_id,
        expected_size_bytes.unwrap_or(0),
        &workspace_path,
        Some(&state.process_guard),
        |msg| log_and_emit(&state, &app, "info", msg),
        |progress| {
            let _ = app.emit("catalog-download-progress", &progress);
        },
    )
    .await;

    drop(_busy);

    if let Err(ref e) = res {
        let level = if e.to_ascii_lowercase().contains("cancelled") {
            "warn"
        } else {
            "error"
        };
        log_and_emit(
            &state,
            &app,
            level,
            format!("Catalog update download failed: {e}"),
        );
    }

    res
}

#[tauri::command]
pub(crate) async fn cancel_catalog_download(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log_and_emit(
        &state,
        &app,
        "warn",
        "Catalog download cancellation requested; terminating curl...",
    );
    state.process_guard.kill_all_tracked_trees();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.process_guard.wait_until_idle(),
    )
    .await;
    Ok(())
}

#[tauri::command]
pub(crate) fn scan_driver_folder(path: String) -> Result<DriverScanResult, String> {
    core_scan_driver_path(&PathBuf::from(path))
}

#[tauri::command]
pub(crate) fn scan_custom_file_path(path: String) -> Result<FileScanResult, String> {
    core_scan_custom_file_path(&PathBuf::from(path))
}

#[tauri::command]
pub(crate) fn preview_reg_file(path: String) -> Result<RegFilePreview, String> {
    core_preview_reg_file(&PathBuf::from(path))
}

#[tauri::command]
pub(crate) fn validate_autounattend(path: String) -> AutounattendValidation {
    core_validate_autounattend(&PathBuf::from(path))
}

#[tauri::command]
pub(crate) fn export_preset(
    app: AppHandle,
    state: State<AppState>,
    path: String,
    preset: StudioPreset,
) -> Result<String, String> {
    let saved = export_preset_to_file(&PathBuf::from(&path), preset)?;
    log_and_emit(&state, &app, "info", format!("Preset exported: {saved}"));
    Ok(saved)
}

#[tauri::command]
pub(crate) fn import_preset(
    app: AppHandle,
    state: State<AppState>,
    path: String,
) -> Result<PresetImportResult, String> {
    let res = import_preset_from_file(&PathBuf::from(&path))?;
    if res.missing_paths.is_empty() {
        log_and_emit(
            &state,
            &app,
            "info",
            format!("Preset imported cleanly: {path}"),
        );
    } else {
        log_and_emit(
            &state,
            &app,
            "warn",
            format!(
                "Preset imported ({path}) with {} missing local path(s).",
                res.missing_paths.len()
            ),
        );
    }
    Ok(res)
}

#[tauri::command]
pub(crate) fn open_external_url(url: String) -> Result<(), String> {
    const ALLOWED_URLS: [&str; 5] = [
        "https://www.microsoft.com/software-download/windows11",
        "https://uupdump.net/",
        "https://www.catalog.update.microsoft.com/",
        "https://github.com/Lunqasd1337/ISOCraft",
        "https://github.com/Lunqasd1337/ISOCraft/releases",
    ];
    if !ALLOWED_URLS.contains(&url.as_str()) {
        return Err("Unsupported external URL.".to_string());
    }
    #[cfg(windows)]
    {
        let explorer = find_explorer()
            .path
            .ok_or_else(|| "explorer.exe not found in Windows directory.".to_string())?;
        std::process::Command::new(explorer)
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn start_build(
    app: AppHandle,
    state: State<'_, AppState>,
    config: BuildConfig,
) -> Result<String, String> {
    crate::pipeline::run_build_pipeline(app, state, config).await
}

#[tauri::command]
pub(crate) async fn cancel_build(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    log_and_emit(
        &state,
        &app,
        "warn",
        "Build cancellation requested; terminating active processes...",
    );
    state.process_guard.kill_all_tracked_trees();

    // Wait for the active build/catalog task to unwind its own /Discard and cleanup
    let waited = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        state.process_guard.wait_until_idle(),
    )
    .await;

    if waited.is_err() {
        log_and_emit(
            &state,
            &app,
            "warn",
            "Timed out waiting for active operation to unwind; forcing workspace recovery.",
        );
        state.process_guard.set_busy(false);
    }

    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .unwrap_or_else(|_| default_workspace_dir());
    let ws = read_workspace_state(&workspace_dir);
    if !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
        let _ = recover_workspace(app, state).await;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn recover_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let Some(_busy) = state.process_guard.try_enter_busy() else {
        return Err("Cannot recover workspace while an image operation is running.".to_string());
    };

    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .unwrap_or_else(|_| default_workspace_dir());

    let dism_path = find_dism()
        .path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dism.exe"));

    log_and_emit(
        &state,
        &app,
        "warn",
        "Starting workspace recovery (DISM cleanup & state reset)...",
    );

    recover_workspace_async(&dism_path, &workspace_dir, |msg| {
        log_and_emit(&state, &app, "info", msg);
    })
    .await?;

    let session_dir = workspace_dir.join("session_current");
    cleanup_session_dir_if_clean(&state, &app, &workspace_dir, &session_dir).await;

    log_and_emit(&state, &app, "info", "Workspace recovery completed.");
    Ok("recovered".to_string())
}

#[tauri::command]
pub(crate) async fn force_cancel_and_close(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log_and_emit(
        &state,
        &app,
        "warn",
        "Force closing requested: terminating active child processes and cleaning workspace...",
    );
    state.process_guard.kill_all_tracked_trees();
    let waited = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.process_guard.wait_until_idle(),
    )
    .await;
    if waited.is_err() {
        state.process_guard.set_busy(false);
    }

    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .unwrap_or_else(|_| default_workspace_dir());
    let ws = read_workspace_state(&workspace_dir);
    if !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
        let _ = recover_workspace(app.clone(), state).await;
    }

    app.exit(0);
    Ok(())
}
