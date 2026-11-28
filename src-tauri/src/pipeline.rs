use crate::state::{
    AppState, BuildConfig, ProgressSpec, ProgressTool, cleanup_session_dir_if_clean, emit_progress,
    log_and_emit, log_with_progress,
};
use isocraft_core::apps::{
    add_provisioned_appx, remove_provisioned_appx, validate_appx_request_paths,
};
use isocraft_core::dism::query_mounted_appx_list_tracked;
use isocraft_core::drivers::{
    inject_driver, is_valid_published_inf_name, remove_driver, scan_driver_path,
};
use isocraft_core::env::{find_bundled_or_path, find_dism, find_oscdimg};
use isocraft_core::features::{
    apply_add_capability, apply_disable_feature, apply_enable_feature, apply_remove_capability,
};
use isocraft_core::files::{FileTarget, apply_file_injections, validate_file_injection_item};
use isocraft_core::fs_utils::{
    is_path_overlapping_dir, is_symlink_or_reparse, is_unc_or_device_path,
};
use isocraft_core::image::{SourceKind, extract_iso_and_export_edition};
use isocraft_core::iso::{
    IsoBuildOptions, build_bootable_iso, validate_autounattend, validate_autounattend_with_bytes,
};
use isocraft_core::post_setup::{
    FirstLogonMode, apply_post_setup_to_mount_cancellable, merge_first_logon_into_autounattend,
    validate_post_setup_plan,
};
use isocraft_core::process::OperationKind;
use isocraft_core::reg_parser::preview_reg_file;
use isocraft_core::registry::{
    apply_reg_files_to_mount_cancellable, apply_service_changes_to_mount_cancellable,
};
use isocraft_core::services::validate_services_plan;
use isocraft_core::updates::{apply_updates, inspect_update_path, run_component_cleanup};
use isocraft_core::wim::{
    export_single_edition, mount_wim, package_install_image, package_install_image_to_path,
    read_workspace_state_status, unmount_wim,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

fn normalize_path_for_compare(p: &Path) -> String {
    if let Ok(canon) = std::fs::canonicalize(p) {
        return canon.to_string_lossy().to_ascii_lowercase();
    }
    if let (Some(parent), Some(name)) = (p.parent(), p.file_name())
        && !parent.as_os_str().is_empty()
        && let Ok(canon_parent) = std::fs::canonicalize(parent)
    {
        return canon_parent
            .join(name)
            .to_string_lossy()
            .to_ascii_lowercase();
    }
    p.to_string_lossy()
        .trim()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn paths_overlap_source(source: &Path, output: &Path) -> bool {
    let src_norm = normalize_path_for_compare(source);
    let out_norm = normalize_path_for_compare(output);
    src_norm == out_norm
}

fn make_progress_cb<'a>(
    state: &'a AppState,
    app: &'a AppHandle,
    step: u8,
    phase_key: &'a str,
    start_pct: f32,
    end_pct: f32,
    tool: ProgressTool,
) -> impl FnMut(&str) + 'a {
    let mut pct = start_pct;
    move |msg: &str| {
        log_with_progress(
            state,
            app,
            ProgressSpec {
                step,
                phase_key,
                start_pct,
                end_pct,
                tool,
            },
            &mut pct,
            msg,
        );
    }
}

fn reject_unc_or_session_path(path: &Path, session_dir: &Path, label: &str) -> Result<(), String> {
    if is_unc_or_device_path(path) {
        return Err(format!(
            "Network/UNC or Windows device paths are not allowed for {label}: '{}'",
            path.display()
        ));
    }
    if is_path_overlapping_dir(path, session_dir) {
        return Err(format!(
            "{label} cannot reside inside temporary workspace session_current: '{}'",
            path.display()
        ));
    }
    Ok(())
}

