use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::fs_utils::{
    is_path_overlapping_dir, is_symlink_or_reparse, is_unc_or_device_path,
    normalize_path_for_prefix, write_file_atomic,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub workspace_dir: String,
    pub oscdimg_path: Option<String>,
    pub theme: String,
    pub language: String,
    #[serde(default)]
    pub window_width: Option<u32>,
    #[serde(default)]
    pub window_height: Option<u32>,
    #[serde(default)]
    pub window_maximized: Option<bool>,
}

pub fn app_data_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("ISOCraft")
}

pub fn default_workspace_dir() -> PathBuf {
    app_data_root().join("work")
}

pub fn webview_user_data_dir() -> PathBuf {
    app_data_root().join("WebView2")
}

pub fn settings_file_path() -> PathBuf {
    app_data_root().join("settings.json")
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            workspace_dir: default_workspace_dir().to_string_lossy().to_string(),
            oscdimg_path: None,
            theme: "system".to_string(),
            language: "auto".to_string(),
            window_width: None,
            window_height: None,
            window_maximized: None,
        }
    }
}

pub fn load_settings() -> AppSettings {
    let path = settings_file_path();
    if let Ok(raw) = fs::read_to_string(&path)
        && let Ok(mut parsed) = serde_json::from_str::<AppSettings>(&raw)
    {
        if parsed.workspace_dir.trim().is_empty()
            || validate_workspace_dir(&PathBuf::from(&parsed.workspace_dir)).is_err()
        {
            parsed.workspace_dir = default_workspace_dir().to_string_lossy().to_string();
        }
        return parsed;
    }
    AppSettings::default()
}

/// Validates that `workspace_dir` is a safe, absolute directory suitable for destructive workspace operations.
pub fn validate_workspace_dir(workspace: &Path) -> Result<(), String> {
    let raw = workspace.to_string_lossy();
    if raw.trim().is_empty() {
        return Err("Invalid workspace directory: path cannot be empty.".to_string());
    }
    if is_unc_or_device_path(workspace) {
        return Err(format!(
            "Invalid workspace directory '{}': UNC network and device paths are not allowed.",
            workspace.display()
        ));
    }
    if !workspace.is_absolute() {
        return Err(format!(
            "Invalid workspace directory '{}': path must be absolute.",
            workspace.display()
        ));
    }

    if let Ok(meta) = fs::symlink_metadata(workspace) {
        if is_symlink_or_reparse(&meta) {
            return Err(format!(
                "Invalid workspace directory '{}': symbolic links and reparse points are not allowed.",
                workspace.display()
            ));
        }
        if !meta.is_dir() {
            return Err(format!(
                "Invalid workspace directory '{}': path exists and is not a directory.",
                workspace.display()
            ));
        }
    }

    let normalized = normalize_path_for_prefix(workspace);
    if normalized.parent().is_none() {
        return Err(format!(
            "Invalid workspace directory '{}': drive or share root cannot be used as a workspace.",
            workspace.display()
        ));
    }

    let norm_lower = normalized.to_string_lossy().to_ascii_lowercase();
    for env_key in [
        "USERPROFILE",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if let Some(val) = std::env::var_os(env_key) {
            let env_norm = normalize_path_for_prefix(Path::new(&val))
                .to_string_lossy()
                .to_ascii_lowercase();
            if !env_norm.is_empty() && norm_lower == env_norm {
                return Err(format!(
                    "Invalid workspace directory '{}': system or user profile root cannot be used as a workspace.",
                    workspace.display()
                ));
            }
        }
    }

    for sys_env in ["SystemRoot", "WINDIR"] {
        if let Some(val) = std::env::var_os(sys_env) {
            let sys_path = PathBuf::from(val);
            if is_path_overlapping_dir(workspace, &sys_path) {
                return Err(format!(
                    "Invalid workspace directory '{}': Windows directory cannot be used as a workspace.",
                    workspace.display()
                ));
            }
        }
    }

    Ok(())
}

pub fn save_settings_to_disk(settings: &AppSettings) -> Result<(), String> {
    let workspace = PathBuf::from(&settings.workspace_dir);
    validate_workspace_dir(&workspace)?;

    let root = app_data_root();
    if let Ok(root_meta) = fs::symlink_metadata(&root)
        && is_symlink_or_reparse(&root_meta)
    {
        return Err(format!(
            "Failed to create settings directory: '{}' must not be a symlink or reparse point.",
            root.display()
        ));
    }

    let path = settings_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create settings directory: {e}"))?;
    }
    fs::create_dir_all(&workspace)
        .map_err(|e| format!("Failed to initialize workspace directory: {e}"))?;

    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    write_file_atomic(&path, json.as_bytes())
        .map_err(|e| format!("Failed to write settings file: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_point_to_local_appdata_work() {
        let s = AppSettings::default();
        assert!(s.workspace_dir.ends_with(r"ISOCraft\work"));
        assert_eq!(s.theme, "system");
        assert_eq!(s.language, "auto");
        assert!(s.oscdimg_path.is_none());
        assert!(s.window_width.is_none());
        assert!(s.window_height.is_none());
        assert!(s.window_maximized.is_none());
    }

    #[test]
    fn deserializes_legacy_settings_without_window_geometry() {
        let legacy_json = r#"{
            "workspaceDir": "D:\\ISOCraftWork",
            "oscdimgPath": null,
            "theme": "dark",
            "language": "en"
        }"#;
        let parsed: AppSettings = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(parsed.workspace_dir, r"D:\ISOCraftWork");
        assert_eq!(parsed.theme, "dark");
        assert_eq!(parsed.language, "en");
        assert!(parsed.window_width.is_none());
        assert!(parsed.window_height.is_none());
        assert!(parsed.window_maximized.is_none());
    }

    #[test]
    fn validate_workspace_dir_rejects_dangerous_or_relative_paths() {
        assert!(validate_workspace_dir(Path::new("")).is_err());
        assert!(validate_workspace_dir(Path::new("relative\\work")).is_err());
        assert!(validate_workspace_dir(Path::new(r"C:\")).is_err());
        assert!(validate_workspace_dir(Path::new(r"\\server\share\work")).is_err());
        assert!(validate_workspace_dir(Path::new(r"\\.\PhysicalDrive0")).is_err());
        if let Some(sys) = std::env::var_os("SystemRoot") {
            let sys_path = PathBuf::from(sys);
            assert!(validate_workspace_dir(&sys_path).is_err());
            assert!(validate_workspace_dir(&sys_path.join("System32")).is_err());
        }
        assert!(validate_workspace_dir(&default_workspace_dir()).is_ok());
    }
}
