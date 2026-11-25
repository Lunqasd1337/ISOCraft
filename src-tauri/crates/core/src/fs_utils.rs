use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf, Prefix};
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns true if metadata describes a symlink or a Windows NTFS reparse point (junction/symlink).
pub fn is_symlink_or_reparse(meta: &fs::Metadata) -> bool {
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

/// Returns true if a single path segment (case-insensitive, stripping trailing spaces/dots and extensions)
/// resolves to a reserved Win32 DOS device name.
pub fn is_windows_reserved_name(segment: &str) -> bool {
    let trimmed = segment.trim_end_matches([' ', '.']);
    let stem = trimmed.split('.').next().unwrap_or("");
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Reads a file up to `max_bytes` using a streaming `Take` bound to avoid OOM on oversized files.
pub fn read_file_bounded(
    path: &Path,
    max_bytes: u64,
    reject_symlink: bool,
) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed inspecting file '{}': {e}", path.display()))?;

    if reject_symlink && is_symlink_or_reparse(&meta) {
        return Err(format!(
            "Symbolic links and reparse points are not allowed for '{}'.",
            path.display()
        ));
    }

    if !meta.is_file() {
        return Err(format!("Path '{}' is not a regular file.", path.display()));
    }

    if meta.len() > max_bytes {
        return Err(format!(
            "File '{}' ({} bytes) exceeds maximum allowed size of {} bytes.",
            path.display(),
            meta.len(),
            max_bytes
        ));
    }

    let file =
        File::open(path).map_err(|e| format!("Failed opening file '{}': {e}", path.display()))?;
    let mut buf = Vec::with_capacity(meta.len().min(max_bytes) as usize);
    let mut limited = file.take(max_bytes.saturating_add(1));
    limited
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed reading file '{}': {e}", path.display()))?;

    if buf.len() as u64 > max_bytes {
        return Err(format!(
            "File '{}' exceeds maximum allowed size of {} bytes.",
            path.display(),
            max_bytes
        ));
    }

    Ok(buf)
}

/// Copies a regular file in chunks, aborting promptly when `is_cancelled()` returns true
/// and removing any incomplete destination file on cancellation or write failure.
pub fn copy_file_cancellable<F>(src: &Path, dst: &Path, is_cancelled: F) -> Result<u64, String>
where
    F: Fn() -> bool,
{
    if is_cancelled() {
        return Err("Operation cancelled by user.".to_string());
    }

    let mut reader = File::open(src)
        .map_err(|e| format!("Failed opening source file '{}': {e}", src.display()))?;
    let mut writer = File::create(dst)
        .map_err(|e| format!("Failed creating destination file '{}': {e}", dst.display()))?;

    let mut buf = vec![0u8; 1024 * 1024];
    let mut total_written: u64 = 0;

    loop {
        if is_cancelled() {
            drop(writer);
            let _ = fs::remove_file(dst);
            return Err("Operation cancelled by user.".to_string());
        }

        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(writer);
                let _ = fs::remove_file(dst);
                return Err(format!("Failed reading '{}': {e}", src.display()));
            }
        };

        if let Err(e) = writer.write_all(&buf[..n]) {
            drop(writer);
            let _ = fs::remove_file(dst);
            return Err(format!("Failed writing '{}': {e}", dst.display()));
        }
        total_written += n as u64;
    }

    if let Err(e) = writer.flush() {
        drop(writer);
        let _ = fs::remove_file(dst);
        return Err(format!("Failed flushing '{}': {e}", dst.display()));
    }

    Ok(total_written)
}

fn strip_verbatim_for_compare(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
            return format!(r"\\{}", &rest[4..]);
        }
        return rest.to_string();
    }
    raw.to_string()
}

/// Lexically normalizes `.` / `..` segments and strips Windows `\\?\` verbatim prefixes
/// for reliable component-wise prefix comparisons.
pub fn normalize_path_for_prefix(path: &Path) -> PathBuf {
    let raw = strip_verbatim_for_compare(&path.to_string_lossy());
    let cleaned_input = PathBuf::from(raw);
    let mut out = PathBuf::new();

    for comp in cleaned_input.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if can_pop {
                    out.pop();
                } else if !out.has_root() {
                    out.push(comp.as_os_str());
                }
            }
            Component::Normal(s) => out.push(s),
        }
    }

    out
}

