use crate::fs_utils::{
    is_path_overlapping_dir, is_symlink_or_reparse, is_unc_or_device_path,
    normalize_path_for_prefix,
};
use crate::image::run_powershell_script_with_env;
use crate::process::{ProcessGuard, run_tracked_command};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverScanResult {
    pub path: String,
    pub is_directory: bool,
    pub inf_count: usize,
    pub sample_infs: Vec<String>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledDriverInfo {
    pub published_name: String,
    pub original_file_name: String,
    pub class_name: String,
    pub category: String,
    pub provider_name: String,
    pub date: String,
    pub version: String,
    pub boot_critical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverQueueItem {
    pub path: String,
    pub is_directory: bool,
    pub recurse: bool,
    pub force_unsigned: bool,
    pub inject_to_install_wim: bool,
    pub inject_to_boot_wim: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DriversBuildPlan {
    pub items: Vec<DriverQueueItem>,
    pub remove_published_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDriverExportRequest {
    pub published_names: Vec<String>,
    pub export_all: bool,
    pub destination_dir: String,
    pub auto_split_storage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverExportProgress {
    pub current: usize,
    pub total: usize,
    pub driver_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDriverExportResult {
    pub destination_dir: String,
    pub exported_count: usize,
    pub failed: Vec<String>,
    pub storage_scan: Option<DriverScanResult>,
    pub other_scan: Option<DriverScanResult>,
    pub combined_scan: Option<DriverScanResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RawPsDriver {
    pub driver: String,
    pub original_file_name: Option<String>,
    pub class_name: Option<String>,
    pub provider_name: Option<String>,
    pub date: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub boot_critical: bool,
}

pub fn classify_driver_class(class_name: &str) -> &'static str {
    let norm = class_name.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return "other";
    }
    if matches!(
        norm.as_str(),
        "scsiadapter" | "hdc" | "diskdrive" | "nvme" | "storagevolumes" | "volumemanager"
    ) || norm.contains("storage")
        || norm.contains("raid")
    {
        return "storage";
    }
    if matches!(
        norm.as_str(),
        "net" | "netclient" | "netservice" | "nettrans" | "netadaptercx"
    ) || norm.contains("network")
    {
        return "network";
    }
    if matches!(norm.as_str(), "display" | "monitor") {
        return "display";
    }
    if matches!(
        norm.as_str(),
        "media" | "audioendpoint" | "audioprocessingobject"
    ) || norm.contains("audio")
    {
        return "audio";
    }
    if matches!(
        norm.as_str(),
        "usb" | "usbdevice" | "usbconnectormanagers" | "system" | "chipset" | "thunderbolt"
    ) {
        return "usb";
    }
    if norm == "bluetooth" || norm.contains("bluetooth") {
        return "bluetooth";
    }
    if matches!(
        norm.as_str(),
        "hidclass" | "keyboard" | "mouse" | "biometric"
    ) {
        return "input";
    }
    "other"
}

pub fn is_valid_published_inf_name(name: &str) -> bool {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    let Some(middle) = lower
        .strip_prefix("oem")
        .and_then(|s| s.strip_suffix(".inf"))
    else {
        return false;
    };
    !middle.is_empty() && middle.len() <= 6 && middle.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn map_raw_drivers(raw: Vec<RawPsDriver>) -> Vec<InstalledDriverInfo> {
    let mut out: Vec<InstalledDriverInfo> = raw
        .into_iter()
        .filter_map(|item| {
            let published = item.driver.trim().to_string();
            if published.is_empty() {
                return None;
            }
            let orig_raw = item.original_file_name.unwrap_or_default();
            let orig_clean = orig_raw
                .trim()
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(orig_raw.trim())
                .to_string();
            let original_file_name = if orig_clean.is_empty() {
                published.clone()
            } else {
                orig_clean
            };
            let class_name = item.class_name.unwrap_or_default().trim().to_string();
            let category = classify_driver_class(&class_name).to_string();
            Some(InstalledDriverInfo {
                published_name: published,
                original_file_name,
                class_name,
                category,
                provider_name: item.provider_name.unwrap_or_default().trim().to_string(),
                date: item.date.unwrap_or_default().trim().to_string(),
                version: item.version.unwrap_or_default().trim().to_string(),
                boot_critical: item.boot_critical,
            })
        })
        .collect();

    out.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| {
                a.original_file_name
                    .to_ascii_lowercase()
                    .cmp(&b.original_file_name.to_ascii_lowercase())
            })
            .then_with(|| {
                a.published_name
                    .to_ascii_lowercase()
                    .cmp(&b.published_name.to_ascii_lowercase())
            })
    });
    out
}

pub fn parse_ps_drivers_json(json_str: &str) -> Result<Vec<InstalledDriverInfo>, String> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }
    let raw: Vec<RawPsDriver> = serde_json::from_str(trimmed)
        .map_err(|e| format!("Failed parsing DISM Drivers JSON: {e}"))?;
    Ok(map_raw_drivers(raw))
}

pub async fn list_host_drivers(
    guard: Option<&ProcessGuard>,
) -> Result<Vec<InstalledDriverInfo>, String> {
    let script = "Import-Module Dism -ErrorAction Stop; \
         $items = @(Get-WindowsDriver -Online | Select-Object \
           @{N='Driver';E={if($_.Driver){$_.Driver.ToString()}else{''}}}, \
           @{N='OriginalFileName';E={if($_.OriginalFileName){$_.OriginalFileName.ToString()}else{''}}}, \
           @{N='ClassName';E={if($_.ClassName){$_.ClassName.ToString()}else{''}}}, \
           @{N='ProviderName';E={if($_.ProviderName){$_.ProviderName.ToString()}else{''}}}, \
           @{N='Date';E={if($_.Date){$_.Date.ToString('yyyy-MM-dd')}else{''}}}, \
           @{N='Version';E={if($_.Version){$_.Version.ToString()}else{''}}}, \
           @{N='BootCritical';E={[bool]$_.BootCritical}}); \
         ConvertTo-Json -InputObject $items -Depth 4 -Compress";
    let json = run_powershell_script_with_env(script, &[], guard).await?;
    parse_ps_drivers_json(&json)
}

pub fn validate_driver_export_dir(path: &Path, session_dir: Option<&Path>) -> Result<(), String> {
    let raw = path.to_string_lossy();
    if raw.trim().is_empty() {
        return Err("Driver export destination path must not be empty.".to_string());
    }
    if is_unc_or_device_path(path) {
        return Err(format!(
            "Network/UNC or Windows device paths are not allowed for driver export destination: '{}'",
            path.display()
        ));
    }
    if !path.is_absolute() {
        return Err(format!(
            "Driver export destination must be an absolute path: '{}'",
            path.display()
        ));
    }
    if let Ok(meta) = fs::symlink_metadata(path) {
        if is_symlink_or_reparse(&meta) {
            return Err(format!(
                "Symbolic links and junctions are not supported for driver export destination: '{}'",
                path.display()
            ));
        }
        if !meta.is_dir() {
            return Err(format!(
                "Driver export destination '{}' points to an existing file.",
                path.display()
            ));
        }
    }
    if let Some(sess) = session_dir
        && is_path_overlapping_dir(path, sess)
    {
        return Err(format!(
            "Driver export destination cannot reside inside temporary workspace session_current: '{}'",
            path.display()
        ));
    }
    let normalized = normalize_path_for_prefix(path);
    if normalized.parent().is_none() {
        return Err(format!(
            "Drive root cannot be used as driver export destination: '{}'",
            path.display()
        ));
    }
    for sys_env in ["SystemRoot", "WINDIR"] {
        if let Some(val) = std::env::var_os(sys_env) {
            let sys_path = PathBuf::from(val);
            if is_path_overlapping_dir(path, &sys_path) {
                return Err(format!(
                    "Windows directory cannot be used as driver export destination: '{}'",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn is_inf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("inf"))
        .unwrap_or(false)
}

pub fn scan_driver_path(path: &Path) -> Result<DriverScanResult, String> {
    let top_meta = fs::symlink_metadata(path)
        .map_err(|_| format!("Driver path does not exist: {}", path.display()))?;
    if is_symlink_or_reparse(&top_meta) {
        return Err(format!(
            "Symbolic links and junctions are not supported for driver paths: {}",
            path.display()
        ));
    }

    if top_meta.is_file() {
        if !is_inf_file(path) {
            return Err(format!(
                "Unsupported driver file '{}'. Expected .inf file.",
                path.display()
            ));
        }
        let total_bytes = top_meta.len();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("driver.inf")
            .to_string();
        return Ok(DriverScanResult {
            path: path.to_string_lossy().to_string(),
            is_directory: false,
            inf_count: 1,
            sample_infs: vec![name],
            total_bytes,
        });
    }

    if top_meta.is_dir() {
        let mut inf_names = Vec::new();
        let mut total_bytes = 0u64;
        let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let entries = fs::read_dir(&dir)
                .map_err(|e| format!("Failed reading driver folder '{}': {e}", dir.display()))?;
            for entry in entries.flatten() {
                let child = entry.path();
                let Ok(meta) = fs::symlink_metadata(&child) else {
                    continue;
                };
                if is_symlink_or_reparse(&meta) {
                    continue;
                }
                if meta.is_dir() {
                    stack.push(child);
                } else if meta.is_file() {
                    total_bytes += meta.len();
                    if is_inf_file(&child)
                        && let Some(name) = child.file_name().and_then(|n| n.to_str())
                    {
                        inf_names.push(name.to_string());
                    }
                }
            }
        }

        if inf_names.is_empty() {
            return Err("No .inf driver files found in the selected folder.".to_string());
        }

        inf_names.sort();
        let inf_count = inf_names.len();
        let sample_infs = inf_names.into_iter().take(5).collect();

        return Ok(DriverScanResult {
            path: path.to_string_lossy().to_string(),
            is_directory: true,
            inf_count,
            sample_infs,
            total_bytes,
        });
    }

    Err(format!("Invalid driver path: {}", path.display()))
}

pub fn build_add_driver_args(
    mount_dir: &Path,
    log_path: &Path,
    item: &DriverQueueItem,
) -> Vec<String> {
    let mut args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Add-Driver".to_string(),
        format!("/Driver:{}", item.path),
    ];

    // DISM returns error 87 if /Recurse is passed for a single .inf file
    if item.is_directory && item.recurse {
        args.push("/Recurse".to_string());
    }
    if item.force_unsigned {
        args.push("/ForceUnsigned".to_string());
    }

    args
}

pub async fn inject_driver<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    item: &DriverQueueItem,
    target_label: &str,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let args = build_add_driver_args(mount_dir, log_path, item);
    on_log(&format!(
        "Injecting driver(s) into {target_label}: {}",
        item.path
    ));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed injecting driver '{}' into {target_label} (exit {}): {} {}",
            item.path,
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

pub fn build_remove_driver_args(
    mount_dir: &Path,
    log_path: &Path,
    published_name: &str,
) -> Result<Vec<String>, String> {
    let trimmed = published_name.trim();
    if !is_valid_published_inf_name(trimmed) {
        return Err(format!(
            "Invalid OEM driver published name '{published_name}'. Expected format like oem12.inf."
        ));
    }
    Ok(vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Remove-Driver".to_string(),
        format!("/Driver:{trimmed}"),
    ])
}

pub async fn remove_driver<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    published_name: &str,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let args = build_remove_driver_args(mount_dir, log_path, published_name)?;
    on_log(&format!(
        "Removing driver '{published_name}' from install.wim..."
    ));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed removing driver '{published_name}' from install.wim (exit {}): {} {}",
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn export_host_drivers<F, P>(
    dism_path: &Path,
    pnputil_path: &Path,
    req: &HostDriverExportRequest,
    host_drivers: &[InstalledDriverInfo],
    session_dir: Option<&Path>,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
    mut on_progress: P,
) -> Result<HostDriverExportResult, String>
where
    F: FnMut(&str) + Send,
    P: FnMut(DriverExportProgress) + Send,
{
    let dest_root = PathBuf::from(req.destination_dir.trim());
    validate_driver_export_dir(&dest_root, session_dir)?;

    let mut category_by_pub: HashMap<String, String> = HashMap::new();
    for d in host_drivers {
        category_by_pub.insert(d.published_name.to_ascii_lowercase(), d.category.clone());
    }

    let selected_names: Vec<String> = if req.export_all {
        host_drivers
            .iter()
            .map(|d| d.published_name.clone())
            .collect()
    } else {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for name in &req.published_names {
            let trimmed = name.trim();
            if !is_valid_published_inf_name(trimmed) {
                return Err(format!(
                    "Invalid OEM driver published name '{name}'. Expected format like oem12.inf."
                ));
            }
            let key = trimmed.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(trimmed.to_string());
            }
        }
        out
    };

    if selected_names.is_empty() {
        return Err("No host drivers selected for export.".to_string());
    }

    tokio::fs::create_dir_all(&dest_root).await.map_err(|e| {
        format!(
            "Failed creating driver export folder '{}': {e}",
            dest_root.display()
        )
    })?;

    if req.export_all && !req.auto_split_storage {
        on_log(&format!(
            "Exporting all third-party host drivers to '{}' via DISM...",
            dest_root.display()
        ));
        on_progress(DriverExportProgress {
            current: 0,
            total: selected_names.len(),
            driver_name: "*".to_string(),
        });
        let args = vec![
            "/English".to_string(),
            "/Online".to_string(),
            "/Export-Driver".to_string(),
            format!("/Destination:{}", dest_root.display()),
        ];
        let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
            if !l.trim().is_empty() {
                on_log(l);
            }
        })
        .await?;
        if out.exit_code != 0 {
            return Err(format!(
                "Failed exporting host drivers via DISM (exit {}): {} {}",
                out.exit_code,
                out.stdout.trim(),
                out.stderr.trim()
            ));
        }

        let scan_path_clone = dest_root.clone();
        let combined_scan = tokio::task::spawn_blocking(move || scan_driver_path(&scan_path_clone))
            .await
            .map_err(|e| format!("Driver scan task failed: {e}"))??;
        on_progress(DriverExportProgress {
            current: selected_names.len(),
            total: selected_names.len(),
            driver_name: "*".to_string(),
        });

        return Ok(HostDriverExportResult {
            destination_dir: dest_root.to_string_lossy().to_string(),
            exported_count: combined_scan.inf_count,
            failed: Vec::new(),
            storage_scan: None,
            other_scan: None,
            combined_scan: Some(combined_scan),
        });
    }

    let storage_root = dest_root.join("storage");
    let other_root = dest_root.join("system");
    let mut storage_used = false;
    let mut other_used = false;
    let mut exported_count = 0usize;
    let mut failed: Vec<String> = Vec::new();
    let total = selected_names.len();

    for (idx, pub_name) in selected_names.iter().enumerate() {
        if let Some(g) = guard
            && g.is_cancel_requested()
        {
            return Err("Operation cancelled by user.".to_string());
        }

        on_progress(DriverExportProgress {
            current: idx + 1,
            total,
            driver_name: pub_name.clone(),
        });

        let cat = category_by_pub
            .get(&pub_name.to_ascii_lowercase())
            .map(String::as_str)
            .unwrap_or("other");
        let is_storage = req.auto_split_storage && cat == "storage";
        let target_base = if !req.auto_split_storage {
            &dest_root
        } else if is_storage {
            storage_used = true;
            &storage_root
        } else {
            other_used = true;
            &other_root
        };

        let stem = pub_name.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
        let sub_name = pub_name
            .strip_suffix(".inf")
            .or_else(|| pub_name.strip_suffix(".INF"))
            .unwrap_or(stem);
        let driver_dest = target_base.join(sub_name);
        tokio::fs::create_dir_all(&driver_dest).await.map_err(|e| {
            format!(
                "Failed creating driver export folder '{}': {e}",
                driver_dest.display()
            )
        })?;

        on_log(&format!(
            "Exporting host driver {pub_name} ({cat}) -> {}",
            driver_dest.display()
        ));
        let args = vec![
            "/export-driver".to_string(),
            pub_name.clone(),
            driver_dest.to_string_lossy().to_string(),
        ];
        let out = run_tracked_command(&pnputil_path.to_string_lossy(), &args, None, guard, |l| {
            if !l.trim().is_empty() {
                on_log(l);
            }
        })
        .await?;

        if out.exit_code == 0 {
            exported_count += 1;
        } else {
            let reason = if !out.stderr.trim().is_empty() {
                out.stderr.trim().to_string()
            } else {
                out.stdout.trim().to_string()
            };
            on_log(&format!(
                "Warning: failed exporting {pub_name} (exit {}): {reason}",
                out.exit_code
            ));
            let _ = tokio::fs::remove_dir_all(&driver_dest).await;
            failed.push(format!("{pub_name}: {reason}"));
        }
    }

    if exported_count == 0 {
        return Err("Failed exporting any selected host drivers.".to_string());
    }

    let storage_scan = if storage_used && storage_root.is_dir() {
        let p = storage_root.clone();
        tokio::task::spawn_blocking(move || scan_driver_path(&p).ok())
            .await
            .unwrap_or(None)
    } else {
        None
    };

    let other_scan = if other_used && other_root.is_dir() {
        let p = other_root.clone();
        tokio::task::spawn_blocking(move || scan_driver_path(&p).ok())
            .await
            .unwrap_or(None)
    } else {
        None
    };

    let combined_scan = if !req.auto_split_storage {
        let p = dest_root.clone();
        tokio::task::spawn_blocking(move || scan_driver_path(&p).ok())
            .await
            .unwrap_or(None)
    } else {
        None
    };

    Ok(HostDriverExportResult {
        destination_dir: dest_root.to_string_lossy().to_string(),
        exported_count,
        failed,
        storage_scan,
        other_scan,
        combined_scan,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_nested_driver_folder_and_single_inf() {
        let base = std::env::temp_dir().join(format!("w11_drv_test_{}", std::process::id()));
        let sub = base.join("nvme");
        let empty = base.join("empty");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&empty).unwrap();

        let inf1 = base.join("iastorvd.inf");
        let inf2 = sub.join("nvme.inf");
        let sys = sub.join("nvme.sys");
        fs::write(&inf1, b"inf1").unwrap();
        fs::write(&inf2, b"inf2").unwrap();
        fs::write(&sys, b"binarysys").unwrap();

        let folder_res = scan_driver_path(&base).unwrap();
        assert!(folder_res.is_directory);
        assert_eq!(folder_res.inf_count, 2);
        assert_eq!(folder_res.sample_infs, vec!["iastorvd.inf", "nvme.inf"]);
        assert_eq!(folder_res.total_bytes, 17);

        let file_res = scan_driver_path(&inf1).unwrap();
        assert!(!file_res.is_directory);
        assert_eq!(file_res.inf_count, 1);

        let empty_err = scan_driver_path(&empty).unwrap_err();
        assert!(empty_err.contains("No .inf"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn builds_driver_args_respecting_directory_and_force_unsigned() {
        let mount = Path::new(r"C:\work\mount");
        let log = Path::new(r"C:\work\dism.log");

        let folder_item = DriverQueueItem {
            path: r"C:\drivers\intel_vmd".to_string(),
            is_directory: true,
            recurse: true,
            force_unsigned: true,
            inject_to_install_wim: true,
            inject_to_boot_wim: true,
        };
        let args_dir = build_add_driver_args(mount, log, &folder_item);
        assert!(args_dir.contains(&"/Add-Driver".to_string()));
        assert!(args_dir.contains(&"/Recurse".to_string()));
        assert!(args_dir.contains(&"/ForceUnsigned".to_string()));

        // Even if recurse=true, single .inf file must NOT include /Recurse
        let file_item = DriverQueueItem {
            path: r"C:\drivers\nvme.inf".to_string(),
            is_directory: false,
            recurse: true,
            force_unsigned: false,
            inject_to_install_wim: true,
            inject_to_boot_wim: false,
        };
        let args_file = build_add_driver_args(mount, log, &file_item);
        assert!(!args_file.contains(&"/Recurse".to_string()));
        assert!(!args_file.contains(&"/ForceUnsigned".to_string()));
    }

    #[test]
    fn builds_driver_args_with_spaces_and_cyrillic() {
        let mount = Path::new(r"D:\Сборка\mount_boot 2");
        let log = Path::new(r"D:\Сборка\dism.log");
        let item = DriverQueueItem {
            path: r"E:\Драйверы NVMe и Intel VMD\iaStorVD.inf".to_string(),
            is_directory: false,
            recurse: true,
            force_unsigned: true,
            inject_to_install_wim: true,
            inject_to_boot_wim: true,
        };
        let args = build_add_driver_args(mount, log, &item);
        assert!(args.contains(&r"/Image:D:\Сборка\mount_boot 2".to_string()));
        assert!(args.contains(&r"/LogPath:D:\Сборка\dism.log".to_string()));
        assert!(args.contains(&r"/Driver:E:\Драйверы NVMe и Intel VMD\iaStorVD.inf".to_string()));
        assert!(args.contains(&"/ForceUnsigned".to_string()));
        assert!(!args.contains(&"/Recurse".to_string()));
    }

    #[test]
    fn validates_published_inf_names_and_remove_args() {
        assert!(is_valid_published_inf_name("oem0.inf"));
        assert!(is_valid_published_inf_name("OEM123.INF"));
        assert!(!is_valid_published_inf_name(""));
        assert!(!is_valid_published_inf_name("nvme.inf"));
        assert!(!is_valid_published_inf_name("oem.inf"));
        assert!(!is_valid_published_inf_name(r"..\oem1.inf"));
        assert!(!is_valid_published_inf_name("oem1.inf /all"));

        let mount = Path::new(r"C:\work\mount");
        let log = Path::new(r"C:\work\dism.log");
        let args = build_remove_driver_args(mount, log, "oem12.inf").unwrap();
        assert!(args.contains(&"/Remove-Driver".to_string()));
        assert!(args.contains(&"/Driver:oem12.inf".to_string()));
        assert!(build_remove_driver_args(mount, log, "bad.inf").is_err());
    }

    #[test]
    fn classifies_driver_classes_and_parses_json() {
        assert_eq!(classify_driver_class("SCSIAdapter"), "storage");
        assert_eq!(classify_driver_class("Net"), "network");
        assert_eq!(classify_driver_class("Display"), "display");
        assert_eq!(classify_driver_class("MEDIA"), "audio");
        assert_eq!(classify_driver_class("USB"), "usb");
        assert_eq!(classify_driver_class("Bluetooth"), "bluetooth");
        assert_eq!(classify_driver_class("HIDClass"), "input");
        assert_eq!(classify_driver_class("Printer"), "other");

        let json = r#"[
          {
            "Driver": "oem5.inf",
            "OriginalFileName": "C:\\Windows\\System32\\DriverStore\\FileRepository\\iastorvd.inf_amd64_1234\\iastorvd.inf",
            "ClassName": "SCSIAdapter",
            "ProviderName": "Intel Corporation",
            "Date": "2025-02-14",
            "Version": "20.1.0.1015",
            "BootCritical": true
          },
          {
            "Driver": "oem8.inf",
            "OriginalFileName": "e1d.inf",
            "ClassName": "Net",
            "ProviderName": "Intel",
            "Date": "2024-11-01",
            "Version": "12.19.2.61",
            "BootCritical": false
          }
        ]"#;
        let parsed = parse_ps_drivers_json(json).unwrap();
        assert_eq!(parsed.len(), 2);
        let net = parsed
            .iter()
            .find(|d| d.published_name == "oem8.inf")
            .unwrap();
        assert_eq!(net.category, "network");
        assert!(!net.boot_critical);

        let stg = parsed
            .iter()
            .find(|d| d.published_name == "oem5.inf")
            .unwrap();
        assert_eq!(stg.category, "storage");
        assert_eq!(stg.original_file_name, "iastorvd.inf");
        assert!(stg.boot_critical);
    }

    #[test]
    fn validates_driver_export_directory_safely() {
        let sess = Path::new(r"C:\work\session_current");
        assert!(validate_driver_export_dir(Path::new(""), Some(sess)).is_err());
        assert!(validate_driver_export_dir(Path::new("rel\\drv"), Some(sess)).is_err());
        assert!(validate_driver_export_dir(Path::new(r"\\server\share\drv"), Some(sess)).is_err());
        assert!(validate_driver_export_dir(Path::new(r"C:\"), Some(sess)).is_err());
        assert!(
            validate_driver_export_dir(Path::new(r"C:\work\session_current\drv"), Some(sess))
                .is_err()
        );
        if let Some(sys) = std::env::var_os("SystemRoot") {
            assert!(validate_driver_export_dir(&PathBuf::from(sys), Some(sess)).is_err());
        }
        assert!(validate_driver_export_dir(Path::new(r"D:\ExportedDrivers"), Some(sess)).is_ok());
    }
}
