use crate::process::{run_tracked_command, ProcessGuard};
use serde::{Deserialize, Serialize};
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
}

fn is_inf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("inf"))
        .unwrap_or(false)
}

fn is_symlink_or_reparse(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return true;
        }
    }
    false
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
                    if is_inf_file(&child) {
                        if let Some(name) = child.file_name().and_then(|n| n.to_str()) {
                            inf_names.push(name.to_string());
                        }
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
}