fn resolve_best_effort(path: &Path) -> PathBuf {
    if let Ok(canon) = fs::canonicalize(path) {
        return normalize_path_for_prefix(&canon);
    }

    let normalized = normalize_path_for_prefix(path);
    let mut existing = normalized.as_path();
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();

    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            return normalized;
        };
        tail.push(name);
        let Some(parent) = existing.parent() else {
            return normalized;
        };
        existing = parent;
    }

    let mut resolved = fs::canonicalize(existing)
        .map(|p| normalize_path_for_prefix(&p))
        .unwrap_or_else(|_| existing.to_path_buf());
    for seg in tail.into_iter().rev() {
        resolved.push(seg);
    }
    normalize_path_for_prefix(&resolved)
}

fn components_equal_ci(a: &Path, b: &Path) -> bool {
    let a_comps: Vec<String> = a
        .components()
        .map(|c| {
            let s = c.as_os_str().to_string_lossy();
            #[cfg(windows)]
            {
                s.to_ascii_lowercase()
            }
            #[cfg(not(windows))]
            {
                s.into_owned()
            }
        })
        .collect();
    let b_comps: Vec<String> = b
        .components()
        .map(|c| {
            let s = c.as_os_str().to_string_lossy();
            #[cfg(windows)]
            {
                s.to_ascii_lowercase()
            }
            #[cfg(not(windows))]
            {
                s.into_owned()
            }
        })
        .collect();

    if b_comps.is_empty() || a_comps.len() < b_comps.len() {
        return false;
    }
    a_comps[..b_comps.len()] == b_comps[..]
}

/// Returns true if `path` is a UNC network share path or a Windows device/namespace path.
pub fn is_unc_or_device_path(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    let trimmed = raw.trim();
    if trimmed.starts_with("//") {
        return true;
    }
    if let Some(Component::Prefix(p)) = path.components().next() {
        return matches!(
            p.kind(),
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::DeviceNS(..)
        );
    }
    if trimmed.starts_with(r"\\") {
        if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
            let bytes = rest.as_bytes();
            let is_verbatim_drive =
                bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
            return !is_verbatim_drive;
        }
        return true;
    }
    false
}

/// Atomically writes `data` to `path` via a sibling temporary file, fsync, and atomic replace,
/// rejecting symbolic links / reparse points on both the destination and its parent directory.
pub fn write_file_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    if is_unc_or_device_path(path) {
        return Err(format!(
            "Refusing atomic write to UNC or device path '{}'.",
            path.display()
        ));
    }

    if let Ok(meta) = fs::symlink_metadata(path)
        && is_symlink_or_reparse(&meta)
    {
        return Err(format!(
            "Refusing to overwrite symlink or reparse point at '{}'.",
            path.display()
        ));
    }

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if let Ok(pmeta) = fs::symlink_metadata(parent)
        && is_symlink_or_reparse(&pmeta)
    {
        return Err(format!(
            "Refusing to write inside symlink or reparse directory '{}'.",
            parent.display()
        ));
    }
    fs::create_dir_all(parent).map_err(|e| {
        format!(
            "Failed creating parent directory '{}': {e}",
            parent.display()
        )
    })?;

    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".{file_name}.{}.{nanos}.tmp", std::process::id()));

    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|e| {
                format!(
                    "Failed creating temporary file '{}': {e}",
                    tmp_path.display()
                )
            })?;
        if let Err(e) = file.write_all(data).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&tmp_path);
            return Err(format!(
                "Failed writing temporary file '{}': {e}",
                tmp_path.display()
            ));
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        use windows::core::PCWSTR;

        let src_w: Vec<u16> = tmp_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let dst_w: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let res = unsafe {
            MoveFileExW(
                PCWSTR(src_w.as_ptr()),
                PCWSTR(dst_w.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if let Err(e) = res {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("Failed replacing '{}': {e}", path.display()));
        }
    }

    #[cfg(not(windows))]
    {
        if let Err(e) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("Failed replacing '{}': {e}", path.display()));
        }
    }

    Ok(())
}

