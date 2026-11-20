use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub workspace_dir: String,
    pub oscdimg_path: Option<String>,
    pub theme: String,
    pub language: String,
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
            language: "ru".to_string(),
        }
    }
}

pub fn load_settings() -> AppSettings {
    let path = settings_file_path();
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(mut parsed) = serde_json::from_str::<AppSettings>(&raw) {
            if parsed.workspace_dir.trim().is_empty() {
                parsed.workspace_dir = default_workspace_dir().to_string_lossy().to_string();
            }
            return parsed;
        }
    }
    AppSettings::default()
}

pub fn save_settings_to_disk(settings: &AppSettings) -> Result<(), String> {
    let path = settings_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create settings directory: {e}"))?;
    }
    let workspace = PathBuf::from(&settings.workspace_dir);
    fs::create_dir_all(&workspace)
        .map_err(|e| format!("Failed to initialize workspace directory: {e}"))?;

    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write settings file: {e}"))?;
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
        assert_eq!(s.language, "ru");
        assert!(s.oscdimg_path.is_none());
    }
}
