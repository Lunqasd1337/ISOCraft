use crate::apps::{extract_package_family_name, protected_appx_reason, ProvisionedAppxInfo};
use crate::features::{WindowsCapabilityInfo, WindowsFeatureInfo};
use crate::image::{
    dismount_iso, escape_ps_single_quoted, mount_iso_drive_letter,
    run_powershell_script_tracked, SourceKind,
};
use crate::process::ProcessGuard;
use crate::wim::{export_single_edition, mount_wim, unmount_wim};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCatalog {
    pub source_path: String,
    pub edition_index: u32,
    pub from_cache: bool,
    pub appx_packages: Vec<ProvisionedAppxInfo>,
    pub features: Vec<WindowsFeatureInfo>,
    pub capabilities: Vec<WindowsCapabilityInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPsAppx {
    display_name: Option<String>,
    package_name: String,
    version: Option<String>,
    publisher_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPsFeature {
    feature_name: String,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPsCapability {
    name: String,
    state: String,
}

fn fnv1a_64(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn catalog_cache_path(workspace_dir: &Path, source_path: &Path, index: u32) -> PathBuf {
    let meta = fs::metadata(source_path).ok();
    let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = format!("{}|{}|{}|{}", source_path.display(), len, mtime, index);
    workspace_dir
        .join("cache")
        .join(format!("catalog_{}.json", fnv1a_64(&key)))
}

pub fn parse_ps_appx_json(json_str: &str) -> Result<Vec<ProvisionedAppxInfo>, String> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }
    let raw: Vec<RawPsAppx> =
        serde_json::from_str(trimmed).map_err(|e| format!("Failed parsing DISM Appx JSON: {e}"))?;

    let mut out: Vec<ProvisionedAppxInfo> = raw
        .into_iter()
        .map(|item| {
            let family = extract_package_family_name(&item.package_name);
            let reason = protected_appx_reason(&item.package_name)
                .or_else(|| protected_appx_reason(&family));
            let display = item
                .display_name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    family
                        .split('_')
                        .next()
                        .unwrap_or(&item.package_name)
                        .to_string()
                });
            ProvisionedAppxInfo {
                display_name: display,
                package_name: item.package_name,
                package_family_name: family,
                version: item.version.unwrap_or_default(),
                publisher_id: item.publisher_id.unwrap_or_default(),
                is_protected: reason.is_some(),
                protection_reason: reason.map(|r| r.to_string()),
            }
        })
        .collect();

    out.sort_by(|a, b| {
        a.display_name
            .to_ascii_lowercase()
            .cmp(&b.display_name.to_ascii_lowercase())
    });
    Ok(out)
}

pub fn parse_ps_features_json(json_str: &str) -> Result<Vec<WindowsFeatureInfo>, String> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }
    let raw: Vec<RawPsFeature> = serde_json::from_str(trimmed)
        .map_err(|e| format!("Failed parsing DISM Features JSON: {e}"))?;

    let mut out: Vec<WindowsFeatureInfo> = raw
        .into_iter()
        .map(|f| WindowsFeatureInfo {
            name: f.feature_name,
            state: f.state,
        })
        .collect();

    out.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    Ok(out)
}

pub fn parse_ps_capabilities_json(json_str: &str) -> Result<Vec<WindowsCapabilityInfo>, String> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }
    let raw: Vec<RawPsCapability> = serde_json::from_str(trimmed)
        .map_err(|e| format!("Failed parsing DISM Capabilities JSON: {e}"))?;

    let mut out: Vec<WindowsCapabilityInfo> = raw
        .into_iter()
        .map(|c| WindowsCapabilityInfo {
            name: c.name,
            state: c.state,
        })
        .collect();

    out.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    Ok(out)
}

pub async fn query_mounted_appx_list(mount_dir: &Path) -> Result<Vec<ProvisionedAppxInfo>, String> {
    query_mounted_appx_list_tracked(mount_dir, None).await
}

pub async fn query_mounted_appx_list_tracked(
    mount_dir: &Path,
    guard: Option<&ProcessGuard>,
) -> Result<Vec<ProvisionedAppxInfo>, String> {
    let esc = escape_ps_single_quoted(&mount_dir.to_string_lossy());
    let script = format!(
        "Import-Module Dism -ErrorAction Stop; \
         $items = @(Get-AppxProvisionedPackage -Path '{esc}' | Select-Object DisplayName, PackageName, @{{N='Version';E={{$_.Version.ToString()}}}}, PublisherId); \
         ConvertTo-Json -InputObject $items -Depth 4 -Compress"
    );
    let json = run_powershell_script_tracked(&script, guard).await?;
    parse_ps_appx_json(&json)
}

