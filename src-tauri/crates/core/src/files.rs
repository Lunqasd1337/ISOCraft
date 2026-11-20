use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileTarget {
    InstallWim,
    IsoRoot,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    #[default]
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileScanResult {
    pub path: String,
    pub is_directory: bool,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInjectionItem {
    pub source_path: String,
    pub is_directory: bool,
    pub target: FileTarget,
    pub destination_path: String,
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FilesBuildPlan {
    pub items: Vec<FileInjectionItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileInjectionReport {
    pub copied_files: usize,
    pub skipped_files: usize,
    pub created_dirs: usize,
}

fn clear_readonly_on_file(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

/// Normalizes a relative Windows destination directory path inside `mount_install` or `iso_root`.
/// Empty input (or `"/"` / `"\\"`) maps to the root (`PathBuf::new()`).
/// Rejects drive letters, UNC paths, traversal segments (`.`, `..`), empty internal segments, and invalid filename chars.
pub fn normalize_destination_dir(raw_dest: &str) -> Result<PathBuf, String> {
    let trimmed = raw_dest.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }

    let unified = trimmed.replace('/', "\\");
    if unified.starts_with("\\\\") {
        return Err("UNC network paths are not allowed as a destination path.".to_string());
    }
    if unified.contains(':') {
        return Err(
            "Drive letters or ':' characters are not allowed in destination path.".to_string(),
        );
    }

    for ch in unified.chars() {
        if matches!(ch, '<' | '>' | '"' | '|' | '?' | '*') || (ch as u32) <= 0x1F {
            return Err(format!(
                "Destination path contains invalid Windows path character: '{ch}'."
            ));
        }
    }

    let stripped = unified.trim_matches('\\');
    if stripped.is_empty() {
        return Ok(PathBuf::new());
    }

    let mut out = PathBuf::new();
    for seg in stripped.split('\\') {
        if seg.is_empty() {
            return Err("Destination path contains an empty path segment.".to_string());
        }
        let seg_trimmed = seg.trim_end_matches([' ', '.']);
        if seg == "." || seg == ".." || seg_trimmed.is_empty() {
            return Err(format!(
                "Path traversal or invalid segment '{seg}' is not allowed in destination path."
            ));
        }
        out.push(seg);
    }

    Ok(out)
}

fn rel_path_to_lower_backslash(rel_path: &Path) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_ascii_lowercase()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "Invalid non-relative component in destination path '{}'.",
                    rel_path.display()
                ));
            }
        }
    }
    Ok(parts.join("\\"))
}

