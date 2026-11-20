use crate::process::{run_tracked_command, ProcessGuard};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsFeatureInfo {
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsCapabilityInfo {
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureEnableItem {
    pub name: String,
    pub enable_all: bool,
    pub custom_sxs_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityAddItem {
    pub name: String,
    pub limit_access: bool,
    pub custom_sxs_path: Option<String>,
}

pub fn is_feature_enabled(state: &str) -> bool {
    matches!(
        state.to_ascii_lowercase().as_str(),
        "enabled" | "enablepending"
    )
}

pub fn is_capability_installed(state: &str) -> bool {
    matches!(
        state.to_ascii_lowercase().as_str(),
        "installed" | "installpending" | "staged"
    )
}

pub fn build_enable_feature_args(
    mount_dir: &Path,
    log_path: &Path,
    feature_name: &str,
    enable_all: bool,
    sxs_source: Option<&Path>,
) -> Vec<String> {
    let mut args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Enable-Feature".to_string(),
        format!("/FeatureName:{feature_name}"),
    ];

    if enable_all {
        args.push("/All".to_string());
    }
    if let Some(sxs) = sxs_source {
        args.push("/LimitAccess".to_string());
        args.push(format!("/Source:{}", sxs.display()));
    }
    args
}

pub fn build_disable_feature_args(
    mount_dir: &Path,
    log_path: &Path,
    feature_name: &str,
) -> Vec<String> {
    vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Disable-Feature".to_string(),
        format!("/FeatureName:{feature_name}"),
    ]
}

pub fn build_add_capability_args(
    mount_dir: &Path,
    log_path: &Path,
    capability_name: &str,
    limit_access: bool,
    sxs_source: Option<&Path>,
) -> Vec<String> {
    let mut args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Add-Capability".to_string(),
        format!("/CapabilityName:{capability_name}"),
    ];

    if limit_access {
        args.push("/LimitAccess".to_string());
    }
    if let Some(sxs) = sxs_source {
        args.push(format!("/Source:{}", sxs.display()));
    }
    args
}

pub fn build_remove_capability_args(
    mount_dir: &Path,
    log_path: &Path,
    capability_name: &str,
) -> Vec<String> {
    vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Remove-Capability".to_string(),
        format!("/CapabilityName:{capability_name}"),
    ]
}

pub async fn apply_enable_feature<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    item: &FeatureEnableItem,
    fallback_sxs: Option<&Path>,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let sxs_owned = item
        .custom_sxs_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from);
    let sxs_ref = sxs_owned.as_deref().or(fallback_sxs);

    let args = build_enable_feature_args(mount_dir, log_path, &item.name, item.enable_all, sxs_ref);
    on_log(&format!("Enabling Optional Feature: {}", item.name));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed enabling feature '{}' (exit {}): {} {}",
            item.name,
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

pub async fn apply_disable_feature<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    feature_name: &str,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let args = build_disable_feature_args(mount_dir, log_path, feature_name);
    on_log(&format!("Disabling Optional Feature: {feature_name}"));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed disabling feature '{feature_name}' (exit {}): {} {}",
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

pub async fn apply_add_capability<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    item: &CapabilityAddItem,
    fallback_sxs: Option<&Path>,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let sxs_owned = item
        .custom_sxs_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from);
    let sxs_ref = sxs_owned.as_deref().or(fallback_sxs);

    let args =
        build_add_capability_args(mount_dir, log_path, &item.name, item.limit_access, sxs_ref);
    on_log(&format!("Adding Capability: {}", item.name));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed adding capability '{}' (exit {}): {} {}",
            item.name,
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

pub async fn apply_remove_capability<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    capability_name: &str,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let args = build_remove_capability_args(mount_dir, log_path, capability_name);
    on_log(&format!("Removing Capability: {capability_name}"));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed removing capability '{capability_name}' (exit {}): {} {}",
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_netfx3_enable_args_with_sxs() {
        let args = build_enable_feature_args(
            Path::new(r"C:\work\mount"),
            Path::new(r"C:\work\dism.log"),
            "NetFx3",
            true,
            Some(Path::new(r"C:\work\iso_root\sources\sxs")),
        );
        assert!(args.contains(&"/Enable-Feature".to_string()));
        assert!(args.contains(&"/FeatureName:NetFx3".to_string()));
        assert!(args.contains(&"/All".to_string()));
        assert!(args.contains(&"/LimitAccess".to_string()));
        assert!(args.contains(&r"/Source:C:\work\iso_root\sources\sxs".to_string()));
    }

    #[test]
    fn builds_capability_args_with_limit_access() {
        let args = build_add_capability_args(
            Path::new(r"C:\work\mount"),
            Path::new(r"C:\work\dism.log"),
            "OpenSSH.Client~~~~0.0.1.0",
            true,
            None,
        );
        assert!(args.contains(&"/Add-Capability".to_string()));
        assert!(args.contains(&"/CapabilityName:OpenSSH.Client~~~~0.0.1.0".to_string()));
        assert!(args.contains(&"/LimitAccess".to_string()));
    }

    #[test]
    fn builds_feature_args_with_spaces_and_cyrillic_sxs() {
        let mount = Path::new(r"D:\Образы и маунты\mount install");
        let log = Path::new(r"D:\Образы и маунты\dism.log");
        let sxs = Path::new(r"D:\Источники ISO\sources\sxs");

        let en = build_enable_feature_args(mount, log, "NetFx3", true, Some(sxs));
        assert!(en.contains(&r"/Image:D:\Образы и маунты\mount install".to_string()));
        assert!(en.contains(&r"/LogPath:D:\Образы и маунты\dism.log".to_string()));
        assert!(en.contains(&r"/Source:D:\Источники ISO\sources\sxs".to_string()));
        assert!(en.contains(&"/All".to_string()));
        assert!(en.contains(&"/LimitAccess".to_string()));

        let dis = build_disable_feature_args(mount, log, "WorkFolders-Client");
        assert!(dis.contains(&"/Disable-Feature".to_string()));
        assert!(dis.contains(&"/FeatureName:WorkFolders-Client".to_string()));
        assert!(dis.contains(&r"/Image:D:\Образы и маунты\mount install".to_string()));
    }
}