pub async fn query_mounted_features_list(
    mount_dir: &Path,
) -> Result<Vec<WindowsFeatureInfo>, String> {
    query_mounted_features_list_tracked(mount_dir, None).await
}

pub async fn query_mounted_features_list_tracked(
    mount_dir: &Path,
    guard: Option<&ProcessGuard>,
) -> Result<Vec<WindowsFeatureInfo>, String> {
    let esc = escape_ps_single_quoted(&mount_dir.to_string_lossy());
    let script = format!(
        "Import-Module Dism -ErrorAction Stop; \
         $items = @(Get-WindowsOptionalFeature -Path '{esc}' | Select-Object FeatureName, @{{N='State';E={{$_.State.ToString()}}}}); \
         ConvertTo-Json -InputObject $items -Depth 3 -Compress"
    );
    let json = run_powershell_script_tracked(&script, guard).await?;
    parse_ps_features_json(&json)
}

pub async fn query_mounted_capabilities_list(
    mount_dir: &Path,
) -> Result<Vec<WindowsCapabilityInfo>, String> {
    query_mounted_capabilities_list_tracked(mount_dir, None).await
}

pub async fn query_mounted_capabilities_list_tracked(
    mount_dir: &Path,
    guard: Option<&ProcessGuard>,
) -> Result<Vec<WindowsCapabilityInfo>, String> {
    let esc = escape_ps_single_quoted(&mount_dir.to_string_lossy());
    let script = format!(
        "Import-Module Dism -ErrorAction Stop; \
         $items = @(Get-WindowsCapability -Path '{esc}' | Select-Object Name, @{{N='State';E={{$_.State.ToString()}}}}); \
         ConvertTo-Json -InputObject $items -Depth 3 -Compress"
    );
    let json = run_powershell_script_tracked(&script, guard).await?;
    parse_ps_capabilities_json(&json)
}

