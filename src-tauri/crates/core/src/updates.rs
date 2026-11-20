use crate::process::{run_tracked_command, ProcessGuard};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateQueueItem {
    pub path: String,
    pub is_directory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScanInfo {
    pub path: String,
    pub is_directory: bool,
    pub kind: String,
    pub package_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdatesBuildPlan {
    pub items: Vec<UpdateQueueItem>,
    pub run_component_cleanup: bool,
    pub cleanup_reset_base: bool,
}

fn is_update_file(path: &Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())?;
    match ext.as_str() {
        "msu" => Some("msu"),
        "cab" => Some("cab"),
        _ => None,
    }
}

pub fn inspect_update_path(path: &Path) -> Result<UpdateScanInfo, String> {
    if !path.exists() {
        return Err(format!("Update path does not exist: {}", path.display()));
    }

    if path.is_file() {
        let kind = is_update_file(path).ok_or_else(|| {
            format!(
                "Unsupported update file '{}'. Expected .msu or .cab.",
                path.display()
            )
        })?;
        let total_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        return Ok(UpdateScanInfo {
            path: path.to_string_lossy().to_string(),
            is_directory: false,
            kind: kind.to_string(),
            package_count: 1,
            total_bytes,
        });
    }

    if path.is_dir() {
        let mut package_count = 0usize;
        let mut total_bytes = 0u64;
        let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let entries = fs::read_dir(&dir)
                .map_err(|e| format!("Failed reading update folder '{}': {e}", dir.display()))?;
            for entry in entries.flatten() {
                let child = entry.path();
                if child.is_dir() {
                    stack.push(child);
                } else if child.is_file() && is_update_file(&child).is_some() {
                    package_count += 1;
                    total_bytes += fs::metadata(&child).map(|m| m.len()).unwrap_or(0);
                }
            }
        }

        if package_count == 0 {
            return Err("Selected folder contains no .msu or .cab update packages.".to_string());
        }

        return Ok(UpdateScanInfo {
            path: path.to_string_lossy().to_string(),
            is_directory: true,
            kind: "folder".to_string(),
            package_count,
            total_bytes,
        });
    }

    Err(format!("Invalid update path: {}", path.display()))
}

pub fn build_add_packages_args(
    mount_dir: &Path,
    log_path: &Path,
    package_paths: &[&Path],
) -> Vec<String> {
    let mut args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Add-Package".to_string(),
    ];

    for pkg in package_paths {
        args.push(format!("/PackagePath:{}", pkg.display()));
    }
    args
}

pub fn explain_dism_update_error(exit_code: i32, stdout: &str, stderr: &str) -> Option<String> {
    let hex_code = format!("0x{:08x}", exit_code as u32);
    let combined = format!("{stdout}\n{stderr}\n{hex_code}").to_ascii_lowercase();

    if combined.contains("0x800f0993")
        || combined.contains("0x80073712")
        || combined.contains("0x800f0988")
        || combined.contains("0x800f0982")
    {
        return Some(
            "Windows 11 24H2+ Checkpoint Cumulative Update prerequisite missing (0x800f0993 / 0x80073712): place the baseline Checkpoint .msu and target cumulative .msu in the same folder or queue them together so DISM can resolve the update chain."
                .to_string(),
        );
    }

    if combined.contains("0x800f081e") {
        return Some(
            "Update package is not applicable to this Windows 11 image (0x800f081e): verify build, edition, and x64 architecture."
                .to_string(),
        );
    }

    if combined.contains("0x800f0823") {
        return Some(
            "Update package requires a newer Servicing Stack Update (SSU) to be integrated first (0x800f0823)."
                .to_string(),
        );
    }

    if combined.contains("0x800f0806") {
        return Some(
            "The offline image has pending CBS servicing operations (0x800f0806) that will complete on first boot."
                .to_string(),
        );
    }

    None
}

pub fn is_dism_pending_error(exit_code: i32, stdout: &str, stderr: &str) -> bool {
    let hex_code = format!("0x{:08x}", exit_code as u32);
    let combined = format!("{stdout}\n{stderr}\n{hex_code}").to_ascii_lowercase();
    combined.contains("0x800f0806")
}

pub async fn apply_updates<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    items: &[UpdateQueueItem],
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let dirs: Vec<&UpdateQueueItem> = items.iter().filter(|i| i.is_directory).collect();
    let files: Vec<&UpdateQueueItem> = items.iter().filter(|i| !i.is_directory).collect();

    for dir_item in dirs {
        let dir_path = Path::new(&dir_item.path);
        let args = build_add_packages_args(mount_dir, log_path, &[dir_path]);
        on_log(&format!(
            "Integrating update folder (DISM resolves Checkpoint CU chain): {}",
            dir_path.display()
        ));

        let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
            if !l.trim().is_empty() {
                on_log(l);
            }
        })
        .await?;

        if out.exit_code != 0 {
            let hint = explain_dism_update_error(out.exit_code, &out.stdout, &out.stderr);
            let mut msg = format!(
                "Failed integrating update folder '{}' (exit {}): {} {}",
                dir_path.display(),
                out.exit_code,
                out.stdout.trim(),
                out.stderr.trim()
            );
            if let Some(h) = hint {
                msg.push_str(&format!("\nHint: {h}"));
            }
            return Err(msg);
        }
    }

    if !files.is_empty() {
        let file_paths: Vec<&Path> = files.iter().map(|f| Path::new(&f.path)).collect();
        let args = build_add_packages_args(mount_dir, log_path, &file_paths);
        on_log(&format!(
            "Integrating {} update package(s) in batched DISM call...",
            file_paths.len()
        ));

        let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
            if !l.trim().is_empty() {
                on_log(l);
            }
        })
        .await?;

        if out.exit_code != 0 {
            let hint = explain_dism_update_error(out.exit_code, &out.stdout, &out.stderr);
            let mut msg = format!(
                "Failed integrating update packages (exit {}): {} {}",
                out.exit_code,
                out.stdout.trim(),
                out.stderr.trim()
            );
            if let Some(h) = hint {
                msg.push_str(&format!("\nHint: {h}"));
            }
            return Err(msg);
        }
    }

    Ok(())
}

