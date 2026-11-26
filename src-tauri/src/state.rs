use isocraft_core::apps::AddAppxRequest;
use isocraft_core::drivers::DriversBuildPlan;
use isocraft_core::features::{CapabilityAddItem, FeatureEnableItem};
use isocraft_core::files::FilesBuildPlan;
use isocraft_core::image::{SourceKind, clear_readonly_recursive};
use isocraft_core::logger::SessionLogger;
use isocraft_core::post_setup::PostSetupBuildPlan;
use isocraft_core::process::ProcessGuard;
use isocraft_core::registry::RegistryBuildPlan;
use isocraft_core::settings::AppSettings;
use isocraft_core::updates::UpdatesBuildPlan;
use isocraft_core::wim::{
    InstallOutputFormat, parse_dism_progress, parse_wimlib_progress, read_workspace_state,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Theme};

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
    #[serde(default)]
    pub post_setup: PostSetupBuildPlan,
}

pub(crate) fn log_and_emit(
    state: &AppState,
    app: &AppHandle,
    level: &str,
    message: impl Into<String>,
) {
    let entry = state.logger.record(level, message);
    let _ = app.emit("app-log", &entry);
}

pub(crate) fn emit_progress(
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
pub(crate) enum ProgressTool {
    Dism,
    Wimlib,
}

#[derive(Clone, Copy)]
pub(crate) struct ProgressSpec<'a> {
    pub step: u8,
    pub phase_key: &'a str,
    pub start_pct: f32,
    pub end_pct: f32,
    pub tool: ProgressTool,
}

pub(crate) fn log_with_progress(
    state: &AppState,
    app: &AppHandle,
    spec: ProgressSpec<'_>,
    last_pct: &mut f32,
    line: &str,
) {
    log_and_emit(state, app, "info", line);
    let parsed = match spec.tool {
        ProgressTool::Dism => parse_dism_progress(line),
        ProgressTool::Wimlib => parse_wimlib_progress(line),
    };
    if let Some(sub_pct) = parsed {
        let mapped = spec.start_pct + (spec.end_pct - spec.start_pct) * (sub_pct / 100.0);
        if mapped >= *last_pct + 0.5 || (sub_pct >= 100.0 && mapped > *last_pct) {
            *last_pct = mapped;
            emit_progress(app, spec.step, 9, spec.phase_key, mapped, line.trim());
        }
    }
}

pub(crate) async fn cleanup_session_dir_if_clean(
    state: &AppState,
    app: &AppHandle,
    workspace_dir: &Path,
    session_dir: &Path,
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
    let dir_clone = session_dir.to_path_buf();
    let res = tokio::task::spawn_blocking(move || {
        let _ = clear_readonly_recursive(&dir_clone);
        std::fs::remove_dir_all(&dir_clone)
    })
    .await;
    match res {
        Ok(Ok(())) => log_and_emit(
            state,
            app,
            "info",
            "Cleaned up temporary session_current workspace.",
        ),
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

pub(crate) fn apply_window_theme(app: &AppHandle, theme: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let tauri_theme = match theme {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        };
        let _ = win.set_theme(tauri_theme);
    }
}
