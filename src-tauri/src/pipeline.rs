use crate::state::{
    cleanup_session_dir_if_clean, emit_progress, log_and_emit, log_with_progress, AppState,
    BuildConfig, ProgressTool,
};
use isocraft_core::apps::{add_provisioned_appx, remove_provisioned_appx};
use isocraft_core::dism::query_mounted_appx_list_tracked;
use isocraft_core::drivers::inject_driver;
use isocraft_core::env::{find_bundled_or_path, find_dism, find_oscdimg};
use isocraft_core::features::{
    apply_add_capability, apply_disable_feature, apply_enable_feature, apply_remove_capability,
};
use isocraft_core::files::{apply_file_injections, validate_file_injection_item, FileTarget};
use isocraft_core::image::{extract_iso_to_workspace, SourceKind};
use isocraft_core::iso::{build_bootable_iso, IsoBuildOptions};
use isocraft_core::registry::apply_reg_files_to_mount_cancellable;
use isocraft_core::updates::{apply_updates, run_component_cleanup};
use isocraft_core::wim::{
    export_single_edition, is_swm_part_for_stem, mount_wim, package_install_image,
    package_install_image_to_path, unmount_wim, InstallOutputFormat,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

fn normalize_path_for_compare(p: &Path) -> String {
    if let Ok(canon) = std::fs::canonicalize(p) {
        return canon.to_string_lossy().to_ascii_lowercase();
    }
    if let (Some(parent), Some(name)) = (p.parent(), p.file_name()) {
        if !parent.as_os_str().is_empty() {
            if let Ok(canon_parent) = std::fs::canonicalize(parent) {
                return canon_parent
                    .join(name)
                    .to_string_lossy()
                    .to_ascii_lowercase();
            }
        }
    }
    p.to_string_lossy()
        .trim()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn paths_overlap_source(source: &Path, output: &Path, is_swm_standalone: bool) -> bool {
    let src_norm = normalize_path_for_compare(source);
    let out_norm = normalize_path_for_compare(output);
    if src_norm == out_norm {
        return true;
    }

    if is_swm_standalone {
        let src_parent = source
            .parent()
            .map(normalize_path_for_compare)
            .unwrap_or_default();
        let out_parent = output
            .parent()
            .map(normalize_path_for_compare)
            .unwrap_or_default();
        if !src_parent.is_empty() && src_parent == out_parent {
            let src_name = source
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let out_stem = output
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("install");
            if is_swm_part_for_stem(src_name, out_stem) {
                return true;
            }
        }
    }

    false
}

pub(crate) async fn run_build_pipeline(
    app: AppHandle,
    state: State<'_, AppState>,
    config: BuildConfig,
) -> Result<String, String> {
    let Some(_busy) = state.process_guard.try_enter_busy() else {
        return Err("Another build or image operation is already running.".to_string());
    };

    let source_path_check = PathBuf::from(&config.source_path);
    let output_path_check = PathBuf::from(&config.output_path);
    if config.output_path.trim().is_empty() {
        return Err("Output path must not be empty.".to_string());
    }
    let is_swm_standalone =
        config.target != "iso" && config.install_format == InstallOutputFormat::Swm;
    if paths_overlap_source(&source_path_check, &output_path_check, is_swm_standalone) {
        return Err("Output path must not overwrite the source image file.".to_string());
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