fn validate_sxs_dir(path_raw: &str, session_dir: &Path, label: &str) -> Result<(), String> {
    let trimmed = path_raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let path = Path::new(trimmed);
    reject_unc_or_session_path(path, session_dir, label)?;
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| format!("{label} directory does not exist: '{}'", path.display()))?;
    if is_symlink_or_reparse(&meta) || !meta.is_dir() {
        return Err(format!(
            "{label} must be a regular non-symlink directory: '{}'",
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn preflight_validate_build_config(
    config: &BuildConfig,
    workspace_dir: &Path,
) -> Result<(), String> {
    let (ws_state, ws_corrupted) = read_workspace_state_status(workspace_dir);
    if ws_corrupted || !ws_state.active_mounts.is_empty() || !ws_state.loaded_hives.is_empty() {
        return Err(
            "Workspace has active WIM mounts or loaded registry hives. Run workspace recovery before starting a build."
                .to_string(),
        );
    }

    if config.selected_index == 0 {
        return Err("Selected edition index must be 1 or greater.".to_string());
    }

    let session_dir = workspace_dir.join("session_current");

    let source_trimmed = config.source_path.trim();
    if source_trimmed.is_empty() {
        return Err("Source image path must not be empty.".to_string());
    }
    let source_path = PathBuf::from(source_trimmed);
    reject_unc_or_session_path(&source_path, &session_dir, "Source image path")?;
    let src_meta = std::fs::symlink_metadata(&source_path)
        .map_err(|_| format!("Source image does not exist: '{}'", source_path.display()))?;
    if is_symlink_or_reparse(&src_meta) || !src_meta.is_file() {
        return Err(format!(
            "Source image must be a regular non-symlink file: '{}'",
            source_path.display()
        ));
    }

    let output_trimmed = config.output_path.trim();
    if output_trimmed.is_empty() {
        return Err("Output path must not be empty.".to_string());
    }
    let output_path = PathBuf::from(output_trimmed);
    if is_unc_or_device_path(&output_path) {
        return Err(format!(
            "Network/UNC or Windows device paths are not allowed for Output path: '{}'",
            output_path.display()
        ));
    }
    if let Ok(out_meta) = std::fs::symlink_metadata(&output_path) {
        if is_symlink_or_reparse(&out_meta) {
            return Err(format!(
                "Refusing to overwrite symlink or reparse point at '{}'",
                output_path.display()
            ));
        }
        if out_meta.is_dir() {
            return Err(format!(
                "Output path '{}' points to an existing directory.",
                output_path.display()
            ));
        }
    }

    if is_path_overlapping_dir(&output_path, &session_dir) {
        return Err(
            "Output path cannot be placed inside temporary workspace session_current directory."
                .to_string(),
        );
    }

    if paths_overlap_source(&source_path, &output_path) {
        return Err("Output path must not overwrite the source image file.".to_string());
    }

    if config.target == "iso" && config.source_kind != SourceKind::Iso {
        return Err("ISO output requires an .iso source image with bootloaders.".to_string());
    }

    if config.target == "iso"
        && let Some(xml_path) = config
            .autounattend_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        reject_unc_or_session_path(Path::new(xml_path), &session_dir, "autounattend.xml path")?;
        let res = validate_autounattend(Path::new(xml_path));
        if !res.valid {
            return Err(format!(
                "Invalid autounattend.xml: {}",
                res.error.unwrap_or_else(|| "unknown XML error".to_string())
            ));
        }
    }

    if let Some(ref sxs) = config.features.default_sxs_path {
        validate_sxs_dir(sxs, &session_dir, "Default SxS source")?;
    }
    for feat in &config.features.enable {
        if let Some(ref sxs) = feat.custom_sxs_path {
            validate_sxs_dir(sxs, &session_dir, "Feature custom SxS source")?;
        }
    }
    for cap in &config.features.capabilities_add {
        if let Some(ref sxs) = cap.custom_sxs_path {
            validate_sxs_dir(sxs, &session_dir, "Capability custom SxS source")?;
        }
    }

    for pkg in &config.apps.add_packages {
        reject_unc_or_session_path(
            Path::new(pkg.package_path.trim()),
            &session_dir,
            "Appx package path",
        )?;
        for dep in &pkg.dependency_paths {
            reject_unc_or_session_path(
                Path::new(dep.trim()),
                &session_dir,
                "Appx dependency path",
            )?;
        }
        validate_appx_request_paths(pkg)?;
    }

    for upd in &config.updates.items {
        let p = Path::new(upd.path.trim());
        reject_unc_or_session_path(p, &session_dir, "Update package path")?;
        inspect_update_path(p)?;
    }

    for drv in &config.drivers.items {
        let p = Path::new(drv.path.trim());
        reject_unc_or_session_path(p, &session_dir, "Driver path")?;
        scan_driver_path(p)?;
    }

    for pub_name in &config.drivers.remove_published_names {
        if !is_valid_published_inf_name(pub_name) {
            return Err(format!(
                "Invalid OEM driver published name '{pub_name}'. Expected format like oem12.inf."
            ));
        }
    }

    for reg in config.registry.reg_files.iter().filter(|r| r.enabled) {
        let p = Path::new(reg.path.trim());
        reject_unc_or_session_path(p, &session_dir, "Registry file path")?;
        let preview = preview_reg_file(p)?;
        if !preview.valid {
            return Err(format!(
                "Syntax error(s) in '{}': {}",
                p.display(),
                preview.errors.join("; ")
            ));
        }
    }

    validate_services_plan(&config.services)?;

    for item in &config.files.items {
        if item.target == FileTarget::IsoRoot && config.target != "iso" {
            continue;
        }
        reject_unc_or_session_path(
            Path::new(item.source_path.trim()),
            &session_dir,
            "Custom file source path",
        )?;
        validate_file_injection_item(item)?;
    }

    validate_post_setup_plan(&config.post_setup, Some(&session_dir))?;
    if config.post_setup.has_enabled_first_logon()
        && config.post_setup.first_logon_mode == FirstLogonMode::Autounattend
        && config.target != "iso"
    {
        return Err(
            "Post-Setup FirstLogonCommands (autounattend.xml) requires ISO output target."
                .to_string(),
        );
    }

    Ok(())
}

pub(crate) async fn run_build_pipeline(
    app: AppHandle,
    state: State<'_, AppState>,
    config: BuildConfig,
) -> Result<String, String> {
    let Some(_busy) = state.process_guard.try_enter_busy_for(OperationKind::Build) else {
        return Err("Another build or image operation is already running.".to_string());
    };

    let settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock poisoned".to_string())?
        .clone();
    let workspace_dir = PathBuf::from(&settings.workspace_dir);

    let config_for_check = config.clone();
    let ws_for_check = workspace_dir.clone();
    tokio::task::spawn_blocking(move || {
        preflight_validate_build_config(&config_for_check, &ws_for_check)
    })
    .await
    .map_err(|e| format!("Pre-flight validation task failed: {e}"))??;

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

    if config.target == "iso" && oscdimg.is_none() && xorriso.is_none() {
        return Err(
            "Neither oscdimg.exe nor xorriso.exe is available for ISO creation.".to_string(),
        );
    }

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
        || !config.drivers.remove_published_names.is_empty()
        || !config.services.changes.is_empty()
        || config.registry.reg_files.iter().any(|r| r.enabled)
        || config
            .files
            .items
            .iter()
            .any(|f| f.target == FileTarget::InstallWim)
        || config.post_setup.has_enabled_items();

    let needs_boot_wim_mount = config.target == "iso"
        && config.source_kind == SourceKind::Iso
        && config.drivers.items.iter().any(|d| d.inject_to_boot_wim);

    let build_res: Result<String, String> = async {
        tokio::fs::create_dir_all(&session_dir)
            .await
            .map_err(|e| format!("Failed creating session directory: {e}"))?;

        for stale_dir in [&mount_install, &mount_boot] {
            if stale_dir.exists() {
                let _ = tokio::fs::remove_dir_all(stale_dir).await;
            }
        }

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
        if config.source_kind == SourceKind::Iso {
            let mut export_phase_emitted = false;
            let mut step2_cb = make_progress_cb(
                &state,
                &app,
                2,
                "step_export_edition",
                16.0,
                26.0,
                ProgressTool::Wimlib,
            );
            extract_iso_and_export_edition(
                &src_path,
                &iso_root,
                &wimlib,
                config.selected_index,
                &work_wim,
                Some(&state.process_guard),
                |msg| log_and_emit(&state, &app, "info", msg),
                |msg| {
                    if !export_phase_emitted {
                        export_phase_emitted = true;
                        emit_progress(
                            &app,
                            2,
                            9,
                            "step_export_edition",
                            16.0,
                            format!(
                                "Exporting selected edition (Index {})...",
                                config.selected_index
                            ),
                        );
                    }
                    step2_cb(msg);
                },
            )
            .await?;
            if !export_phase_emitted {
                emit_progress(
                    &app,
                    2,
                    9,
                    "step_export_edition",
                    26.0,
                    format!(
                        "Exported selected edition (Index {}).",
                        config.selected_index
                    ),
                );
            }
        } else {
            // Step 2: Single-edition export via wimlib (trims unselected editions)
            emit_progress(
                &app,
                2,
                9,
                "step_export_edition",
                16.0,
                format!("Exporting selected edition (Index {})...", config.selected_index),
            );

            let step2_cb = make_progress_cb(
                &state,
                &app,
                2,
                "step_export_edition",
                16.0,
                26.0,
                ProgressTool::Wimlib,
            );
            export_single_edition(
                &wimlib,
                &src_path,
                config.selected_index,
                &work_wim,
                Some(&state.process_guard),
                step2_cb,
            )
            .await?;
        }

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
                            ProgressSpec {
                                step: 3,
                                phase_key: "step_mount_rw",
                                start_pct: 26.0,
                                end_pct: install_end,
                                tool: ProgressTool::Dism,
                            },
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
                            ProgressSpec {
                                step: 3,
                                phase_key: "step_mount_rw",
                                start_pct: 32.0,
                                end_pct: 34.0,
                                tool: ProgressTool::Dism,
                            },
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
                                    ProgressSpec {
                                        step: 4,
                                        phase_key: "step_features",
                                        start_pct: s,
                                        end_pct: e,
                                        tool: ProgressTool::Dism,
                                    },
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
                                    ProgressSpec {
                                        step: 4,
                                        phase_key: "step_features",
                                        start_pct: s,
                                        end_pct: e,
                                        tool: ProgressTool::Dism,
                                    },
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
                                    ProgressSpec {
                                        step: 4,
                                        phase_key: "step_features",
                                        start_pct: s,
                                        end_pct: e,
                                        tool: ProgressTool::Dism,
                                    },
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
                                    ProgressSpec {
                                        step: 4,
                                        phase_key: "step_features",
                                        start_pct: s,
                                        end_pct: e,
                                        tool: ProgressTool::Dism,
                                    },
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
                                ProgressSpec {
                                    step: 5,
                                    phase_key: "step_updates",
                                    start_pct: 46.0,
                                    end_pct: 58.0,
                                    tool: ProgressTool::Dism,
                                },
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
                        let live_apps = query_mounted_appx_list_tracked(
                            &mount_install,
                            Some(&state.process_guard),
                        )
                        .await?;
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
                                            ProgressSpec {
                                                step: 6,
                                                phase_key: "step_appx",
                                                start_pct: s,
                                                end_pct: e,
                                                tool: ProgressTool::Dism,
                                            },
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
                                    ProgressSpec {
                                        step: 6,
                                        phase_key: "step_appx",
                                        start_pct: s,
                                        end_pct: e,
                                        tool: ProgressTool::Dism,
                                    },
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
                if !config.drivers.items.is_empty()
                    || (install_mounted && !config.drivers.remove_published_names.is_empty())
                {
                    emit_progress(
                        &app,
                        7,
                        9,
                        "step_drivers",
                        66.0,
                        "Managing device drivers...",
                    );

                    let remove_drv_count = if install_mounted {
                        config.drivers.remove_published_names.len()
                    } else {
                        0
                    };
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
                    let total_drv_ops =
                        (remove_drv_count + install_drv_count + boot_drv_count).max(1) as f32;
                    let mut drv_idx = 0usize;

                    if install_mounted {
                        for pub_name in &config.drivers.remove_published_names {
                            let s = 66.0 + 6.0 * (drv_idx as f32 / total_drv_ops);
                            let e = 66.0 + 6.0 * ((drv_idx + 1) as f32 / total_drv_ops);
                            let mut pct = s;
                            remove_driver(
                                &dism_path,
                                &mount_install,
                                &dism_log,
                                pub_name,
                                Some(&state.process_guard),
                                |msg| {
                                    log_with_progress(
                                        &state,
                                        &app,
                                        ProgressSpec {
                                            step: 7,
                                            phase_key: "step_drivers",
                                            start_pct: s,
                                            end_pct: e,
                                            tool: ProgressTool::Dism,
                                        },
                                        &mut pct,
                                        msg,
                                    )
                                },
                            )
                            .await?;
                            drv_idx += 1;
                        }

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
                                        ProgressSpec {
                                            step: 7,
                                            phase_key: "step_drivers",
                                            start_pct: s,
                                            end_pct: e,
                                            tool: ProgressTool::Dism,
                                        },
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
                                        ProgressSpec {
                                            step: 7,
                                            phase_key: "step_drivers",
                                            start_pct: s,
                                            end_pct: e,
                                            tool: ProgressTool::Dism,
                                        },
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
                                ProgressSpec {
                                    step: 8,
                                    phase_key: "step_cleanup",
                                    start_pct: 72.0,
                                    end_pct: 75.0,
                                    tool: ProgressTool::Dism,
                                },
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

                if install_mounted && !config.services.changes.is_empty() {
                    emit_progress(
                        &app,
                        8,
                        9,
                        "step_services",
                        74.5,
                        "Applying offline Windows Services configuration...",
                    );

                    let mount_clone = mount_install.clone();
                    let ws_clone = workspace_dir.clone();
                    let svc_changes = config.services.changes.clone();
                    let app_clone = app.clone();
                    let rep = tokio::task::spawn_blocking(move || {
                        apply_service_changes_to_mount_cancellable(
                            &mount_clone,
                            &ws_clone,
                            &svc_changes,
                            || app_clone.state::<AppState>().process_guard.is_cancel_requested(),
                            |msg| {
                                let st = app_clone.state::<AppState>();
                                log_and_emit(&st, &app_clone, "info", msg);
                            },
                        )
                    })
                    .await
                    .map_err(|e| format!("Offline services task panicked: {e}"))??;

                    log_and_emit(
                        &state,
                        &app,
                        "info",
                        format!(
                            "Offline Windows Services applied (applied={}, skipped_missing={}, skipped_non_win32={}).",
                            rep.applied, rep.skipped_missing, rep.skipped_non_win32
                        ),
                    );
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
                        apply_reg_files_to_mount_cancellable(
                            &mount_clone,
                            &ws_clone,
                            &enabled_regs,
                            || app_clone.state::<AppState>().process_guard.is_cancel_requested(),
                            |msg| {
                                let st = app_clone.state::<AppState>();
                                log_and_emit(&st, &app_clone, "info", msg);
                            },
                        )
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

                if install_mounted && config.post_setup.has_enabled_items() {
                    emit_progress(
                        &app,
                        8,
                        9,
                        "step_post_setup",
                        77.0,
                        "Configuring Post-Setup scripts and payloads...",
                    );

                    let mount_clone = mount_install.clone();
                    let ws_clone = workspace_dir.clone();
                    let post_setup_clone = config.post_setup.clone();
                    let app_clone = app.clone();
                    tokio::task::spawn_blocking(move || {
                        apply_post_setup_to_mount_cancellable(
                            &mount_clone,
                            &ws_clone,
                            &post_setup_clone,
                            || app_clone.state::<AppState>().process_guard.is_cancel_requested(),
                            |msg| {
                                let st = app_clone.state::<AppState>();
                                log_and_emit(&st, &app_clone, "info", msg);
                            },
                        )
                    })
                    .await
                    .map_err(|e| format!("Post-Setup staging task panicked: {e}"))??;
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
                                    ProgressSpec {
                                        step: 9,
                                        phase_key: "step_unmount_commit",
                                        start_pct: 78.0,
                                        end_pct: boot_end,
                                        tool: ProgressTool::Dism,
                                    },
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
                                    ProgressSpec {
                                        step: 9,
                                        phase_key: "step_unmount_commit",
                                        start_pct,
                                        end_pct: 85.0,
                                        tool: ProgressTool::Dism,
                                    },
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

        // Step 9: Package install.* (WIM / ESD) and optionally build ISO
        emit_progress(
            &app,
            9,
            9,
            "step_finalize",
            85.0,
            "Finalizing output image...",
        );

        let out_path = PathBuf::from(&config.output_path);
        let skip_recompress_wim = !needs_install_rw_mount;
        let final_output_display = if config.target == "iso" {
            let sources_dir = iso_root.join("sources");
            let pkg_cb = make_progress_cb(
                &state,
                &app,
                9,
                "step_finalize",
                85.0,
                94.0,
                ProgressTool::Wimlib,
            );
            package_install_image(
                &wimlib,
                &work_wim,
                &sources_dir,
                &config.install_format,
                skip_recompress_wim,
                Some(&state.process_guard),
                pkg_cb,
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

            let merged_autounattend_bytes = if config.post_setup.has_enabled_first_logon()
                && config.post_setup.first_logon_mode == FirstLogonMode::Autounattend
            {
                let existing_bytes = if let Some(ref xml_p) = autounattend_ref {
                    let (v, b) = validate_autounattend_with_bytes(xml_p);
                    if !v.valid {
                        return Err(format!(
                            "Invalid autounattend.xml: {}",
                            v.error.unwrap_or_else(|| "unknown XML error".to_string())
                        ));
                    }
                    b
                } else {
                    None
                };
                Some(merge_first_logon_into_autounattend(existing_bytes.as_deref())?)
            } else {
                None
            };

            let iso_tool = build_bootable_iso(
                IsoBuildOptions {
                    iso_root: &iso_root,
                    output_iso: &out_path,
                    volume_label: &config.volume_label,
                    uefi_no_prompt: config.uefi_no_prompt,
                    autounattend_path: autounattend_ref.as_deref(),
                    autounattend_xml_override: merged_autounattend_bytes.as_deref(),
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
            out_path.to_string_lossy().to_string()
        } else {
            let pkg_cb = make_progress_cb(
                &state,
                &app,
                9,
                "step_finalize",
                85.0,
                99.0,
                ProgressTool::Wimlib,
            );
            let files = package_install_image_to_path(
                &wimlib,
                &work_wim,
                &out_path,
                &config.install_format,
                skip_recompress_wim,
                Some(&state.process_guard),
                pkg_cb,
            )
            .await?;

            let primary_out = files.first().cloned().unwrap_or_else(|| out_path.clone());
            log_and_emit(
                &state,
                &app,
                "info",
                format!("Image package saved to '{}'", primary_out.display()),
            );
            primary_out.to_string_lossy().to_string()
        };

        emit_progress(&app, 9, 9, "step_done", 100.0, "Build completed.");
        Ok(final_output_display)
    }
    .await;

    cleanup_session_dir_if_clean(&state, &app, &workspace_dir, &session_dir).await;
    drop(_busy);
    if let Err(ref e) = build_res {
        log_and_emit(&state, &app, "error", format!("Build failed: {e}"));
    }
    build_res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppsBuildPlan, FeaturesBuildPlan};
    use isocraft_core::drivers::DriversBuildPlan;
    use isocraft_core::files::FilesBuildPlan;
    use isocraft_core::post_setup::{
        PostSetupBuildPlan, PostSetupItem, PostSetupItemKind, PostSetupPhase, WingetScope,
        WingetSource,
    };
    use isocraft_core::registry::{RegQueueEntry, RegistryBuildPlan};
    use isocraft_core::services::{ServiceStartChange, ServiceStartType, ServicesBuildPlan};
    use isocraft_core::updates::UpdatesBuildPlan;
    use isocraft_core::wim::InstallOutputFormat;

    fn sample_config(src: &Path, out: &Path) -> BuildConfig {
        BuildConfig {
            source_path: src.to_string_lossy().to_string(),
            source_kind: SourceKind::Iso,
            selected_index: 1,
            target: "iso".to_string(),
            install_format: InstallOutputFormat::Wim,
            output_path: out.to_string_lossy().to_string(),
            autounattend_path: None,
            add_generic_ei_cfg: false,
            uefi_no_prompt: true,
            volume_label: "WIN11".to_string(),
            apps: AppsBuildPlan::default(),
            features: FeaturesBuildPlan::default(),
            services: ServicesBuildPlan::default(),
            updates: UpdatesBuildPlan::default(),
            drivers: DriversBuildPlan::default(),
            registry: RegistryBuildPlan::default(),
            files: FilesBuildPlan::default(),
            post_setup: PostSetupBuildPlan::default(),
        }
    }

    #[test]
    fn preflight_rejects_dirty_workspace_and_bad_paths() {
        let dir = std::env::temp_dir().join(format!(
            "isocraft_pipeline_preflight_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("win11.iso");
        std::fs::write(&src, b"iso").unwrap();
        let out = dir.join("custom.iso");

        let mut cfg = sample_config(&src, &out);
        assert!(preflight_validate_build_config(&cfg, &dir).is_ok());

        // Dirty workspace_state.json blocks build
        std::fs::write(
            dir.join("workspace_state.json"),
            r#"{"activeMounts":["C:\\m"],"loadedHives":[],"updatedAt":"now"}"#,
        )
        .unwrap();
        assert!(
            preflight_validate_build_config(&cfg, &dir)
                .unwrap_err()
                .contains("recovery")
        );
        let _ = std::fs::remove_file(dir.join("workspace_state.json"));

        // Index 0 rejected
        cfg.selected_index = 0;
        assert!(preflight_validate_build_config(&cfg, &dir).is_err());
        cfg.selected_index = 1;

        // Output inside session_current rejected
        cfg.output_path = dir
            .join("session_current")
            .join("out.iso")
            .to_string_lossy()
            .to_string();
        assert!(preflight_validate_build_config(&cfg, &dir).is_err());

        // Existing dir as output rejected
        let existing_dir = dir.join("out_folder");
        std::fs::create_dir_all(&existing_dir).unwrap();
        cfg.output_path = existing_dir.to_string_lossy().to_string();
        assert!(preflight_validate_build_config(&cfg, &dir).is_err());
        cfg.output_path = out.to_string_lossy().to_string();

        // Invalid .reg syntax rejected upfront
        let bad_reg = dir.join("bad.reg");
        std::fs::write(&bad_reg, "Not a reg header").unwrap();
        cfg.registry.reg_files.push(RegQueueEntry {
            path: bad_reg.to_string_lossy().to_string(),
            enabled: true,
        });
        assert!(preflight_validate_build_config(&cfg, &dir).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preflight_rejects_unc_bad_sxs_session_inputs_and_corrupt_state() {
        let dir = std::env::temp_dir().join(format!(
            "isocraft_pipeline_security_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("win11.iso");
        std::fs::write(&src, b"iso").unwrap();
        let out = dir.join("custom.iso");

        let mut cfg = sample_config(&src, &out);
        assert!(preflight_validate_build_config(&cfg, &dir).is_ok());

        // Corrupted workspace_state.json blocks build
        std::fs::write(dir.join("workspace_state.json"), b"{not-valid-json").unwrap();
        assert!(
            preflight_validate_build_config(&cfg, &dir)
                .unwrap_err()
                .contains("recovery")
        );
        let _ = std::fs::remove_file(dir.join("workspace_state.json"));

        // UNC source path rejected
        cfg.source_path = r"\\server\share\win11.iso".to_string();
        assert!(
            preflight_validate_build_config(&cfg, &dir)
                .unwrap_err()
                .contains("UNC")
        );
        cfg.source_path = src.to_string_lossy().to_string();

        // Missing default_sxs_path rejected
        cfg.features.default_sxs_path = Some(dir.join("missing_sxs").to_string_lossy().to_string());
        assert!(preflight_validate_build_config(&cfg, &dir).is_err());
        cfg.features.default_sxs_path = None;

        // Input residing inside session_current rejected
        let in_session = dir.join("session_current").join("input.reg");
        std::fs::create_dir_all(in_session.parent().unwrap()).unwrap();
        std::fs::write(&in_session, "Windows Registry Editor Version 5.00\r\n").unwrap();
        cfg.registry.reg_files.push(RegQueueEntry {
            path: in_session.to_string_lossy().to_string(),
            enabled: true,
        });
        assert!(
            preflight_validate_build_config(&cfg, &dir)
                .unwrap_err()
                .contains("session_current")
        );

        // Invalid remove_published_names rejected; valid oemX.inf accepted
        cfg.registry.reg_files.clear();
        cfg.drivers.remove_published_names = vec!["oem12.inf".to_string()];
        assert!(preflight_validate_build_config(&cfg, &dir).is_ok());
        cfg.drivers.remove_published_names = vec!["..\\evil.inf".to_string()];
        assert!(preflight_validate_build_config(&cfg, &dir).is_err());
        cfg.drivers.remove_published_names.clear();

        // Invalid winget ID in Post-Setup rejected; valid accepted
        cfg.post_setup.items.push(PostSetupItem {
            id: "wg1".into(),
            kind: PostSetupItemKind::Winget,
            phase: PostSetupPhase::FirstLogon,
            enabled: true,
            source_path: None,
            args: String::new(),
            winget_id: Some("7zip.7zip".into()),
            display_name: None,
            winget_source: WingetSource::Winget,
            winget_scope: WingetScope::Default,
        });
        assert!(preflight_validate_build_config(&cfg, &dir).is_ok());
        cfg.post_setup.items[0].winget_id = Some("bad;calc".into());
        assert!(preflight_validate_build_config(&cfg, &dir).is_err());
        cfg.post_setup.items.clear();

        // Locked critical service disable rejected in preflight; safe service accepted
        cfg.services.changes = vec![ServiceStartChange {
            name: "DiagTrack".into(),
            start_type: ServiceStartType::Disabled,
        }];
        assert!(preflight_validate_build_config(&cfg, &dir).is_ok());
        cfg.services.changes = vec![ServiceStartChange {
            name: "RpcSs".into(),
            start_type: ServiceStartType::Disabled,
        }];
        assert!(
            preflight_validate_build_config(&cfg, &dir)
                .unwrap_err()
                .contains("cannot be disabled")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
