use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default chunk buffer size (1 MiB) used by cancellable file copy helpers.
pub const DEFAULT_COPY_BUFFER_SIZE: usize = 1024 * 1024;

static ATOMIC_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    let meta = if reject_symlink {
        let sym_meta = fs::symlink_metadata(path)
            .map_err(|e| format!("Failed inspecting file '{}': {e}", path.display()))?;
        if is_symlink_or_reparse(&sym_meta) {
            return Err(format!(
                "Symbolic links and reparse points are not allowed for '{}'.",
                path.display()
            ));
        }
        sym_meta
    } else {
        fs::metadata(path)
            .map_err(|e| format!("Failed inspecting file '{}': {e}", path.display()))?
    };

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

/// Copies a regular file in chunks using a caller-provided buffer, aborting promptly when
/// `is_cancelled()` returns true, rejecting symlinks/reparse points at `dst`, and removing
/// any incomplete destination file on cancellation or write failure.
pub fn copy_file_cancellable_with_buf<F>(
    src: &Path,
    dst: &Path,
    buf: &mut [u8],
    is_cancelled: F,
) -> Result<u64, String>
where
    F: Fn() -> bool,
{
    if is_cancelled() {
        return Err("Operation cancelled by user.".to_string());
    }

    if let Ok(dst_meta) = fs::symlink_metadata(dst)
        && is_symlink_or_reparse(&dst_meta)
    {
        return Err(format!(
            "Refusing to overwrite symlink or reparse point at '{}'.",
            dst.display()
        ));
    }

    let mut reader = File::open(src)
        .map_err(|e| format!("Failed opening source file '{}': {e}", src.display()))?;
    let mut writer = File::create(dst)
        .map_err(|e| format!("Failed creating destination file '{}': {e}", dst.display()))?;

    let mut fallback = Vec::new();
    let chunk_buf: &mut [u8] = if buf.is_empty() {
        fallback.resize(8192, 0);
        &mut fallback
    } else {
        buf
    };
    let mut total_written: u64 = 0;

    loop {
        if is_cancelled() {
            drop(writer);
            let _ = fs::remove_file(dst);
            return Err("Operation cancelled by user.".to_string());
        }

        let n = match reader.read(chunk_buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(writer);
                let _ = fs::remove_file(dst);
                return Err(format!("Failed reading '{}': {e}", src.display()));
            }
        };

        if let Err(e) = writer.write_all(&chunk_buf[..n]) {
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

/// Copies a regular file in chunks, aborting promptly when `is_cancelled()` returns true
/// and removing any incomplete destination file on cancellation or write failure.
pub fn copy_file_cancellable<F>(src: &Path, dst: &Path, is_cancelled: F) -> Result<u64, String>
where
    F: Fn() -> bool,
{
    let mut buf = vec![0u8; DEFAULT_COPY_BUFFER_SIZE];
    copy_file_cancellable_with_buf(src, dst, &mut buf, is_cancelled)
}

fn strip_verbatim_str(raw: &str, max_utf16_len: Option<usize>) -> Option<String> {
    let rest = raw.strip_prefix(r"\\?\")?;
    if let Some(unc_prefix) = rest.get(..4)
        && unc_prefix.eq_ignore_ascii_case(r"UNC\")
    {
        let unc = format!(r"\\{}", &rest[4..]);
        if max_utf16_len.is_none_or(|limit| unc.encode_utf16().count() < limit) {
            return Some(unc);
        }
        return None;
    }

    let bytes = rest.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && max_utf16_len.is_none_or(|limit| rest.encode_utf16().count() < limit)
    {
        return Some(rest.to_string());
    }

    None
}

/// Strips the Windows `\\?\` verbatim path prefix when the resulting path is a
/// standard drive or UNC path shorter than the legacy MAX_PATH limit (260 UTF-16
/// code units), keeping device paths and long paths intact.
pub fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    const MAX_PATH_UTF16: usize = 260;
    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };
    match strip_verbatim_str(raw, Some(MAX_PATH_UTF16)) {
        Some(stripped) => PathBuf::from(stripped),
        None => path.to_path_buf(),
    }
}

fn strip_verbatim_for_compare(raw: &str) -> String {
    strip_verbatim_str(raw, None).unwrap_or_else(|| raw.to_string())
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

fn resolve_strict_path(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() || is_unc_or_device_path(path) {
        return None;
    }

    // Try canonicalize directly first: active WIM mounts carry IO_REPARSE_TAG_WIM and resolve to
    // their own workspace path, whereas NTFS junctions resolve to their real target outside.
    if let Ok(canon) = fs::canonicalize(path) {
        return Some(normalize_path_for_prefix(&canon));
    }

    let normalized = normalize_path_for_prefix(path);
    let mut existing = normalized.as_path();
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();

    loop {
        match fs::symlink_metadata(existing) {
            Ok(meta) => {
                if is_symlink_or_reparse(&meta) {
                    return None;
                }
                if !tail.is_empty() && !meta.is_dir() {
                    return None;
                }
                let canon = fs::canonicalize(existing).ok()?;
                let mut resolved = normalize_path_for_prefix(&canon);
                for seg in tail.into_iter().rev() {
                    resolved.push(seg);
                }
                return Some(normalize_path_for_prefix(&resolved));
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let name = existing.file_name()?;
                tail.push(name);
                let parent = existing.parent()?;
                if parent.as_os_str().is_empty() {
                    return None;
                }
                existing = parent;
            }
            Err(_) => return None,
        }
    }
}

fn normalize_component_for_compare(c: Component<'_>) -> String {
    let s = c.as_os_str().to_string_lossy();
    #[cfg(windows)]
    {
        s.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        s.into_owned()
    }
}

fn components_equal_ci(a: &Path, b: &Path) -> bool {
    let a_comps: Vec<String> = a
        .components()
        .map(normalize_component_for_compare)
        .collect();
    let b_comps: Vec<String> = b
        .components()
        .map(normalize_component_for_compare)
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
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'\\' | b'/') && matches!(bytes[1], b'\\' | b'/') {
        if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
            let rest_bytes = rest.as_bytes();
            let is_verbatim_drive = rest_bytes.len() >= 3
                && rest_bytes[0].is_ascii_alphabetic()
                && rest_bytes[1] == b':'
                && rest_bytes[2] == b'\\';
            if !is_verbatim_drive {
                return true;
            }
        } else {
            return true;
        }
    }
    if let Some(Component::Prefix(p)) = Path::new(trimmed).components().next() {
        return !matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_));
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
    let seq = ATOMIC_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = parent.join(format!(
        ".{file_name}.{}.{nanos}.{seq}.tmp",
        std::process::id()
    ));

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
        use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};
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

        const RETRY_DELAYS_MS: [u64; 4] = [10, 20, 40, 80];
        let mut attempt = 0usize;
        loop {
            let res = unsafe {
                MoveFileExW(
                    PCWSTR(src_w.as_ptr()),
                    PCWSTR(dst_w.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            match res {
                Ok(()) => break,
                Err(e) => {
                    let code = e.code();
                    let retryable = code == ERROR_SHARING_VIOLATION.to_hresult()
                        || code == ERROR_ACCESS_DENIED.to_hresult();
                    if retryable && attempt < RETRY_DELAYS_MS.len() {
                        std::thread::sleep(std::time::Duration::from_millis(
                            RETRY_DELAYS_MS[attempt],
                        ));
                        attempt += 1;
                        continue;
                    }
                    let _ = fs::remove_file(&tmp_path);
                    return Err(format!("Failed replacing '{}': {e}", path.display()));
                }
            }
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

/// Strictly checks whether `path` resides inside (or equals) `dir` on the real filesystem,
/// resolving symlinks/NTFS junctions and blocking broken reparse points or non-existent tails
/// underneath symlinks/junctions. Use for allow-checks (security containment).
pub fn is_path_strictly_within_dir(path: &Path, dir: &Path) -> bool {
    let (Some(res_path), Some(res_dir)) = (resolve_strict_path(path), resolve_strict_path(dir))
    else {
        return false;
    };
    components_equal_ci(&res_path, &res_dir)
}

/// Conservatively checks whether `path` overlaps `dir` either lexically or after best-effort
/// canonicalization. Use for deny-checks (e.g. rejecting paths inside `session_current` or Windows dir).
pub fn is_path_overlapping_dir(path: &Path, dir: &Path) -> bool {
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
    fn read_file_bounded_handles_symlinks_when_permitted() {
        let dir = make_temp_dir("bounded_symlink");
        let target = dir.join("target.bin");
        let link = dir.join("link.bin");
        fs::write(&target, vec![7u8; 32]).unwrap();

        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        #[cfg(not(windows))]
        let created = std::os::unix::fs::symlink(&target, &link).is_ok();

        if created {
            assert!(read_file_bounded(&link, 64, true).is_err());
            let data = read_file_bounded(&link, 32, false).unwrap();
            assert_eq!(data, vec![7u8; 32]);
            let err = read_file_bounded(&link, 31, false).unwrap_err();
            assert!(err.contains("exceeds maximum"));
        }

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
    fn copy_file_cancellable_with_buf_reuses_buffer_handles_empty_and_rejects_dst_reparse() {
        let dir = make_temp_dir("copy_buf");
        let src1 = dir.join("src1.bin");
        let src2 = dir.join("src2.bin");
        let dst1 = dir.join("dst1.bin");
        let dst2 = dir.join("dst2.bin");
        let dst3 = dir.join("dst3.bin");
        fs::write(&src1, vec![1u8; 128]).unwrap();
        fs::write(&src2, vec![2u8; 10_000]).unwrap();

        let mut buf = vec![0u8; 64];
        assert_eq!(
            copy_file_cancellable_with_buf(&src1, &dst1, &mut buf, || false).unwrap(),
            128
        );
        assert_eq!(
            copy_file_cancellable_with_buf(&src2, &dst2, &mut buf, || false).unwrap(),
            10_000
        );
        assert_eq!(
            copy_file_cancellable_with_buf(&src1, &dst3, &mut [], || false).unwrap(),
            128
        );
        assert_eq!(fs::read(&dst1).unwrap(), vec![1u8; 128]);
        assert_eq!(fs::read(&dst2).unwrap(), vec![2u8; 10_000]);
        assert_eq!(fs::read(&dst3).unwrap(), vec![1u8; 128]);

        #[cfg(windows)]
        {
            let target_dir = dir.join("real_target_dir");
            let dst_junction = dir.join("dst_junction");
            fs::create_dir_all(&target_dir).unwrap();
            let mklink_ok = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    &dst_junction.to_string_lossy(),
                    &target_dir.to_string_lossy(),
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if mklink_ok {
                let err = copy_file_cancellable_with_buf(&src1, &dst_junction, &mut buf, || false)
                    .unwrap_err();
                assert!(err.contains("symlink or reparse point"));
                let _ = fs::remove_dir(&dst_junction);
            }
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn strip_verbatim_and_normalize_handles_utf8_and_long_paths() {
        // Multi-byte UTF-8 right at byte index 4 must never panic
        assert_eq!(
            normalize_path_for_prefix(Path::new(r"\\?\aбв")),
            PathBuf::from(r"\\?\aбв")
        );
        assert_eq!(
            normalize_path_for_prefix(Path::new(r"\\?\Кириллица\файл")),
            PathBuf::from(r"\\?\Кириллица\файл")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\aбв")),
            PathBuf::from(r"\\?\aбв")
        );

        // Device paths stay untouched
        assert_eq!(
            normalize_path_for_prefix(Path::new(r"\\?\Volume{1234}\foo")),
            PathBuf::from(r"\\?\Volume{1234}\foo")
        );
        assert_eq!(
            normalize_path_for_prefix(Path::new(r"\\?\GLOBALROOT\Device\HarddiskVolume1")),
            PathBuf::from(r"\\?\GLOBALROOT\Device\HarddiskVolume1")
        );

        // Long paths (>= 260 UTF-16) keep \\?\ in strip_windows_verbatim_prefix,
        // but drop \\?\ in normalize_path_for_prefix for canonical comparison
        let long_tail = "a".repeat(260);
        let long_verbatim = format!(r"\\?\C:\{long_tail}");
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(&long_verbatim)),
            PathBuf::from(&long_verbatim)
        );
        assert_eq!(
            normalize_path_for_prefix(Path::new(&long_verbatim)),
            PathBuf::from(format!(r"C:\{long_tail}"))
        );
    }

    #[test]
    fn path_overlapping_dir_respects_boundaries_parent_refs_and_unicode_case() {
        let base = Path::new(r"C:\ISOCraft\work\session_1");
        assert!(is_path_overlapping_dir(
            Path::new(r"c:\isocraft\work\session_1\out.iso"),
            base
        ));
        assert!(is_path_overlapping_dir(
            Path::new(r"C:\ISOCraft\work\session_1\sub\..\out.iso"),
            base
        ));
        assert!(!is_path_overlapping_dir(
            Path::new(r"C:\ISOCraft\work\session_10\out.iso"),
            base
        ));
        assert!(!is_path_overlapping_dir(
            Path::new(r"C:\ISOCraft\work\session_1\..\out.iso"),
            base
        ));

        #[cfg(windows)]
        {
            let cyr_base = Path::new(r"C:\Папка\Вложенная");
            assert!(is_path_overlapping_dir(
                Path::new(r"c:\папка\вложенная\Файл.iso"),
                cyr_base
            ));
        }
    }

    #[test]
    fn path_strictly_within_dir_blocks_junctions_and_checks_unicode_ancestors() {
        let dir = make_temp_dir("strict_within");
        let safe_dir = dir.join("safe_Папка");
        let outside_dir = dir.join("outside_dir");
        fs::create_dir_all(&safe_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        let outside_file = outside_dir.join("target.exe");
        fs::write(&outside_file, b"mz").unwrap();

        // Non-existent nested path inside an existing regular dir is strictly within
        assert!(is_path_strictly_within_dir(
            &safe_dir.join("sub").join("future.exe"),
            &safe_dir
        ));
        assert!(!is_path_strictly_within_dir(
            &safe_dir.join("..").join("outside_dir").join("target.exe"),
            &safe_dir
        ));

        #[cfg(windows)]
        {
            // Unicode case-insensitive match on existing directory
            let upper_cyr = dir.join("SAFE_ПАПКА").join("future.exe");
            assert!(is_path_strictly_within_dir(&upper_cyr, &safe_dir));

            // Junction inside safe_dir pointing to outside_dir must fail strict check
            // (both for existing and non-existent targets) while overlapping catches lexical match
            let junction = safe_dir.join("escape_link");
            let mklink_ok = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    &junction.to_string_lossy(),
                    &outside_dir.to_string_lossy(),
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            if mklink_ok {
                let via_junction_existing = junction.join("target.exe");
                assert!(!is_path_strictly_within_dir(
                    &via_junction_existing,
                    &safe_dir
                ));
                assert!(is_path_overlapping_dir(&via_junction_existing, &safe_dir));

                let via_junction_future = junction.join("nested").join("future.exe");
                assert!(!is_path_strictly_within_dir(
                    &via_junction_future,
                    &safe_dir
                ));
                assert!(is_path_overlapping_dir(&via_junction_future, &safe_dir));

                // Broken junction (delete outside_dir while junction still exists)
                let _ = fs::remove_dir_all(&outside_dir);
                assert!(!is_path_strictly_within_dir(&junction, &safe_dir));
                assert!(!is_path_strictly_within_dir(
                    &via_junction_future,
                    &safe_dir
                ));

                let _ = fs::remove_dir(&junction);
            }
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_unc_and_device_paths() {
        assert!(is_unc_or_device_path(Path::new(r"\\server\share\file.reg")));
        assert!(is_unc_or_device_path(Path::new(
            r"\\?\UNC\server\share\file.reg"
        )));
        assert!(is_unc_or_device_path(Path::new(r"\\.\PhysicalDrive0")));
        assert!(is_unc_or_device_path(Path::new("//server/share/file.reg")));
        assert!(is_unc_or_device_path(Path::new(
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\foo"
        )));
        assert!(is_unc_or_device_path(Path::new(r"\\?\Volume{1234}\foo")));
        assert!(is_unc_or_device_path(Path::new(r"\\?\PhysicalDrive0")));
        assert!(is_unc_or_device_path(Path::new(r"\\?\C:")));
        assert!(is_unc_or_device_path(Path::new(r"\\?\C:/ISOCraft")));
        assert!(is_unc_or_device_path(Path::new(r"/\server\share\a")));
        assert!(is_unc_or_device_path(Path::new(r"\/server/share/a")));
        assert!(is_unc_or_device_path(Path::new("//./PhysicalDrive0")));
        assert!(is_unc_or_device_path(Path::new(r"  \\?\GLOBALROOT\x  ")));

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

    #[test]
    fn write_file_atomic_handles_concurrent_writes_and_sharing_lock() {
        let dir = make_temp_dir("atomic_concurrent");
        let target = dir.join("shared.json");
        write_file_atomic(&target, b"init").unwrap();

        let mut handles = Vec::new();
        for tid in 0..4 {
            let p = target.clone();
            handles.push(std::thread::spawn(move || {
                for iter in 0..10 {
                    let payload = format!("t{tid}-{iter}");
                    write_file_atomic(&p, payload.as_bytes()).unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert!(target.exists());

        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;

            let locked = OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&target)
                .unwrap();
            let locker = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(30));
                drop(locked);
            });

            write_file_atomic(&target, b"after-lock").unwrap();
            locker.join().unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"after-lock");
        }

        let leftover_tmps: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover_tmps.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