/// Loads the catalog (Provisioned Appx, Optional Features, Capabilities) for a selected edition.
/// Uses disk cache when valid; otherwise performs a single read-only mount, extracts JSON via
/// PowerShell Dism module, and immediately unmounts with `/Discard`.
pub async fn load_image_catalog<F>(
    dism_path: &Path,
    wimlib_path: &Path,
    source_path: &Path,
    source_kind: &SourceKind,
    index: u32,
    workspace_dir: &Path,
    force_refresh: bool,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<ImageCatalog, String>
where
    F: FnMut(&str) + Send,
{
    let cache_path = catalog_cache_path(workspace_dir, source_path, index);
    if !force_refresh && cache_path.is_file() {
        if let Ok(raw) = fs::read_to_string(&cache_path) {
            if let Ok(mut cached) = serde_json::from_str::<ImageCatalog>(&raw) {
                cached.from_cache = true;
                on_log(&format!(
                    "Loaded edition catalog from cache ({} Appx, {} Features, {} Capabilities).",
                    cached.appx_packages.len(),
                    cached.features.len(),
                    cached.capabilities.len()
                ));
                return Ok(cached);
            }
        }
    }

    let inspect_dir = workspace_dir.join("cache").join("ro_inspect");
    let ro_mount_dir = inspect_dir.join("mount");
    let temp_wim = inspect_dir.join("ro_temp.wim");
    fs::create_dir_all(&inspect_dir)
        .map_err(|e| format!("Failed creating inspect directory: {e}"))?;

    let mut mounted_iso = false;
    let mut temp_wim_created = false;

    let res: Result<ImageCatalog, String> = async {
        let (wim_to_mount, mount_index) = match source_kind {
            SourceKind::Wim => (source_path.to_path_buf(), index),
            SourceKind::Esd => {
                on_log("Exporting ESD index to temporary WIM for read-only catalog inspection...");
                export_single_edition(wimlib_path, source_path, index, &temp_wim, guard, |l| {
                    on_log(l);
                })
                .await?;
                temp_wim_created = true;
                (temp_wim.clone(), 1)
            }
            SourceKind::Iso => {
                on_log(&format!(
                    "Temporarily mounting ISO for catalog inspection: {}",
                    source_path.display()
                ));
                let drive = mount_iso_drive_letter(source_path, guard).await?;
                mounted_iso = true;
                let drive_root = PathBuf::from(format!("{drive}\\"));
                let install_wim = drive_root.join("sources").join("install.wim");
                let install_esd = drive_root.join("sources").join("install.esd");

                if install_wim.is_file() {
                    // Export single index to temp WIM to avoid DISM read-only optical drive locking issues
                    on_log("Preparing temporary single-index WIM from ISO for catalog scan...");
                    export_single_edition(
                        wimlib_path,
                        &install_wim,
                        index,
                        &temp_wim,
                        guard,
                        |l| {
                            on_log(l);
                        },
                    )
                    .await?;
                    temp_wim_created = true;
                    dismount_iso(source_path).await;
                    mounted_iso = false;
                    (temp_wim.clone(), 1)
                } else if install_esd.is_file() {
                    on_log("Exporting ISO install.esd index to temporary WIM for catalog scan...");
                    export_single_edition(
                        wimlib_path,
                        &install_esd,
                        index,
                        &temp_wim,
                        guard,
                        |l| {
                            on_log(l);
                        },
                    )
                    .await?;
                    temp_wim_created = true;
                    dismount_iso(source_path).await;
                    mounted_iso = false;
                    (temp_wim.clone(), 1)
                } else {
                    return Err(
                        "ISO does not contain sources\\install.wim or sources\\install.esd"
                            .to_string(),
                    );
                }
            }
        };

        mount_wim(
            dism_path,
            &wim_to_mount,
            mount_index,
            &ro_mount_dir,
            true,
            workspace_dir,
            guard,
            |l| on_log(l),
        )
        .await?;

        let query_res: Result<ImageCatalog, String> = async {
            on_log("Querying Provisioned Appx packages via PowerShell Dism...");
            let appx_packages = query_mounted_appx_list_tracked(&ro_mount_dir, guard).await?;
            on_log("Querying Optional Features via PowerShell Dism...");
            let features = query_mounted_features_list_tracked(&ro_mount_dir, guard).await?;
            on_log("Querying Windows Capabilities via PowerShell Dism...");
            let capabilities =
                query_mounted_capabilities_list_tracked(&ro_mount_dir, guard).await?;

            Ok(ImageCatalog {
                source_path: source_path.to_string_lossy().to_string(),
                edition_index: index,
                from_cache: false,
                appx_packages,
                features,
                capabilities,
            })
        }
        .await;

        let unmount_res = unmount_wim(dism_path, &ro_mount_dir, false, workspace_dir, None, |l| {
            on_log(l)
        })
        .await;

        let catalog = query_res?;
        unmount_res?;

        if let Some(parent) = cache_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&catalog) {
            let _ = fs::write(&cache_path, json);
        }

        on_log(&format!(
            "Catalog cached ({} Appx, {} Features, {} Capabilities).",
            catalog.appx_packages.len(),
            catalog.features.len(),
            catalog.capabilities.len()
        ));

        Ok(catalog)
    }
    .await;

    if mounted_iso {
        dismount_iso(source_path).await;
    }
    if temp_wim_created && temp_wim.exists() {
        let _ = fs::remove_file(&temp_wim);
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_appx_json_array_and_marks_protected() {
        let json = r#"[
          {
            "DisplayName": "Microsoft Store",
            "PackageName": "Microsoft.WindowsStore_22409.1401.6.0_x64__8wekyb3d8bbwe",
            "Version": "22409.1401.6.0",
            "PublisherId": "8wekyb3d8bbwe"
          },
          {
            "DisplayName": "Microsoft Clipchamp",
            "PackageName": "Clipchamp.Clipchamp_3.1.0.0_neutral__yxz26nhyzhsrt",
            "Version": "3.1.0.0",
            "PublisherId": "yxz26nhyzhsrt"
          }
        ]"#;
        let items = parse_ps_appx_json(json).unwrap();
        assert_eq!(items.len(), 2);
        let store = items
            .iter()
            .find(|i| i.package_family_name == "Microsoft.WindowsStore_8wekyb3d8bbwe")
            .unwrap();
        assert!(store.is_protected);
        assert_eq!(store.protection_reason.as_deref(), Some("store"));

        let clip = items
            .iter()
            .find(|i| i.display_name == "Microsoft Clipchamp")
            .unwrap();
        assert!(!clip.is_protected);
    }

    #[test]
    fn parses_features_and_capabilities_json() {
        let feat_json = r#"[
          {"FeatureName": "NetFx3", "State": "DisabledWithPayloadRemoved"},
          {"FeatureName": "Microsoft-Hyper-V-All", "State": "Enabled"}
        ]"#;
        let feats = parse_ps_features_json(feat_json).unwrap();
        assert_eq!(feats.len(), 2);
        assert_eq!(feats[1].name, "NetFx3");

        let cap_json = r#"[{"Name": "OpenSSH.Client~~~~0.0.1.0", "State": "Installed"}]"#;
        let caps = parse_ps_capabilities_json(cap_json).unwrap();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].state, "Installed");
    }
}