/// Blocks critical system registry hives in `install.wim` and boot/install payload files in ISO root.
pub fn check_protected_relative_path(target: &FileTarget, rel_path: &Path) -> Result<(), String> {
    let norm = rel_path_to_lower_backslash(rel_path)?;
    if norm.is_empty() {
        return Ok(());
    }

    match target {
        FileTarget::InstallWim => {
            if norm == "windows\\system32\\config"
                || norm.starts_with("windows\\system32\\config\\")
            {
                return Err(format!(
                    "Protected system path cannot be modified via file copy: '{}'. Use the Registry screen instead.",
                    rel_path.display()
                ));
            }
            if norm == "users\\default\\ntuser.dat"
                || norm.starts_with("users\\default\\ntuser.dat\\")
            {
                return Err(format!(
                    "Protected Default user hive cannot be modified via file copy: '{}'. Use the Registry screen instead.",
                    rel_path.display()
                ));
            }
        }
        FileTarget::IsoRoot => {
            const PROTECTED_ISO_FILES: [&str; 8] = [
                "sources\\install.wim",
                "sources\\install.esd",
                "sources\\boot.wim",
                "boot\\etfsboot.com",
                "efi\\microsoft\\boot\\efisys.bin",
                "efi\\microsoft\\boot\\efisys_noprompt.bin",
                "bootmgr",
                "bootmgr.efi",
            ];
            if PROTECTED_ISO_FILES.contains(&norm.as_str()) {
                return Err(format!(
                    "Protected ISO boot/image file cannot be overwritten: '{}'.",
                    rel_path.display()
                ));
            }
            if let Some(file_part) = norm.strip_prefix("sources\\") {
                if !file_part.contains('\\')
                    && file_part.starts_with("install")
                    && file_part.ends_with(".swm")
                {
                    return Err(format!(
                        "Protected ISO split image file cannot be overwritten: '{}'.",
                        rel_path.display()
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Validates a queued file/folder injection item prior to mounting images.
/// Returns the normalized relative destination path (`<destination_path>\<source_name>`).
pub fn validate_file_injection_item(item: &FileInjectionItem) -> Result<PathBuf, String> {
    let src = Path::new(&item.source_path);
    if item.source_path.trim().is_empty() || !src.exists() {
        return Err(format!(
            "Custom file or folder does not exist: '{}'",
            item.source_path
        ));
    }

    if item.is_directory && !src.is_dir() {
        return Err(format!(
            "Expected a directory for custom file rule, got file: '{}'",
            item.source_path
        ));
    }
    if !item.is_directory && !src.is_file() {
        return Err(format!(
            "Expected a file for custom file rule, got directory: '{}'",
            item.source_path
        ));
    }

    let entry_name = src.file_name().ok_or_else(|| {
        format!(
            "Source path has no valid file or folder name: '{}'",
            item.source_path
        )
    })?;

    let dest_dir = normalize_destination_dir(&item.destination_path)?;
    let full_rel = if dest_dir.as_os_str().is_empty() {
        PathBuf::from(entry_name)
    } else {
        dest_dir.join(entry_name)
    };

    check_protected_relative_path(&item.target, &full_rel)?;

    if item.is_directory {
        let mut stack: Vec<(PathBuf, PathBuf)> = vec![(src.to_path_buf(), full_rel.clone())];
        while let Some((cur_src, cur_rel)) = stack.pop() {
            let entries = fs::read_dir(&cur_src).map_err(|e| {
                format!(
                    "Failed reading custom directory '{}': {e}",
                    cur_src.display()
                )
            })?;
            for entry in entries {
                let entry = entry.map_err(|e| {
                    format!(
                        "Failed iterating custom directory '{}': {e}",
                        cur_src.display()
                    )
                })?;
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_symlink() {
                    continue;
                }
                let child_rel = cur_rel.join(entry.file_name());
                check_protected_relative_path(&item.target, &child_rel)?;
                if ft.is_dir() {
                    stack.push((entry.path(), child_rel));
                }
            }
        }
    }

    Ok(full_rel)
}

/// Scans a user-chosen file or folder to inspect file count and total size.
pub fn scan_custom_file_path(path: &Path) -> Result<FileScanResult, String> {
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }

    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed inspecting '{}': {e}", path.display()))?;

    if meta.file_type().is_symlink() {
        return Err(format!(
            "Symbolic links are not supported as top-level custom sources: '{}'",
            path.display()
        ));
    }

    if path.is_file() {
        return Ok(FileScanResult {
            path: path.to_string_lossy().to_string(),
            is_directory: false,
            file_count: 1,
            total_bytes: meta.len(),
        });
    }

    if path.is_dir() {
        let mut file_count = 0usize;
        let mut total_bytes = 0u64;
        let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let entries = fs::read_dir(&dir)
                .map_err(|e| format!("Failed reading folder '{}': {e}", dir.display()))?;
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    stack.push(entry.path());
                } else if ft.is_file() {
                    file_count += 1;
                    total_bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }

        return Ok(FileScanResult {
            path: path.to_string_lossy().to_string(),
            is_directory: true,
            file_count,
            total_bytes,
        });
    }

    Err(format!("Unsupported path type: {}", path.display()))
}

/// Copies all queued custom files/folders matching `target_kind` into `target_root` (`mount_install` or `iso_root`).
pub fn apply_file_injections<F, C>(
    target_root: &Path,
    target_kind: FileTarget,
    items: &[FileInjectionItem],
    is_cancelled: C,
    mut on_log: F,
) -> Result<FileInjectionReport, String>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    let mut report = FileInjectionReport::default();

    for item in items.iter().filter(|i| i.target == target_kind) {
        if is_cancelled() {
            return Err("Operation cancelled by user.".to_string());
        }

        let rel_base = validate_file_injection_item(item)?;
        let src = Path::new(&item.source_path);

        if !item.is_directory {
            let dest_file = target_root.join(&rel_base);
            if let Some(parent) = dest_file.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| {
                        format!(
                            "Failed creating destination directory '{}': {e}",
                            parent.display()
                        )
                    })?;
                    report.created_dirs += 1;
                }
            }

            if dest_file.exists() && item.conflict_policy == ConflictPolicy::Skip {
                report.skipped_files += 1;
                on_log(&format!(
                    "Skipped existing file '{}' -> '{}'",
                    src.display(),
                    rel_base.display()
                ));
                continue;
            }

            if dest_file.exists() {
                clear_readonly_on_file(&dest_file);
            }
            fs::copy(src, &dest_file).map_err(|e| {
                format!(
                    "Failed copying '{}' -> '{}': {e}",
                    src.display(),
                    dest_file.display()
                )
            })?;
            clear_readonly_on_file(&dest_file);
            report.copied_files += 1;
            on_log(&format!(
                "Copied file '{}' -> '{}'",
                src.display(),
                rel_base.display()
            ));
        } else {
            let dest_root = target_root.join(&rel_base);
            if !dest_root.exists() {
                fs::create_dir_all(&dest_root).map_err(|e| {
                    format!(
                        "Failed creating destination directory '{}': {e}",
                        dest_root.display()
                    )
                })?;
                report.created_dirs += 1;
            }

            let mut dir_copied = 0usize;
            let mut dir_skipped = 0usize;
            let mut stack: Vec<(PathBuf, PathBuf)> = vec![(src.to_path_buf(), rel_base.clone())];

            while let Some((cur_src, cur_rel)) = stack.pop() {
                let entries = fs::read_dir(&cur_src).map_err(|e| {
                    format!("Failed reading source folder '{}': {e}", cur_src.display())
                })?;

                for entry in entries {
                    if is_cancelled() {
                        return Err("Operation cancelled by user.".to_string());
                    }

                    let entry = entry.map_err(|e| {
                        format!(
                            "Failed iterating source folder '{}': {e}",
                            cur_src.display()
                        )
                    })?;
                    let Ok(ft) = entry.file_type() else {
                        continue;
                    };
                    if ft.is_symlink() {
                        continue;
                    }

                    let child_src = entry.path();
                    let child_rel = cur_rel.join(entry.file_name());
                    check_protected_relative_path(&target_kind, &child_rel)?;

                    let child_dest = target_root.join(&child_rel);
                    if ft.is_dir() {
                        if !child_dest.exists() {
                            fs::create_dir_all(&child_dest).map_err(|e| {
                                format!(
                                    "Failed creating destination directory '{}': {e}",
                                    child_dest.display()
                                )
                            })?;
                            report.created_dirs += 1;
                        }
                        stack.push((child_src, child_rel));
                    } else if ft.is_file() {
                        if child_dest.exists() && item.conflict_policy == ConflictPolicy::Skip {
                            dir_skipped += 1;
                            report.skipped_files += 1;
                            continue;
                        }
                        if child_dest.exists() {
                            clear_readonly_on_file(&child_dest);
                        }
                        fs::copy(&child_src, &child_dest).map_err(|e| {
                            format!(
                                "Failed copying '{}' -> '{}': {e}",
                                child_src.display(),
                                child_dest.display()
                            )
                        })?;
                        clear_readonly_on_file(&child_dest);
                        dir_copied += 1;
                        report.copied_files += 1;
                    }
                }
            }

            on_log(&format!(
                "Copied folder '{}' -> '{}' (copied={}, skipped={})",
                src.display(),
                rel_base.display(),
                dir_copied,
                dir_skipped
            ));
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}_{}_{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn normalizes_and_validates_destination_paths() {
        assert_eq!(normalize_destination_dir("").unwrap(), PathBuf::new());
        assert_eq!(normalize_destination_dir("  /  ").unwrap(), PathBuf::new());
        assert_eq!(normalize_destination_dir("\\").unwrap(), PathBuf::new());
        assert_eq!(
            normalize_destination_dir("/Windows/Setup/Scripts/").unwrap(),
            PathBuf::from(r"Windows\Setup\Scripts")
        );
        assert_eq!(
            normalize_destination_dir(r"sources\$OEM$").unwrap(),
            PathBuf::from(r"sources\$OEM$")
        );

        assert!(normalize_destination_dir(r"..\Windows").is_err());
        assert!(normalize_destination_dir(r"Windows\..\System32").is_err());
        assert!(normalize_destination_dir(r"C:\Tools").is_err());
        assert!(normalize_destination_dir(r"\\server\share").is_err());
        assert!(normalize_destination_dir(r"Tools\\Sub").is_err());
        assert!(normalize_destination_dir(r"Tools\bad*name").is_err());
    }

    #[test]
    fn blocks_protected_system_and_iso_paths() {
        assert!(check_protected_relative_path(
            &FileTarget::InstallWim,
            Path::new(r"Windows\System32\config")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::InstallWim,
            Path::new(r"windows/system32/config/SOFTWARE")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::InstallWim,
            Path::new(r"Users\Default\NTUSER.DAT")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::InstallWim,
            Path::new(r"Windows\Setup\Scripts\SetupComplete.cmd")
        )
        .is_ok());

        assert!(check_protected_relative_path(
            &FileTarget::IsoRoot,
            Path::new(r"sources\install.wim")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::IsoRoot,
            Path::new(r"sources\install2.swm")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::IsoRoot,
            Path::new(r"sources\boot.wim")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::IsoRoot,
            Path::new(r"boot\etfsboot.com")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::IsoRoot,
            Path::new(r"efi\microsoft\boot\efisys_noprompt.bin")
        )
        .is_err());
        assert!(check_protected_relative_path(
            &FileTarget::IsoRoot,
            Path::new(r"sources\$OEM$\$$\Setup\Scripts\setup.cmd")
        )
        .is_ok());
    }

    #[test]
    fn scans_custom_file_and_nested_folder() {
        let base = make_temp_dir("isocraft_files_scan");
        let folder = base.join("Tools");
        let sub = folder.join("sub");
        fs::create_dir_all(&sub).unwrap();

        let f1 = folder.join("run.cmd");
        let f2 = sub.join("readme.txt");
        fs::write(&f1, b"echo ok").unwrap();
        fs::write(&f2, b"hello").unwrap();

        let file_scan = scan_custom_file_path(&f1).unwrap();
        assert!(!file_scan.is_directory);
        assert_eq!(file_scan.file_count, 1);
        assert_eq!(file_scan.total_bytes, 7);

        let dir_scan = scan_custom_file_path(&folder).unwrap();
        assert!(dir_scan.is_directory);
        assert_eq!(dir_scan.file_count, 2);
        assert_eq!(dir_scan.total_bytes, 12);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn applies_file_and_folder_injections_with_overwrite_and_skip() {
        let base = make_temp_dir("isocraft_files_apply");
        let src_dir = base.join("src");
        let bundle = src_dir.join("MyApp");
        fs::create_dir_all(bundle.join("bin")).unwrap();

        let script = src_dir.join("SetupComplete.cmd");
        fs::write(&script, b"@echo new").unwrap();
        fs::write(bundle.join("bin").join("app.exe"), b"exe-bytes").unwrap();

        let mount = base.join("mount");
        let iso = base.join("iso");
        fs::create_dir_all(mount.join(r"Windows\Setup\Scripts")).unwrap();
        fs::create_dir_all(iso.join("sources")).unwrap();

        // Pre-populate read-only target files to verify Overwrite & Skip
        let existing_script = mount.join(r"Windows\Setup\Scripts\SetupComplete.cmd");
        fs::write(&existing_script, b"@echo old").unwrap();
        let mut ro_perms = fs::metadata(&existing_script).unwrap().permissions();
        ro_perms.set_readonly(true);
        fs::set_permissions(&existing_script, ro_perms).unwrap();

        let skip_file = src_dir.join("keep.txt");
        fs::write(&skip_file, b"from-src").unwrap();
        let existing_skip = mount.join(r"Tools\keep.txt");
        fs::create_dir_all(existing_skip.parent().unwrap()).unwrap();
        fs::write(&existing_skip, b"original-keep").unwrap();

        let items = vec![
            FileInjectionItem {
                source_path: script.to_string_lossy().to_string(),
                is_directory: false,
                target: FileTarget::InstallWim,
                destination_path: r"Windows\Setup\Scripts".to_string(),
                conflict_policy: ConflictPolicy::Overwrite,
            },
            FileInjectionItem {
                source_path: skip_file.to_string_lossy().to_string(),
                is_directory: false,
                target: FileTarget::InstallWim,
                destination_path: "Tools".to_string(),
                conflict_policy: ConflictPolicy::Skip,
            },
            FileInjectionItem {
                source_path: bundle.to_string_lossy().to_string(),
                is_directory: true,
                target: FileTarget::IsoRoot,
                destination_path: "Soft".to_string(),
                conflict_policy: ConflictPolicy::Overwrite,
            },
        ];

        let mut logs = Vec::new();
        let rep_wim = apply_file_injections(
            &mount,
            FileTarget::InstallWim,
            &items,
            || false,
            |m| logs.push(m.to_string()),
        )
        .unwrap();

        assert_eq!(rep_wim.copied_files, 1);
        assert_eq!(rep_wim.skipped_files, 1);
        assert_eq!(fs::read(&existing_script).unwrap(), b"@echo new");
        assert_eq!(fs::read(&existing_skip).unwrap(), b"original-keep");

        let rep_iso = apply_file_injections(
            &iso,
            FileTarget::IsoRoot,
            &items,
            || false,
            |m| logs.push(m.to_string()),
        )
        .unwrap();
        assert_eq!(rep_iso.copied_files, 1);
        assert_eq!(
            fs::read(iso.join(r"Soft\MyApp\bin\app.exe")).unwrap(),
            b"exe-bytes"
        );

        // Verify a folder named `sources` containing `install.wim` at ISO root is blocked
        let evil_sources = src_dir.join("sources");
        fs::create_dir_all(&evil_sources).unwrap();
        fs::write(evil_sources.join("install.wim"), b"fake").unwrap();
        let blocked_item = FileInjectionItem {
            source_path: evil_sources.to_string_lossy().to_string(),
            is_directory: true,
            target: FileTarget::IsoRoot,
            destination_path: "".to_string(),
            conflict_policy: ConflictPolicy::Overwrite,
        };
        assert!(validate_file_injection_item(&blocked_item).is_err());
        assert!(apply_file_injections(
            &iso,
            FileTarget::IsoRoot,
            &[blocked_item],
            || false,
            |_| {}
        )
        .is_err());

        let _ = fs::remove_dir_all(&base);
    }
}
