use crate::state::{
    AppState, BuildConfig, apply_window_theme, cleanup_session_dir_if_clean, log_and_emit,
};
use isocraft_core::apps::{DependencyScanResult, scan_appx_dependencies as core_scan_deps};
use isocraft_core::dism::{ImageCatalog, load_image_catalog as core_load_catalog};
use isocraft_core::drivers::{
    DriverScanResult, HostDriverExportRequest, HostDriverExportResult, InstalledDriverInfo,
    export_host_drivers as core_export_host_drivers, list_host_drivers as core_list_host_drivers,
    scan_driver_path as core_scan_driver_path,
};
use isocraft_core::env::{
    EnvironmentStatus, WorkspaceStorageStats, check_environment_status,
    clear_workspace_storage as core_clear_workspace_storage, compute_workspace_storage_stats,
    find_bundled_or_path, find_curl, find_dism, find_explorer, find_pnputil, find_tar,
    find_winsqlite3, is_trusted_oscdimg_path, strip_windows_verbatim_prefix,
};
use isocraft_core::files::{FileScanResult, scan_custom_file_path as core_scan_custom_file_path};
use isocraft_core::image::{
    ImageInspectionResult, SourceKind, inspect_image as core_inspect_image,
};
use isocraft_core::iso::{
    AutounattendValidation, validate_autounattend as core_validate_autounattend,
};
use isocraft_core::logger::{LogEntry, SessionLogger};
use isocraft_core::post_setup::{
    PostSetupBuildPlan, PostSetupFileScanResult, PostSetupPreview, WingetSource,
    preview_post_setup_scripts as core_preview_post_setup,
    scan_post_setup_file as core_scan_post_setup_file,
};
use isocraft_core::preset::{
    PresetImportResult, StudioPreset, export_preset_to_file, import_preset_from_file,
};
use isocraft_core::process::OperationKind;
use isocraft_core::reg_parser::{RegFilePreview, preview_reg_file as core_preview_reg_file};
use isocraft_core::settings::{
    AppSettings, default_workspace_dir, save_settings_to_disk, validate_workspace_dir,
};
use isocraft_core::updates::{
    CatalogDownloadResult, CatalogUpdateEntry, UpdateScanInfo,
    download_catalog_update as core_download_catalog_update,
    inspect_update_path as core_inspect_update_path,
    search_microsoft_catalog as core_search_microsoft_catalog,
};
use isocraft_core::wim::{read_workspace_state_status, recover_workspace_async};
use isocraft_core::winget_catalog::{
    DEFAULT_SEARCH_LIMIT, WingetSearchResponse, search_winget_catalog as core_search_winget_catalog,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateProgress {
    pub event: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<u8>,
}

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
    mut new_settings: AppSettings,
) -> Result<AppSettings, String> {
    if let Ok(guard) = state.settings.lock() {
        if state.process_guard.is_busy()
            && !guard
                .workspace_dir
                .trim()
                .eq_ignore_ascii_case(new_settings.workspace_dir.trim())
        {
            return Err(
                "Cannot change workspace directory while an image operation is running."
                    .to_string(),
            );
        }
        if new_settings.window_width.is_none() {
            new_settings.window_width = guard.window_width;
        }
        if new_settings.window_height.is_none() {
            new_settings.window_height = guard.window_height;
        }
        if new_settings.window_maximized.is_none() {
            new_settings.window_maximized = guard.window_maximized;
        }
    }
    if let Some(ref oscdimg) = new_settings.oscdimg_path {
        let trimmed = oscdimg.trim();
        if !trimmed.is_empty() && !is_trusted_oscdimg_path(Path::new(trimmed)) {
            return Err(
                "Selected oscdimg.exe must be a valid Microsoft-signed binary inside a protected Program Files directory."
                    .to_string(),
            );
        }
    }
    validate_workspace_dir(&PathBuf::from(&new_settings.workspace_dir))?;
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
pub(crate) async fn export_diagnostics(
    state: State<'_, AppState>,
    dest_path: String,
) -> Result<String, String> {
    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .unwrap_or_else(|_| default_workspace_dir());
    let entries = state.logger.snapshot();
    tokio::task::spawn_blocking(move || {
        SessionLogger::export_diagnostics_from_entries(
            &entries,
            &PathBuf::from(dest_path),
            &workspace_dir,
        )
    })
    .await
    .map_err(|e| format!("Diagnostics export task failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn inspect_image(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ImageInspectionResult, String> {
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::ImageInspect)
    else {
        return Err("Another image operation is currently running.".to_string());
    };

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
    let result = core_inspect_image(
        &PathBuf::from(&path),
        &wimlib_path,
        Some(&state.process_guard),
    )
    .await?;
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
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::CatalogLoad)
    else {
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

    let preferred_lang = match settings.language.trim().to_ascii_lowercase().as_str() {
        "ru" => Some("ru-RU"),
        "en" => Some("en-US"),
        _ => None,
    };

    let res = core_load_catalog(
        &dism_path,
        &wimlib_path,
        &PathBuf::from(&source_path),
        &source_kind,
        index,
        &workspace_dir,
        force_refresh.unwrap_or(false),
        preferred_lang,
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
pub(crate) async fn scan_appx_dependencies(
    paths: Vec<String>,
) -> Result<DependencyScanResult, String> {
    tokio::task::spawn_blocking(move || core_scan_deps(&paths))
        .await
        .map_err(|e| format!("Dependency scan task failed: {e}"))
}

#[tauri::command]
pub(crate) async fn scan_update_path(path: String) -> Result<UpdateScanInfo, String> {
    tokio::task::spawn_blocking(move || core_inspect_update_path(&PathBuf::from(path)))
        .await
        .map_err(|e| format!("Update scan task failed: {e}"))?
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
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::CatalogDownload)
    else {
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
    if state
        .process_guard
        .cancel_operation_kind(OperationKind::CatalogDownload)
    {
        log_and_emit(
            &state,
            &app,
            "warn",
            "Catalog download cancellation requested; terminating curl...",
        );
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.process_guard.wait_until_idle(),
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn scan_driver_folder(path: String) -> Result<DriverScanResult, String> {
    tokio::task::spawn_blocking(move || core_scan_driver_path(&PathBuf::from(path)))
        .await
        .map_err(|e| format!("Driver scan task failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn list_host_drivers(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<InstalledDriverInfo>, String> {
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::DriverExport)
    else {
        return Err("Another image operation is currently running.".to_string());
    };

    log_and_emit(
        &state,
        &app,
        "info",
        "Querying third-party drivers installed on this PC...",
    );
    let res = core_list_host_drivers(Some(&state.process_guard)).await;
    drop(_busy);
    match &res {
        Ok(items) => log_and_emit(
            &state,
            &app,
            "info",
            format!("Found {} third-party driver(s) on this PC.", items.len()),
        ),
        Err(e) => log_and_emit(
            &state,
            &app,
            "error",
            format!("Failed querying host drivers: {e}"),
        ),
    }
    res
}

#[tauri::command]
pub(crate) async fn export_host_drivers(
    app: AppHandle,
    state: State<'_, AppState>,
    request: HostDriverExportRequest,
) -> Result<HostDriverExportResult, String> {
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::DriverExport)
    else {
        return Err("Another image operation is currently running.".to_string());
    };

    let dism_path = find_dism()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "dism.exe not found.".to_string())?;
    let pnputil_path = find_pnputil()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "pnputil.exe not found in Windows directory.".to_string())?;

    let workspace_dir = state
        .settings
        .lock()
        .map_err(|_| "Settings lock poisoned".to_string())?
        .workspace_dir
        .clone();
    let session_dir = PathBuf::from(&workspace_dir).join("session_current");

    let host_drivers = core_list_host_drivers(Some(&state.process_guard)).await?;
    let res = core_export_host_drivers(
        &dism_path,
        &pnputil_path,
        &request,
        &host_drivers,
        Some(&session_dir),
        Some(&state.process_guard),
        |msg| log_and_emit(&state, &app, "info", msg),
        |progress| {
            let _ = app.emit("driver-export-progress", &progress);
        },
    )
    .await;
    drop(_busy);

    match &res {
        Ok(out) => log_and_emit(
            &state,
            &app,
            "info",
            format!(
                "Host driver export finished: exported={}, failed={}, destination={}",
                out.exported_count,
                out.failed.len(),
                out.destination_dir
            ),
        ),
        Err(e) => {
            let level = if e.to_ascii_lowercase().contains("cancelled") {
                "warn"
            } else {
                "error"
            };
            log_and_emit(
                &state,
                &app,
                level,
                format!("Host driver export failed: {e}"),
            );
        }
    }
    res
}

#[tauri::command]
pub(crate) async fn cancel_driver_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if state
        .process_guard
        .cancel_operation_kind(OperationKind::DriverExport)
    {
        log_and_emit(
            &state,
            &app,
            "warn",
            "Host driver export cancellation requested...",
        );
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.process_guard.wait_until_idle(),
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn scan_custom_file_path(path: String) -> Result<FileScanResult, String> {
    tokio::task::spawn_blocking(move || core_scan_custom_file_path(&PathBuf::from(path)))
        .await
        .map_err(|e| format!("File scan task failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn scan_post_setup_file(path: String) -> Result<PostSetupFileScanResult, String> {
    tokio::task::spawn_blocking(move || core_scan_post_setup_file(&PathBuf::from(path)))
        .await
        .map_err(|e| format!("Post-Setup file scan task failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn preview_post_setup(
    plan: PostSetupBuildPlan,
    autounattend_path: Option<String>,
    target: Option<String>,
) -> Result<PostSetupPreview, String> {
    tokio::task::spawn_blocking(move || {
        let xml_path = autounattend_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let tgt = target.unwrap_or_else(|| "iso".to_string());
        core_preview_post_setup(&plan, xml_path.as_deref(), &tgt)
    })
    .await
    .map_err(|e| format!("Post-Setup preview task failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn search_winget_packages(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    source: WingetSource,
    force_refresh: Option<bool>,
    limit: Option<usize>,
) -> Result<WingetSearchResponse, String> {
    let curl_path = find_curl()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "curl.exe not found on this Windows installation.".to_string())?;
    let tar_path = find_tar()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "tar.exe not found on this Windows installation.".to_string())?;
    let winsqlite_path = find_winsqlite3()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "winsqlite3.dll not found on this Windows installation.".to_string())?;

    let workspace_dir = state
        .settings
        .lock()
        .map_err(|_| "Settings lock poisoned".to_string())?
        .workspace_dir
        .clone();
    let workspace_path = PathBuf::from(workspace_dir);

    let res = core_search_winget_catalog(
        &curl_path,
        &tar_path,
        &winsqlite_path,
        &workspace_path,
        &query,
        source,
        force_refresh.unwrap_or(false),
        limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
        &state.winget_index_lock,
        |msg| log_and_emit(&state, &app, "info", msg),
    )
    .await;

    if let Err(ref e) = res {
        log_and_emit(
            &state,
            &app,
            "error",
            format!("Winget catalog search failed: {e}"),
        );
    }
    res
}

#[tauri::command]
pub(crate) async fn preview_reg_file(path: String) -> Result<RegFilePreview, String> {
    tokio::task::spawn_blocking(move || core_preview_reg_file(&PathBuf::from(path)))
        .await
        .map_err(|e| format!("Registry preview task failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn validate_autounattend(path: String) -> Result<AutounattendValidation, String> {
    tokio::task::spawn_blocking(move || core_validate_autounattend(&PathBuf::from(path)))
        .await
        .map_err(|e| format!("Autounattend validation task failed: {e}"))
}

#[tauri::command]
pub(crate) async fn export_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    preset: StudioPreset,
) -> Result<String, String> {
    let path_clone = path.clone();
    let saved = tokio::task::spawn_blocking(move || {
        export_preset_to_file(&PathBuf::from(&path_clone), preset)
    })
    .await
    .map_err(|e| format!("Preset export task failed: {e}"))??;
    log_and_emit(&state, &app, "info", format!("Preset exported: {saved}"));
    Ok(saved)
}

#[tauri::command]
pub(crate) async fn import_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<PresetImportResult, String> {
    let path_clone = path.clone();
    let res =
        tokio::task::spawn_blocking(move || import_preset_from_file(&PathBuf::from(&path_clone)))
            .await
            .map_err(|e| format!("Preset import task failed: {e}"))??;
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
pub(crate) fn reveal_in_explorer(path: String) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty()
        || trimmed.contains('"')
        || trimmed.contains('\r')
        || trimmed.contains('\n')
        || trimmed.contains('\0')
    {
        return Err(format!("Path does not exist: {trimmed}"));
    }
    let target = std::path::Path::new(trimmed);
    if !target.exists() {
        return Err(format!("Path does not exist: {trimmed}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let explorer = find_explorer()
            .path
            .ok_or_else(|| "explorer.exe not found in Windows directory.".to_string())?;
        let canonical = std::fs::canonicalize(target)
            .map(|p| strip_windows_verbatim_prefix(&p))
            .unwrap_or_else(|_| target.to_path_buf());
        let display_path = canonical.to_string_lossy().replace('"', "");
        std::process::Command::new(explorer)
            .raw_arg(format!("/select,\"{display_path}\""))
            .spawn()
            .map_err(|e| format!("Failed to open Explorer: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn check_app_update(app: AppHandle) -> Result<Option<AppUpdateInfo>, String> {
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed initializing updater: {e}"))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("Failed checking for updates: {e}"))?;

    Ok(update.map(|u| AppUpdateInfo {
        version: u.version,
        date: u
            .date
            .map(|d| format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())),
        body: u.body,
    }))
}

#[tauri::command]
pub(crate) async fn install_app_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::AppUpdate)
    else {
        return Err("Another build or image operation is currently running.".to_string());
    };

    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .unwrap_or_else(|_| default_workspace_dir());
    let (ws, ws_corrupted) = read_workspace_state_status(&workspace_dir);
    if ws_corrupted || !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
        return Err(
            "Cannot install application update while mounts or registry hives are active. Run workspace recovery first."
                .to_string(),
        );
    }

    log_and_emit(&state, &app, "info", "Checking for application updates...");
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed initializing updater: {e}"))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("Failed checking for updates: {e}"))?
        .ok_or_else(|| "No application update is currently available.".to_string())?;

    log_and_emit(
        &state,
        &app,
        "info",
        format!("Downloading ISOCraft v{} update...", update.version),
    );

    let started = AtomicBool::new(false);
    let downloaded = Arc::new(AtomicU64::new(0));
    let total_known = Arc::new(AtomicU64::new(0));
    let last_progress_emit = Arc::new(Mutex::new(None::<Instant>));
    let downloaded_finish = Arc::clone(&downloaded);
    let total_finish = Arc::clone(&total_known);
    let app_chunk = app.clone();
    let app_finish = app.clone();

    update
        .download_and_install(
            move |chunk_len, content_len| {
                if !started.swap(true, Ordering::SeqCst) {
                    if let Some(total) = content_len {
                        total_known.store(total, Ordering::SeqCst);
                    }
                    let total_opt = content_len.filter(|&v| v > 0);
                    let _ = app_chunk.emit(
                        "app-update-progress",
                        AppUpdateProgress {
                            event: "started".to_string(),
                            downloaded_bytes: 0,
                            total_bytes: total_opt,
                            percent: total_opt.map(|_| 0),
                        },
                    );
                }

                let cur =
                    downloaded.fetch_add(chunk_len as u64, Ordering::SeqCst) + chunk_len as u64;
                let total_raw = total_known.load(Ordering::SeqCst);
                let total_opt = if total_raw > 0 { Some(total_raw) } else { None };
                let pct = total_opt
                    .map(|t| ((cur as f64 / t as f64) * 100.0).round().clamp(0.0, 100.0) as u8);
                let should_emit = {
                    let mut last_guard =
                        last_progress_emit.lock().unwrap_or_else(|e| e.into_inner());
                    let now = Instant::now();
                    match *last_guard {
                        Some(prev) if now.duration_since(prev) < Duration::from_millis(120) => {
                            false
                        }
                        _ => {
                            *last_guard = Some(now);
                            true
                        }
                    }
                };
                if should_emit {
                    let _ = app_chunk.emit(
                        "app-update-progress",
                        AppUpdateProgress {
                            event: "progress".to_string(),
                            downloaded_bytes: cur,
                            total_bytes: total_opt,
                            percent: pct,
                        },
                    );
                }
            },
            move || {
                let total_raw = total_finish.load(Ordering::SeqCst);
                let total_opt = if total_raw > 0 { Some(total_raw) } else { None };
                let _ = app_finish.emit(
                    "app-update-progress",
                    AppUpdateProgress {
                        event: "finished".to_string(),
                        downloaded_bytes: downloaded_finish.load(Ordering::SeqCst),
                        total_bytes: total_opt,
                        percent: Some(100),
                    },
                );
            },
        )
        .await
        .map_err(|e| format!("Failed downloading or installing update: {e}"))?;

    drop(_busy);
    log_and_emit(
        &state,
        &app,
        "info",
        "Application update installed; restarting...",
    );
    app.restart();
}

#[tauri::command]
pub(crate) async fn get_workspace_storage_stats(
    state: State<'_, AppState>,
) -> Result<WorkspaceStorageStats, String> {
    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .map_err(|_| "Settings lock poisoned".to_string())?;
    tokio::task::spawn_blocking(move || compute_workspace_storage_stats(&workspace_dir))
        .await
        .map_err(|e| format!("Storage stats task failed: {e}"))
}

#[tauri::command]
pub(crate) async fn clear_workspace_storage(
    app: AppHandle,
    state: State<'_, AppState>,
    target: String,
) -> Result<WorkspaceStorageStats, String> {
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::StorageClear)
    else {
        return Err(
            "Cannot clear workspace cache while an image operation is running.".to_string(),
        );
    };
    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .map_err(|_| "Settings lock poisoned".to_string())?;
    let target_trimmed = target.trim().to_string();
    let ws_clone = workspace_dir.clone();
    let (freed, stats) =
        tokio::task::spawn_blocking(move || -> Result<(u64, WorkspaceStorageStats), String> {
            let freed = core_clear_workspace_storage(&ws_clone, &target_trimmed)?;
            let stats = compute_workspace_storage_stats(&ws_clone);
            Ok((freed, stats))
        })
        .await
        .map_err(|e| format!("Storage clear task failed: {e}"))??;
    log_and_emit(
        &state,
        &app,
        "info",
        format!("Cleared workspace '{target}' ({freed} bytes freed)."),
    );
    Ok(stats)
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
        Duration::from_secs(30),
        state.process_guard.wait_until_idle(),
    )
    .await;

    if waited.is_err() {
        log_and_emit(
            &state,
            &app,
            "error",
            "Timed out waiting for active operation to stop cleanly.",
        );
        return Err(
            "Timed out waiting for active operation to stop; retry cancellation or workspace recovery."
                .to_string(),
        );
    }

    let workspace_dir = state
        .settings
        .lock()
        .map(|s| PathBuf::from(&s.workspace_dir))
        .unwrap_or_else(|_| default_workspace_dir());
    let (ws, ws_corrupted) = read_workspace_state_status(&workspace_dir);
    if ws_corrupted || !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
        recover_workspace(app, state).await?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn recover_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let Some(_busy) = state
        .process_guard
        .try_enter_busy_for(OperationKind::Recovery)
    else {
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
        Duration::from_secs(10),
        state.process_guard.wait_until_idle(),
    )
    .await;

    if waited.is_ok() {
        let workspace_dir = state
            .settings
            .lock()
            .map(|s| PathBuf::from(&s.workspace_dir))
            .unwrap_or_else(|_| default_workspace_dir());
        let (ws, ws_corrupted) = read_workspace_state_status(&workspace_dir);
        if ws_corrupted || !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
            let _ = recover_workspace(app.clone(), state).await;
        }
    } else {
        log_and_emit(
            &state,
            &app,
            "warn",
            "Active operation did not become idle within force-close timeout; exiting now.",
        );
    }

    app.exit(0);
    Ok(())
}