/// Returns true if `path` equals or is located inside `dir` (checking both lexical
/// normalized components and best-effort canonicalized ancestors).
pub fn is_path_within_dir(path: &Path, dir: &Path) -> bool {
    let lex_path = normalize_path_for_prefix(path);
    let lex_dir = normalize_path_for_prefix(dir);
    if components_equal_ci(&lex_path, &lex_dir) {
        return true;
    }

    let res_path = resolve_best_effort(path);
    let res_dir = resolve_best_effort(dir);
    components_equal_ci(&res_path, &res_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "isocraft_fs_utils_test_{label}_{}_{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reserved_names_match_case_and_trailing_dots() {
        assert!(is_windows_reserved_name("CON"));
        assert!(is_windows_reserved_name("con.txt"));
        assert!(is_windows_reserved_name("NUL.tar.gz "));
        assert!(is_windows_reserved_name("com1."));
        assert!(is_windows_reserved_name("LPT9.log.."));
        assert!(!is_windows_reserved_name("console.txt"));
        assert!(!is_windows_reserved_name("COM10"));
    }

    #[test]
    fn read_file_bounded_rejects_oversized_file() {
        let dir = make_temp_dir("bounded");
        let file = dir.join("sample.bin");
        fs::write(&file, vec![1u8; 64]).unwrap();

        assert!(read_file_bounded(&file, 64, true).is_ok());
        let err = read_file_bounded(&file, 63, true).unwrap_err();
        assert!(err.contains("exceeds maximum"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_file_cancellable_aborts_and_cleans_up_destination() {
        let dir = make_temp_dir("copy_cancel");
        let src = dir.join("src.bin");
        let dst = dir.join("dst.bin");
        fs::write(&src, vec![42u8; 16]).unwrap();

        let cancelled = AtomicBool::new(true);
        let err =
            copy_file_cancellable(&src, &dst, || cancelled.load(Ordering::Relaxed)).unwrap_err();
        assert!(err.contains("cancelled"));
        assert!(!dst.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_within_dir_respects_component_boundaries_and_parent_refs() {
        let base = Path::new(r"C:\ISOCraft\work\session_1");
        assert!(is_path_within_dir(
            Path::new(r"c:\isocraft\work\session_1\out.iso"),
            base
        ));
        assert!(is_path_within_dir(
            Path::new(r"C:\ISOCraft\work\session_1\sub\..\out.iso"),
            base
        ));
        assert!(!is_path_within_dir(
            Path::new(r"C:\ISOCraft\work\session_10\out.iso"),
            base
        ));
        assert!(!is_path_within_dir(
            Path::new(r"C:\ISOCraft\work\session_1\..\out.iso"),
            base
        ));
    }

    #[test]
    fn detects_unc_and_device_paths() {
        assert!(is_unc_or_device_path(Path::new(r"\\server\share\file.reg")));
        assert!(is_unc_or_device_path(Path::new(
            r"\\?\UNC\server\share\file.reg"
        )));
        assert!(is_unc_or_device_path(Path::new(r"\\.\PhysicalDrive0")));
        assert!(is_unc_or_device_path(Path::new("//server/share/file.reg")));

        assert!(!is_unc_or_device_path(Path::new(r"C:\ISOCraft\work")));
        assert!(!is_unc_or_device_path(Path::new(r"\\?\C:\ISOCraft\work")));
        assert!(!is_unc_or_device_path(Path::new(r"relative\path.txt")));
    }

    #[test]
    fn write_file_atomic_creates_and_overwrites_cleanly() {
        let dir = make_temp_dir("atomic_write");
        let target = dir.join("state.json");

        write_file_atomic(&target, b"first").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first");

        write_file_atomic(&target, b"second-version").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second-version");

        let leftover_tmps: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover_tmps.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
