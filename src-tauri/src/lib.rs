use isocraft_core::apps::{
    add_provisioned_appx, remove_provisioned_appx, scan_appx_dependencies as core_scan_deps,
    AddAppxRequest, DependencyScanResult,
};
use isocraft_core::dism::{
    load_image_catalog as core_load_catalog, query_mounted_appx_list, ImageCatalog,
};
use isocraft_core::drivers::{
    inject_driver, scan_driver_path as core_scan_driver_path, DriverScanResult, DriversBuildPlan,
};
use isocraft_core::env::{
    check_environment_status, find_bundled_or_path, find_dism, find_oscdimg, EnvironmentStatus,
};
use isocraft_core::features::{
    apply_add_capability, apply_disable_feature, apply_enable_feature, apply_remove_capability,
    CapabilityAddItem, FeatureEnableItem,
};
use isocraft_core::files::{
    apply_file_injections, scan_custom_file_path as core_scan_custom_file_path,
    validate_file_injection_item, FileScanResult, FileTarget, FilesBuildPlan,
};
use isocraft_core::image::{
    clear_readonly_recursive, extract_iso_to_workspace, inspect_image as core_inspect_image,
    ImageInspectionResult, SourceKind,
};
use isocraft_core::iso::{
    build_bootable_iso, validate_autounattend as core_validate_autounattend,
    AutounattendValidation, IsoBuildOptions,
};
use isocraft_core::logger::{LogEntry, SessionLogger};
use isocraft_core::preset::{
    export_preset_to_file, import_preset_from_file, PresetImportResult, StudioPreset,
};
use isocraft_core::process::ProcessGuard;
use isocraft_core::reg_parser::{preview_reg_file as core_preview_reg_file, RegFilePreview};
use isocraft_core::registry::{apply_reg_files_to_mount, RegistryBuildPlan};
use isocraft_core::settings::{
    default_workspace_dir, load_settings, save_settings_to_disk, webview_user_data_dir, AppSettings,
};
use isocraft_core::updates::{
    apply_updates, inspect_update_path as core_inspect_update_path, run_component_cleanup,
    UpdateScanInfo, UpdatesBuildPlan,
};
use isocraft_core::wim::{
    export_single_edition, mount_wim, package_install_image, package_install_image_to_path,
    parse_dism_progress, parse_wimlib_progress, read_workspace_state, recover_workspace_async,
    unmount_wim, InstallOutputFormat,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, Manager, State, Theme, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

pub struct AppState {
    pub settings: Mutex<AppSettings>,
    pub logger: SessionLogger,
    pub process_guard: ProcessGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProgressEvent {
    pub step: u8,
    pub total_steps: u8,
    pub phase_key: String,
    pub percent: f32,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppsBuildPlan {
    pub remove_families: Vec<String>,
    pub add_packages: Vec<AddAppxRequest>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FeaturesBuildPlan {
    pub enable: Vec<FeatureEnableItem>,
    pub disable: Vec<String>,
    pub capabilities_add: Vec<CapabilityAddItem>,
    pub capabilities_remove: Vec<String>,
    pub default_sxs_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildConfig {
    pub source_path: String,
    pub source_kind: SourceKind,
    pub selected_index: u32,
    pub target: String, // "iso" | "image"
    pub install_format: InstallOutputFormat,
    pub split_size_mb: u32,
    pub output_path: String,
    pub autounattend_path: Option<String>,
    pub add_generic_ei_cfg: bool,
    pub uefi_no_prompt: bool,
    pub volume_label: String,
    #[serde(default)]
    pub apps: AppsBuildPlan,
    #[serde(default)]
    pub features: FeaturesBuildPlan,
    #[serde(default)]
    pub updates: UpdatesBuildPlan,
    #[serde(default)]
    pub drivers: DriversBuildPlan,
    #[serde(default)]
    pub registry: RegistryBuildPlan,
    #[serde(default)]
    pub files: FilesBuildPlan,
}

fn log_and_emit(state: &AppState, app: &AppHandle, level: &str, message: impl Into<String>) {
    let entry = state.logger.record(level, message);
    let _ = app.emit("app-log", &entry);
}

fn emit_progress(
    app: &AppHandle,
    step: u8,
    total_steps: u8,
    phase_key: &str,
    percent: f32,
    message: impl Into<String>,
) {
    let payload = BuildProgressEvent {
        step,
        total_steps,
        phase_key: phase_key.to_string(),
        percent,
        message: message.into(),
    };
    let _ = app.emit("build-progress", &payload);
}

#[derive(Clone, Copy)]
enum ProgressTool {
    Dism,
    Wimlib,
}

fn log_with_progress(
    state: &AppState,
    app: &AppHandle,
    step: u8,
    phase_key: &str,
    start_pct: f32,
    end_pct: f32,
    tool: ProgressTool,
    last_pct: &mut f32,
    line: &str,
) {
    log_and_emit(state, app, "info", line);
    let parsed = match tool {
        ProgressTool::Dism => parse_dism_progress(line),
        ProgressTool::Wimlib => parse_wimlib_progress(line),
    };
    if let Some(sub_pct) = parsed {
        let mapped = start_pct + (end_pct - start_pct) * (sub_pct / 100.0);
        if mapped >= *last_pct + 0.5 || (sub_pct >= 100.0 && mapped > *last_pct) {
            *last_pct = mapped;
            emit_progress(app, step, 9, phase_key, mapped, line.trim());
        }
    }
}

async fn cleanup_session_dir_if_clean(
    state: &AppState,
    app: &AppHandle,
    workspace_dir: &PathBuf,
    session_dir: &PathBuf,
) {
    let ws = read_workspace_state(workspace_dir);
    if !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
        log_and_emit(
            state,
            app,
            "warn",
            "Skipping session_current cleanup because active WIM mounts or registry hives remain.",
        );
        return;
    }
    if !session_dir.exists() {
        return;
    }
    let dir_clone = session_dir.clone();
    let res = tokio::task::spawn_blocking(move || {
        let _ = clear_readonly_recursive(&dir_clone);
        std::fs::remove_dir_all(&dir_clone)
    })
    .await;
    match res {
        Ok(Ok(())) => log_and_emit(state, app, "info", "Cleaned up temporary session_current workspace."),
        Ok(Err(e)) => log_and_emit(
            state,
            app,
            "warn",
            format!("Could not fully clean session_current: {e}"),
        ),
        Err(e) => log_and_emit(
            state,
            app,
            "warn",
            format!("Session cleanup task failed: {e}"),
        ),
    }
}

#[tauri::command]
fn check_environment(app: AppHandle, state: State<AppState>) -> Result<EnvironmentStatus, String> {
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
fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "Settings lock poisoned".to_string())
}

#[tauri::command]
fn save_settings(
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
fn set_app_theme(app: AppHandle, state: State<AppState>, theme: String) -> Result<(), String> {
    apply_window_theme(&app, &theme);
    if let Ok(mut guard) = state.settings.lock() {
        guard.theme = theme;
        let _ = save_settings_to_disk(&guard);
    }
    Ok(())
}

#[tauri::command]
fn get_logs(state: State<AppState>) -> Vec<LogEntry> {
    state.logger.snapshot()
}

#[tauri::command]
fn export_diagnostics(state: State<AppState>, dest_path: String) -> Result<String, String> {
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
async fn inspect_image(
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
async fn load_image_catalog(
    app: AppHandle,
    state: State<'_, AppState>,
    source_path: String,
    source_kind: SourceKind,
    index: u32,
    force_refresh: Option<bool>,
) -> Result<ImageCatalog, String> {
    if state.process_guard.is_busy() {
        return Err("Another image operation is currently running.".to_string());
    }

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

    let _busy = state.process_guard.enter_busy();
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
fn scan_appx_dependencies(paths: Vec<String>) -> DependencyScanResult {
    core_scan_deps(&paths)
}

#[tauri::command]
fn scan_update_path(path: String) -> Result<UpdateScanInfo, String> {
    core_inspect_update_path(&PathBuf::from(path))
}

#[tauri::command]
fn scan_driver_folder(path: String) -> Result<DriverScanResult, String> {
    core_scan_driver_path(&PathBuf::from(path))
}

#[tauri::command]
fn scan_custom_file_path(path: String) -> Result<FileScanResult, String> {
    core_scan_custom_file_path(&PathBuf::from(path))
}

#[tauri::command]
fn preview_reg_file(path: String) -> Result<RegFilePreview, String> {
    core_preview_reg_file(&PathBuf::from(path))
}

#[tauri::command]
fn validate_autounattend(path: String) -> AutounattendValidation {
    core_validate_autounattend(&PathBuf::from(path))
}

#[tauri::command]
fn export_preset(
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
fn import_preset(
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
fn open_external_url(url: String) -> Result<(), String> {
    const ALLOWED_URLS: [&str; 4] = [
        "https://www.microsoft.com/software-download/windows11",
        "https://uupdump.net/",
        "https://github.com/Lunqasd1337/ISOCraft",
        "https://github.com/Lunqasd1337/ISOCraft/releases",
    ];
    if !ALLOWED_URLS.contains(&url.as_str()) {
        return Err("Unsupported external URL.".to_string());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn start_build(
    app: AppHandle,
    state: State<'_, AppState>,
    config: BuildConfig,
) -> Result<String, String> {
    if state.process_guard.is_busy() {
        return Err("Another build or image operation is already running.".to_string());
    }

    let settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock poisoned".to_string())?
        .clone();

    let res_dir = app.path().resource_dir().ok();
    let dism_path = find_dism()
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "dism.exe not found.".to_string())?;

    let wimlib = find_bundled_or_path(
        res_dir.as_deref(),
        &["wimlib/wimlib-imagex.exe", "wimlib-imagex.exe"],
        "wimlib-imagex.exe",
    )
    .path
    .map(PathBuf::from)
    .ok_or_else(|| "wimlib-imagex.exe is missing.".to_string())?;

    let xorriso = find_bundled_or_path(
        res_dir.as_deref(),
        &["xorriso/xorriso.exe", "xorriso.exe"],
        "xorriso.exe",
    )
    .path
    .map(PathBuf::from);

    let oscdimg = find_oscdimg(settings.oscdimg_path.as_deref())
        .path
        .map(PathBuf::from);

    if config.target == "iso" && config.source_kind != SourceKind::Iso {
        return Err("ISO output requires an .iso source image with bootloaders.".to_string());
    }
    if config.target == "iso" && oscdimg.is_none() && xorriso.is_none() {
        return Err(
            "Neither oscdimg.exe nor xorriso.exe is available for ISO creation.".to_string(),
        );
    }

    for item in &config.files.items {
        if item.target == FileTarget::IsoRoot && config.target != "iso" {
            continue;
        }
        validate_file_injection_item(item)?;
    }

    let _busy = state.process_guard.enter_busy();
    let workspace_dir = PathBuf::from(&settings.workspace_dir);
    let session_dir = workspace_dir.join("session_current");
    let iso_root = session_dir.join("iso_root");
    let work_wim = session_dir.join("work").join("install.wim");
    let mount_install = session_dir.join("mount_install");
    let mount_boot = session_dir.join("mount_boot");
    let dism_log = workspace_dir.join("dism.log");

    let needs_install_rw_mount = !config.apps.remove_families.is_empty()
        || !config.apps.add_packages.is_empty()
        || !config.features.enable.is_empty()
        || !config.features.disable.is_empty()
        || !config.features.capabilities_add.is_empty()
        || !config.features.capabilities_remove.is_empty()
        || !config.updates.items.is_empty()
        || config.updates.run_component_cleanup
        || config.drivers.items.iter().any(|d| d.inject_to_install_wim)
        || config.registry.reg_files.iter().any(|r| r.enabled)
        || config
            .files
            .items
            .iter()
            .any(|f| f.target == FileTarget::InstallWim);

    let needs_boot_wim_mount = config.target == "iso"
        && config.source_kind == SourceKind::Iso
        && config.drivers.items.iter().any(|d| d.inject_to_boot_wim);

    let build_res: Result<String, String> = async {
        std::fs::create_dir_all(&session_dir)
            .map_err(|e| format!("Failed creating session directory: {e}"))?;

        // Step 1: Prepare workspace & extract ISO (if source is ISO)
        emit_progress(
            &app,
            1,
            9,
            "step_prepare",
            5.0,
            "Preparing workspace directory...",
        );

        let src_path = PathBuf::from(&config.source_path);
        let src_install_path = if config.source_kind == SourceKind::Iso {
            extract_iso_to_workspace(
                &src_path,
                &iso_root,
                Some(&state.process_guard),
                |msg| log_and_emit(&state, &app, "info", msg),
            )
            .await?
        } else {
            src_path.clone()
        };

        // Step 2: Single-edition export via wimlib (trims unselected editions)
        emit_progress(
            &app,
            2,
            9,
            "step_export_edition",
            16.0,
            format!("Exporting selected edition (Index {})...", config.selected_index),
        );

        let mut step2_pct = 16.0f32;
        export_single_edition(
            &wimlib,
            &src_install_path,
            config.selected_index,
            &work_wim,
            Some(&state.process_guard),
            |msg| {
                log_with_progress(
                    &state,
                    &app,
                    2,
                    "step_export_edition",
                    16.0,
                    26.0,
                    ProgressTool::Wimlib,
                    &mut step2_pct,
                    msg,
                )
            },
        )
        .await?;

        if needs_install_rw_mount || needs_boot_wim_mount {
            let mut install_mounted = false;
            let mut boot_mounted = false;

            // Step 3: RW Mount install.wim and optionally boot.wim (Index 2 - Windows Setup)
            emit_progress(
                &app,
                3,
                9,
                "step_mount_rw",
                26.0,
                "Mounting WIM image(s) (Read-Write)...",
            );

            if needs_install_rw_mount {
                let mut step3_pct = 26.0f32;
                let install_end = if needs_boot_wim_mount { 32.0 } else { 34.0 };
                mount_wim(
                    &dism_path,
                    &work_wim,
                    1,
                    &mount_install,
                    false,
                    &workspace_dir,
                    Some(&state.process_guard),
                    |msg| {
                        log_with_progress(
                            &state,
                            &app,
                            3,
                            "step_mount_rw",
                            26.0,
                            install_end,
                            ProgressTool::Dism,
                            &mut step3_pct,
                            msg,
                        )
                    },
                )
                .await?;
                install_mounted = true;
            }

            if needs_boot_wim_mount {
                let boot_wim_path = iso_root.join("sources").join("boot.wim");
                if !boot_wim_path.is_file() {
                    if install_mounted {
                        let _ = unmount_wim(
                            &dism_path,
                            &mount_install,
                            false,
                            &workspace_dir,
                            None,
                            |msg| log_and_emit(&state, &app, "warn", msg),
                        )
                        .await;
                    }
                    return Err("sources\\boot.wim not found in ISO; cannot inject Setup drivers.".to_string());
                }

                let mut boot_pct = 32.0f32;
                if let Err(err) = mount_wim(
                    &dism_path,
                    &boot_wim_path,
                    2,
                    &mount_boot,
                    false,
                    &workspace_dir,
                    Some(&state.process_guard),
                    |msg| {
                        log_with_progress(
                            &state,
                            &app,
                            3,
                            "step_mount_rw",
                            32.0,
                            34.0,
                            ProgressTool::Dism,
                            &mut boot_pct,
                            msg,
                        )
                    },
                )
                .await
                {
                    if install_mounted {
                        let _ = unmount_wim(
                            &dism_path,
                            &mount_install,
                            false,
                            &workspace_dir,
                            None,
                            |msg| log_and_emit(&state, &app, "warn", msg),
                        )
                        .await;
                    }
                    return Err(err);
                }
                boot_mounted = true;
            }

            let service_res: Result<(), String> = async {
                // Step 4: Features & Capabilities
                if install_mounted
                    && (!config.features.enable.is_empty()
                        || !config.features.disable.is_empty()
                        || !config.features.capabilities_add.is_empty()
                        || !config.features.capabilities_remove.is_empty())
                {
                    emit_progress(
                        &app,
                        4,
                        9,
                        "step_features",
                        36.0,
                        "Applying Windows Features & Capabilities...",
                    );

                    let auto_iso_sxs = iso_root.join("sources").join("sxs");
                    let user_sxs = config
                        .features
                        .default_sxs_path
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                        .map(PathBuf::from);
                    let fallback_sxs: Option<PathBuf> = user_sxs.or_else(|| {
                        if auto_iso_sxs.is_dir() {
                            Some(auto_iso_sxs)
                        } else {
                            None
                        }
                    });

                    let total_feat_ops = (config.features.enable.len()
                        + config.features.disable.len()
                        + config.features.capabilities_add.len()
                        + config.features.capabilities_remove.len())
                    .max(1) as f32;
                    let mut op_idx = 0usize;

                    for feat in &config.features.enable {
                        let s = 34.0 + 12.0 * (op_idx as f32 / total_feat_ops);
                        let e = 34.0 + 12.0 * ((op_idx + 1) as f32 / total_feat_ops);
                        let mut pct = s;
                        apply_enable_feature(
                            &dism_path,
                            &mount_install,
                            &dism_log,
                            feat,
                            fallback_sxs.as_deref(),
                            Some(&state.process_guard),
                            |msg| {
                                log_with_progress(
                                    &state,
                                    &app,
                                    4,
                                    "step_features",
                                    s,
                                    e,
                                    ProgressTool::Dism,
                                    &mut pct,
                                    msg,
                                )
                            },
                        )
                        .await?;
                        op_idx += 1;
                    }
                    for feat_name in &config.features.disable {
                        let s = 34.0 + 12.0 * (op_idx as f32 / total_feat_ops);
                        let e = 34.0 + 12.0 * ((op_idx + 1) as f32 / total_feat_ops);
                        let mut pct = s;
                        apply_disable_feature(
                            &dism_path,
                            &mount_install,
                            &dism_log,
                            feat_name,
                            Some(&state.process_guard),
                            |msg| {
                                log_with_progress(
                                    &state,
                                    &app,
                                    4,
                                    "step_features",
                                    s,
                                    e,
                                    ProgressTool::Dism,
                                    &mut pct,
                                    msg,
                                )
                            },
                        )
                        .await?;
                        op_idx += 1;
                    }
                    for cap in &config.features.capabilities_add {
                        let s = 34.0 + 12.0 * (op_idx as f32 / total_feat_ops);
                        let e = 34.0 + 12.0 * ((op_idx + 1) as f32 / total_feat_ops);
                        let mut pct = s;
                        apply_add_capability(
                            &dism_path,
                            &mount_install,
                            &dism_log,
                            cap,
                            fallback_sxs.as_deref(),
                            Some(&state.process_guard),
                            |msg| {
                                log_with_progress(
                                    &state,
                                    &app,
                                    4,
                                    "step_features",
                                    s,
                                    e,
                                    ProgressTool::Dism,
                                    &mut pct,
                                    msg,
                                )
                            },
                        )
                        .await?;
                        op_idx += 1;
                    }
                    for cap_name in &config.features.capabilities_remove {
                        let s = 34.0 + 12.0 * (op_idx as f32 / total_feat_ops);
                        let e = 34.0 + 12.0 * ((op_idx + 1) as f32 / total_feat_ops);
                        let mut pct = s;
                        apply_remove_capability(
                            &dism_path,
                            &mount_install,
                            &dism_log,
                            cap_name,
                            Some(&state.process_guard),
                            |msg| {
                                log_with_progress(
                                    &state,
                                    &app,
                                    4,
                                    "step_features",
                                    s,
                                    e,
                                    ProgressTool::Dism,
                                    &mut pct,
                                    msg,
                                )
                            },
                        )
                        .await?;
                        op_idx += 1;
                    }
                }

                // Step 5: Windows Updates (.msu / .cab / 24H2+ Checkpoint CU folder)
                if install_mounted && !config.updates.items.is_empty() {
                    emit_progress(
                        &app,
                        5,
                        9,
                        "step_updates",
                        46.0,
                        "Integrating Windows Updates...",
                    );
                    let mut upd_pct = 46.0f32;
                    apply_updates(
                        &dism_path,
                        &mount_install,
                        &dism_log,
                        &config.updates.items,
                        Some(&state.process_guard),
                        |msg| {
                            log_with_progress(
                                &state,
                                &app,
                                5,
                                "step_updates",
                                46.0,
                                58.0,
                                ProgressTool::Dism,
                                &mut upd_pct,
                                msg,
                            )
                        },
                    )
                    .await?;
                }

                // Step 6: Provisioned Appx / MSIX
                if install_mounted
                    && (!config.apps.remove_families.is_empty()
                        || !config.apps.add_packages.is_empty())
                {
                    emit_progress(
                        &app,
                        6,
                        9,
                        "step_appx",
                        58.0,
                        "Applying Provisioned Appx / MSIX changes...",
                    );

                    let total_appx_ops = (config.apps.remove_families.len()
                        + config.apps.add_packages.len())
                    .max(1) as f32;
                    let mut appx_idx = 0usize;

                    if !config.apps.remove_families.is_empty() {
                        let live_apps = query_mounted_appx_list(&mount_install).await?;
                        let mut by_family: HashMap<String, String> = HashMap::new();
                        for a in live_apps {
                            by_family.insert(
                                a.package_family_name.to_ascii_lowercase(),
                                a.package_name.clone(),
                            );
                        }

                        for fam in &config.apps.remove_families {
                            let s = 58.0 + 8.0 * (appx_idx as f32 / total_appx_ops);
                            let e = 58.0 + 8.0 * ((appx_idx + 1) as f32 / total_appx_ops);
                            let mut pct = s;
                            let key = fam.to_ascii_lowercase();
                            if let Some(exact_pkg) = by_family.get(&key) {
                                remove_provisioned_appx(
                                    &dism_path,
                                    &mount_install,
                                    &dism_log,
                                    exact_pkg,
                                    Some(&state.process_guard),
                                    |msg| {
                                        log_with_progress(
                                            &state,
                                            &app,
                                            6,
                                            "step_appx",
                                            s,
                                            e,
                                            ProgressTool::Dism,
                                            &mut pct,
                                            msg,
                                        )
                                    },
                                )
                                .await?;
                            } else {
                                log_and_emit(
                                    &state,
                                    &app,
                                    "warn",
                                    format!(
                                        "Provisioned Appx '{fam}' not present in mounted edition; skipping removal."
                                    ),
                                );
                            }
                            appx_idx += 1;
                        }
                    }

                    for pkg in &config.apps.add_packages {
                        let s = 58.0 + 8.0 * (appx_idx as f32 / total_appx_ops);
                        let e = 58.0 + 8.0 * ((appx_idx + 1) as f32 / total_appx_ops);
                        let mut pct = s;
                        add_provisioned_appx(
                            &dism_path,
                            &mount_install,
                            &dism_log,
                            pkg,
                            Some(&state.process_guard),
                            |msg| {
                                log_with_progress(
                                    &state,
                                    &app,
                                    6,
                                    "step_appx",
                                    s,
                                    e,
                                    ProgressTool::Dism,
                                    &mut pct,
                                    msg,
                                )
                            },
                        )
                        .await?;
                        appx_idx += 1;
                    }
                }

                // Step 7: Drivers (.inf) into install.wim and optionally boot.wim (Index 2)
                if !config.drivers.items.is_empty() {
                    emit_progress(
                        &app,
                        7,
                        9,
                        "step_drivers",
                        66.0,
                        "Injecting device drivers...",
                    );

                    let install_drv_count = if install_mounted {
                        config
                            .drivers
                            .items
                            .iter()
                            .filter(|d| d.inject_to_install_wim)
                            .count()
                    } else {
                        0
                    };
                    let boot_drv_count = if boot_mounted {
                        config
                            .drivers
                            .items
                            .iter()
                            .filter(|d| d.inject_to_boot_wim)
                            .count()
                    } else {
                        0
                    };
                    let total_drv_ops = (install_drv_count + boot_drv_count).max(1) as f32;
                    let mut drv_idx = 0usize;

                    if install_mounted {
                        for drv in config
                            .drivers
                            .items
                            .iter()
                            .filter(|d| d.inject_to_install_wim)
                        {
                            let s = 66.0 + 6.0 * (drv_idx as f32 / total_drv_ops);
                            let e = 66.0 + 6.0 * ((drv_idx + 1) as f32 / total_drv_ops);
                            let mut pct = s;
                            inject_driver(
                                &dism_path,
                                &mount_install,
                                &dism_log,
                                drv,
                                "install.wim",
                                Some(&state.process_guard),
                                |msg| {
                                    log_with_progress(
                                        &state,
                                        &app,
                                        7,
                                        "step_drivers",
                                        s,
                                        e,
                                        ProgressTool::Dism,
                                        &mut pct,
                                        msg,
                                    )
                                },
                            )
                            .await?;
                            drv_idx += 1;
                        }
                    }

                    if boot_mounted {
                        for drv in config
                            .drivers
                            .items
                            .iter()
                            .filter(|d| d.inject_to_boot_wim)
                        {
                            let s = 66.0 + 6.0 * (drv_idx as f32 / total_drv_ops);
                            let e = 66.0 + 6.0 * ((drv_idx + 1) as f32 / total_drv_ops);
                            let mut pct = s;
                            inject_driver(
                                &dism_path,
                                &mount_boot,
                                &dism_log,
                                drv,
                                "boot.wim (Index 2)",
                                Some(&state.process_guard),
                                |msg| {
                                    log_with_progress(
                                        &state,
                                        &app,
                                        7,
                                        "step_drivers",
                                        s,
                                        e,
                                        ProgressTool::Dism,
                                        &mut pct,
                                        msg,
                                    )
                                },
                            )
                            .await?;
                            drv_idx += 1;
                        }
                    }
                }

                // Step 8: Component Cleanup (StartComponentCleanup) & Offline Registry
                if install_mounted && config.updates.run_component_cleanup {
                    emit_progress(
                        &app,
                        8,
                        9,
                        "step_cleanup",
                        72.0,
                        "Running component store cleanup...",
                    );
                    let mut clean_pct = 72.0f32;
                    let cleaned = run_component_cleanup(
                        &dism_path,
                        &mount_install,
                        &dism_log,
                        config.updates.cleanup_reset_base,
                        Some(&state.process_guard),
                        |msg| {
                            log_with_progress(
                                &state,
                                &app,
                                8,
                                "step_cleanup",
                                72.0,
                                75.0,
                                ProgressTool::Dism,
                                &mut clean_pct,
                                msg,
                            )
                        },
                    )
                    .await?;
                    if !cleaned {
                        log_and_emit(
                            &state,
                            &app,
                            "warn",
                            "DISM /StartComponentCleanup skipped (0x800f0806): offline image has pending servicing operations (e.g. from features or updates) that will complete on first boot.",
                        );
                    }
                }

                let enabled_regs: Vec<PathBuf> = config
                    .registry
                    .reg_files
                    .iter()
                    .filter(|r| r.enabled)
                    .map(|r| PathBuf::from(&r.path))
                    .collect();

                if install_mounted && !enabled_regs.is_empty() {
                    emit_progress(
                        &app,
                        8,
                        9,
                        "step_registry",
                        75.0,
                        "Applying offline .reg files...",
                    );

                    let mount_clone = mount_install.clone();
                    let ws_clone = workspace_dir.clone();
                    let app_clone = app.clone();
                    tokio::task::spawn_blocking(move || {
                        apply_reg_files_to_mount(&mount_clone, &ws_clone, &enabled_regs, |msg| {
                            let st = app_clone.state::<AppState>();
                            log_and_emit(&st, &app_clone, "info", msg);
                        })
                    })
                    .await
                    .map_err(|e| format!("Offline registry task panicked: {e}"))??;
                }

                let has_install_files = config
                    .files
                    .items
                    .iter()
                    .any(|f| f.target == FileTarget::InstallWim);

                if install_mounted && has_install_files {
                    emit_progress(
                        &app,
                        8,
                        9,
                        "step_files",
                        76.0,
                        "Copying custom files into install.wim...",
                    );

                    let mount_clone = mount_install.clone();
                    let files_clone = config.files.items.clone();
                    let app_clone = app.clone();
                    let rep = tokio::task::spawn_blocking(move || {
                        apply_file_injections(
                            &mount_clone,
                            FileTarget::InstallWim,
                            &files_clone,
                            || app_clone.state::<AppState>().process_guard.is_cancel_requested(),
                            |msg| {
                                let st = app_clone.state::<AppState>();
                                log_and_emit(&st, &app_clone, "info", msg);
                            },
                        )
                    })
                    .await
                    .map_err(|e| format!("Custom file copy task panicked: {e}"))??;

                    log_and_emit(
                        &state,
                        &app,
                        "info",
                        format!(
                            "Custom files applied to install.wim (copied={}, skipped={}, dirs={}).",
                            rep.copied_files, rep.skipped_files, rep.created_dirs
                        ),
                    );
                }

                Ok(())
            }
            .await;

            match service_res {
                Ok(()) => {
                    emit_progress(
                        &app,
                        9,
                        9,
                        "step_unmount_commit",
                        78.0,
                        "Committing and unmounting WIM image(s)...",
                    );

                    if boot_mounted {
                        let mut um_boot_pct = 78.0f32;
                        let boot_end = if install_mounted { 80.0 } else { 85.0 };
                        if let Err(err) = unmount_wim(
                            &dism_path,
                            &mount_boot,
                            true,
                            &workspace_dir,
                            Some(&state.process_guard),
                            |msg| {
                                log_with_progress(
                                    &state,
                                    &app,
                                    9,
                                    "step_unmount_commit",
                                    78.0,
                                    boot_end,
                                    ProgressTool::Dism,
                                    &mut um_boot_pct,
                                    msg,
                                )
                            },
                        )
                        .await
                        {
                            if install_mounted {
                                let _ = unmount_wim(
                                    &dism_path,
                                    &mount_install,
                                    false,
                                    &workspace_dir,
                                    None,
                                    |msg| log_and_emit(&state, &app, "warn", msg),
                                )
                                .await;
                            }
                            return Err(err);
                        }
                    }

                    if install_mounted {
                        let start_pct = if boot_mounted { 80.0 } else { 78.0 };
                        let mut um_inst_pct = start_pct;
                        unmount_wim(
                            &dism_path,
                            &mount_install,
                            true,
                            &workspace_dir,
                            Some(&state.process_guard),
                            |msg| {
                                log_with_progress(
                                    &state,
                                    &app,
                                    9,
                                    "step_unmount_commit",
                                    start_pct,
                                    85.0,
                                    ProgressTool::Dism,
                                    &mut um_inst_pct,
                                    msg,
                                )
                            },
                        )
                        .await?;
                    }
                }
                Err(err) => {
                    if boot_mounted {
                        let _ = unmount_wim(
                            &dism_path,
                            &mount_boot,
                            false,
                            &workspace_dir,
                            None,
                            |msg| log_and_emit(&state, &app, "warn", msg),
                        )
                        .await;
                    }
                    if install_mounted {
                        let _ = unmount_wim(
                            &dism_path,
                            &mount_install,
                            false,
                            &workspace_dir,
                            None,
                            |msg| log_and_emit(&state, &app, "warn", msg),
                        )
                        .await;
                    }
                    return Err(err);
                }
            }
        }

        // Step 9: Package install.* (WIM / ESD / SWM) and optionally build ISO
        emit_progress(
            &app,
            9,
            9,
            "step_finalize",
            85.0,
            "Finalizing output image...",
        );

        let out_path = PathBuf::from(&config.output_path);
        if config.target == "iso" {
            let sources_dir = iso_root.join("sources");
            let mut pkg_pct = 85.0f32;
            package_install_image(
                &wimlib,
                &work_wim,
                &sources_dir,
                &config.install_format,
                config.split_size_mb,
                Some(&state.process_guard),
                |msg| {
                    log_with_progress(
                        &state,
                        &app,
                        9,
                        "step_finalize",
                        85.0,
                        94.0,
                        ProgressTool::Wimlib,
                        &mut pkg_pct,
                        msg,
                    )
                },
            )
            .await?;

            let has_iso_files = config
                .files
                .items
                .iter()
                .any(|f| f.target == FileTarget::IsoRoot);

            if has_iso_files {
                emit_progress(
                    &app,
                    9,
                    9,
                    "step_files_iso",
                    94.0,
                    "Copying custom files into ISO root...",
                );

                let iso_root_clone = iso_root.clone();
                let files_clone = config.files.items.clone();
                let app_clone = app.clone();
                let rep = tokio::task::spawn_blocking(move || {
                    apply_file_injections(
                        &iso_root_clone,
                        FileTarget::IsoRoot,
                        &files_clone,
                        || app_clone.state::<AppState>().process_guard.is_cancel_requested(),
                        |msg| {
                            let st = app_clone.state::<AppState>();
                            log_and_emit(&st, &app_clone, "info", msg);
                        },
                    )
                })
                .await
                .map_err(|e| format!("Custom ISO file copy task panicked: {e}"))??;

                log_and_emit(
                    &state,
                    &app,
                    "info",
                    format!(
                        "Custom files applied to ISO root (copied={}, skipped={}, dirs={}).",
                        rep.copied_files, rep.skipped_files, rep.created_dirs
                    ),
                );
            }

            let autounattend_ref = config
                .autounattend_path
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from);

            let iso_tool = build_bootable_iso(
                IsoBuildOptions {
                    iso_root: &iso_root,
                    output_iso: &out_path,
                    volume_label: &config.volume_label,
                    uefi_no_prompt: config.uefi_no_prompt,
                    autounattend_path: autounattend_ref.as_deref(),
                    add_generic_ei_cfg: config.add_generic_ei_cfg,
                    oscdimg_path: oscdimg.as_deref(),
                    xorriso_path: xorriso.as_deref(),
                },
                Some(&state.process_guard),
                |msg| log_and_emit(&state, &app, "info", msg),
            )
            .await?;

            log_and_emit(
                &state,
                &app,
                "info",
                format!("ISO created via {iso_tool}: {}", out_path.display()),
            );
        } else {
            let mut pkg_pct = 85.0f32;
            let files = package_install_image_to_path(
                &wimlib,
                &work_wim,
                &out_path,
                &config.install_format,
                config.split_size_mb,
                Some(&state.process_guard),
                |msg| {
                    log_with_progress(
                        &state,
                        &app,
                        9,
                        "step_finalize",
                        85.0,
                        99.0,
                        ProgressTool::Wimlib,
                        &mut pkg_pct,
                        msg,
                    )
                },
            )
            .await?;

            let primary_out = files.first().cloned().unwrap_or_else(|| out_path.clone());
            log_and_emit(
                &state,
                &app,
                "info",
                if config.install_format == InstallOutputFormat::Swm {
                    format!(
                        "Split image saved to '{}' ({} part(s))",
                        primary_out.display(),
                        files.len()
                    )
                } else {
                    format!("Image package saved to '{}'", primary_out.display())
                },
            );
        }

        emit_progress(&app, 9, 9, "step_done", 100.0, "Build completed.");
        Ok(out_path.to_string_lossy().to_string())
    }
    .await;

    cleanup_session_dir_if_clean(&state, &app, &workspace_dir, &session_dir).await;
    drop(_busy);
    if let Err(ref e) = build_res {
        log_and_emit(&state, &app, "error", format!("Build failed: {e}"));
    }
    build_res
}

#[tauri::command]
async fn cancel_build(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
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
async fn recover_workspace(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    if state.process_guard.is_busy() {
        return Err("Cannot recover workspace while an image operation is running.".to_string());
    }

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
async fn force_cancel_and_close(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
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

fn apply_window_theme(app: &AppHandle, theme: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let tauri_theme = match theme {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        };
        let _ = win.set_theme(tauri_theme);
    }
}

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
            check_environment,
            get_settings,
            save_settings,
            set_app_theme,
            get_logs,
            export_diagnostics,
            inspect_image,
            load_image_catalog,
            scan_appx_dependencies,
            scan_update_path,
            scan_driver_folder,
            scan_custom_file_path,
            preview_reg_file,
            validate_autounattend,
            export_preset,
            import_preset,
            open_external_url,
            start_build,
            cancel_build,
            recover_workspace,
            force_cancel_and_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