pub fn build_component_cleanup_args(
    mount_dir: &Path,
    log_path: &Path,
    reset_base: bool,
) -> Vec<String> {
    let mut args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Cleanup-Image".to_string(),
        "/StartComponentCleanup".to_string(),
    ];
    if reset_base {
        args.push("/ResetBase".to_string());
    }
    args
}

pub async fn run_component_cleanup<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    reset_base: bool,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<bool, String>
where
    F: FnMut(&str) + Send,
{
    let args = build_component_cleanup_args(mount_dir, log_path, reset_base);
    on_log(&format!(
        "Running DISM /StartComponentCleanup (reset_base={reset_base})..."
    ));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        if is_dism_pending_error(out.exit_code, &out.stdout, &out.stderr) {
            return Ok(false);
        }
        return Err(format!(
            "DISM /StartComponentCleanup failed (exit {}): {} {}",
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspects_update_folder_and_rejects_empty() {
        let base = std::env::temp_dir().join(format!("w11_upd_test_{}", std::process::id()));
        let sub = base.join("nested");
        let empty = base.join("empty");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&empty).unwrap();

        fs::write(base.join("checkpoint.msu"), b"12345").unwrap();
        fs::write(sub.join("ssu.cab"), b"123").unwrap();
        fs::write(base.join("notes.txt"), b"ignore me").unwrap();

        let info = inspect_update_path(&base).unwrap();
        assert!(info.is_directory);
        assert_eq!(info.kind, "folder");
        assert_eq!(info.package_count, 2);
        assert_eq!(info.total_bytes, 8);

        let empty_err = inspect_update_path(&empty).unwrap_err();
        assert!(empty_err.contains("no .msu or .cab"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn builds_add_packages_args_for_folder_and_batch() {
        let mount = Path::new(r"C:\work\mount");
        let log = Path::new(r"C:\work\dism.log");

        let single = build_add_packages_args(mount, log, &[Path::new(r"C:\upd\24h2")]);
        assert!(single.contains(&"/Add-Package".to_string()));
        assert!(single.contains(&r"/PackagePath:C:\upd\24h2".to_string()));

        let batch = build_add_packages_args(
            mount,
            log,
            &[Path::new(r"C:\upd\ckpt.msu"), Path::new(r"C:\upd\lcu.msu")],
        );
        assert!(batch.contains(&r"/PackagePath:C:\upd\ckpt.msu".to_string()));
        assert!(batch.contains(&r"/PackagePath:C:\upd\lcu.msu".to_string()));
    }

    #[test]
    fn explains_known_dism_update_errors() {
        let h1 = explain_dism_update_error(-2146498157, "Error: 0x800f0993", "").unwrap();
        assert!(h1.contains("Checkpoint"));

        let h2 = explain_dism_update_error(1, "", "HRESULT 0x80073712 missing file").unwrap();
        assert!(h2.contains("Checkpoint"));

        let h3 = explain_dism_update_error(-2146498530, "", "").unwrap();
        assert!(h3.contains("0x800f081e"));
    }

    #[test]
    fn builds_component_cleanup_args_with_safe_default() {
        let mount = Path::new(r"C:\work\mount");
        let log = Path::new(r"C:\work\dism.log");

        let safe = build_component_cleanup_args(mount, log, false);
        assert!(safe.contains(&"/StartComponentCleanup".to_string()));
        assert!(!safe.contains(&"/ResetBase".to_string()));

        let reset = build_component_cleanup_args(mount, log, true);
        assert!(reset.contains(&"/StartComponentCleanup".to_string()));
        assert!(reset.contains(&"/ResetBase".to_string()));
    }

    #[test]
    fn detects_pending_operations_error_0x800f0806() {
        assert!(is_dism_pending_error(-2146498554, "", ""));
        assert!(is_dism_pending_error(1, "Error: 0x800f0806", ""));
        assert!(!is_dism_pending_error(
            0,
            "The operation completed successfully.",
            ""
        ));
    }

    #[test]
    fn builds_update_args_with_spaces_and_cyrillic() {
        let mount = Path::new(r"C:\Работа с Win11\mount dir");
        let log = Path::new(r"C:\Работа с Win11\dism.log");
        let pkg1 = Path::new(r"D:\Обновления 24H2\Checkpoint\kb_ckpt.msu");
        let pkg2 = Path::new(r"D:\Обновления 24H2\Cumulative\kb_cu.msu");

        let args = build_add_packages_args(mount, log, &[pkg1, pkg2]);
        assert!(args.contains(&r"/Image:C:\Работа с Win11\mount dir".to_string()));
        assert!(args.contains(&r"/LogPath:C:\Работа с Win11\dism.log".to_string()));
        assert!(
            args.contains(&r"/PackagePath:D:\Обновления 24H2\Checkpoint\kb_ckpt.msu".to_string())
        );
        assert!(args.contains(&r"/PackagePath:D:\Обновления 24H2\Cumulative\kb_cu.msu".to_string()));
    }
}
