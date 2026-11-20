use crate::settings::AppSettings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub found: bool,
    pub path: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentStatus {
    pub is_admin: bool,
    pub dism: ToolStatus,
    pub wimlib: ToolStatus,
    pub xorriso: ToolStatus,
    pub oscdimg: ToolStatus,
    pub workspace_dir: String,
    pub free_space_bytes: u64,
    pub free_space_gb: f64,
    pub low_space_warning: bool,
    pub has_dirty_workspace: bool,
    pub dirty_workspace_details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    pub active_mounts: Vec<String>,
    pub loaded_hives: Vec<String>,
    pub updated_at: String,
}

pub fn check_environment_status(
    resource_dir: Option<&Path>,
    settings: &AppSettings,
) -> EnvironmentStatus {
    let workspace_path = PathBuf::from(&settings.workspace_dir);
    let (free_space_bytes, free_space_gb) = query_free_space(&workspace_path);
    let low_space_warning = free_space_gb > 0.0 && free_space_gb < 40.0;

    let state_file = workspace_path.join("workspace_state.json");
    let (has_dirty_workspace, dirty_workspace_details) = if state_file.exists() {
        match std::fs::read_to_string(&state_file) {
            Ok(raw) => match serde_json::from_str::<WorkspaceState>(&raw) {
                Ok(ws) if !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() => (
                    true,
                    Some(format!(
                        "mounts: {}, hives: {}",
                        ws.active_mounts.len(),
                        ws.loaded_hives.len()
                    )),
                ),
                _ => (false, None),
            },
            Err(_) => (false, None),
        }
    } else {
        (false, None)
    };

    EnvironmentStatus {
        is_admin: is_process_elevated(),
        dism: find_dism(),
        wimlib: find_bundled_or_path(
            resource_dir,
            &["wimlib/wimlib-imagex.exe", "wimlib-imagex.exe"],
            "wimlib-imagex.exe",
        ),
        xorriso: find_bundled_or_path(
            resource_dir,
            &["xorriso/xorriso.exe", "xorriso.exe"],
            "xorriso.exe",
        ),
        oscdimg: find_oscdimg(settings.oscdimg_path.as_deref()),
        workspace_dir: settings.workspace_dir.clone(),
        free_space_bytes,
        free_space_gb,
        low_space_warning,
        has_dirty_workspace,
        dirty_workspace_details,
    }
}

fn windows_root_dir() -> PathBuf {
    std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

fn find_system_binary(sys_exe: PathBuf, exe_name: &str) -> ToolStatus {
    if sys_exe.is_file() {
        return ToolStatus {
            found: true,
            path: Some(sys_exe.to_string_lossy().to_string()),
            source: "system".to_string(),
        };
    }
    #[cfg(debug_assertions)]
    if let Some(p) = search_path(exe_name) {
        return ToolStatus {
            found: true,
            path: Some(p.to_string_lossy().to_string()),
            source: "path".to_string(),
        };
    }
    let _ = exe_name;
    ToolStatus {
        found: false,
        path: None,
        source: "missing".to_string(),
    }
}

pub fn find_dism() -> ToolStatus {
    find_system_binary(
        windows_root_dir().join("System32").join("dism.exe"),
        "dism.exe",
    )
}

/// Locates the system curl.exe utility used for Microsoft Update Catalog HTTP operations.
pub fn find_curl() -> ToolStatus {
    find_system_binary(
        windows_root_dir().join("System32").join("curl.exe"),
        "curl.exe",
    )
}

/// Locates the system Windows PowerShell 5.1 executable in `%SystemRoot%\System32`.
pub fn find_powershell() -> ToolStatus {
    find_system_binary(
        windows_root_dir()
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe"),
        "powershell.exe",
    )
}

/// Locates `explorer.exe` in `%SystemRoot%`.
pub fn find_explorer() -> ToolStatus {
    find_system_binary(windows_root_dir().join("explorer.exe"), "explorer.exe")
}

pub fn find_bundled_or_path(
    resource_dir: Option<&Path>,
    relative_candidates: &[&str],
    exe_name: &str,
) -> ToolStatus {
    let mut search_roots = Vec::new();
    if let Some(res_dir) = resource_dir {
        search_roots.push(res_dir.join("resources").join("bin"));
        search_roots.push(res_dir.join("bin"));
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            search_roots.push(exe_dir.join("resources").join("bin"));
            search_roots.push(exe_dir.join("bin"));
            #[cfg(debug_assertions)]
            if let Some(target_dir) = exe_dir.parent().and_then(|p| p.parent()) {
                if let Some(src_tauri_dir) = target_dir.parent() {
                    search_roots.push(src_tauri_dir.join("resources").join("bin"));
                }
            }
        }
    }
    #[cfg(debug_assertions)]
    if let Ok(cwd) = std::env::current_dir() {
        search_roots.push(cwd.join("src-tauri").join("resources").join("bin"));
        search_roots.push(cwd.join("resources").join("bin"));
    }

    for root in search_roots {
        for rel in relative_candidates {
            let candidate = root.join(rel);
            if candidate.is_file() {
                return ToolStatus {
                    found: true,
                    path: Some(candidate.to_string_lossy().to_string()),
                    source: "bundled".to_string(),
                };
            }
        }
    }

    #[cfg(debug_assertions)]
    if let Some(p) = search_path(exe_name) {
        return ToolStatus {
            found: true,
            path: Some(p.to_string_lossy().to_string()),
            source: "path".to_string(),
        };
    }

    let _ = exe_name;
    ToolStatus {
        found: false,
        path: None,
        source: "missing".to_string(),
    }
}

pub fn is_valid_oscdimg_path(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return false;
        }
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("oscdimg.exe"))
        .unwrap_or(false)
}

pub fn find_oscdimg(custom_path: Option<&str>) -> ToolStatus {
    if let Some(custom) = custom_path.filter(|s| !s.trim().is_empty()) {
        let p = PathBuf::from(custom.trim());
        if is_valid_oscdimg_path(&p) {
            return ToolStatus {
                found: true,
                path: Some(p.to_string_lossy().to_string()),
                source: "settings".to_string(),
            };
        }
    }

    let adk_candidates = [
        r"C:\Program Files (x86)\Windows Kits\10\Assessment and Deployment Kit\Deployment Tools\amd64\Oscdimg\oscdimg.exe",
        r"C:\Program Files (x86)\Windows Kits\11\Assessment and Deployment Kit\Deployment Tools\amd64\Oscdimg\oscdimg.exe",
        r"C:\Program Files\Windows Kits\10\Assessment and Deployment Kit\Deployment Tools\amd64\Oscdimg\oscdimg.exe",
    ];
    for candidate in adk_candidates {
        let p = Path::new(candidate);
        if is_valid_oscdimg_path(p) {
            return ToolStatus {
                found: true,
                path: Some(candidate.to_string()),
                source: "adk".to_string(),
            };
        }
    }

    #[cfg(debug_assertions)]
    if let Some(p) = search_path("oscdimg.exe") {
        if is_valid_oscdimg_path(&p) {
            return ToolStatus {
                found: true,
                path: Some(p.to_string_lossy().to_string()),
                source: "path".to_string(),
            };
        }
    }

    ToolStatus {
        found: false,
        path: None,
        source: "missing".to_string(),
    }
}

#[cfg(debug_assertions)]
fn search_path(exe_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn is_process_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
fn is_process_elevated() -> bool {
    false
}

#[cfg(windows)]
fn query_free_space(path: &Path) -> (u64, f64) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let wide: Vec<u16> = probe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_bytes_available = 0u64;
    unsafe {
        if GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free_bytes_available),
            None,
            None,
        )
        .is_ok()
        {
            let gb = free_space_to_gb(free_bytes_available);
            return (free_bytes_available, gb);
        }
    }
    (0, 0.0)
}

#[cfg(not(windows))]
fn query_free_space(_path: &Path) -> (u64, f64) {
    (0, 0.0)
}

fn free_space_to_gb(bytes: u64) -> f64 {
    (bytes as f64) / (1024.0 * 1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bytes_to_gb_accurately() {
        let forty_gb = 40u64 * 1024 * 1024 * 1024;
        assert!((free_space_to_gb(forty_gb) - 40.0).abs() < 1e-6);
    }

    #[test]
    fn validates_oscdimg_filename_strictly() {
        let dir =
            std::env::temp_dir().join(format!("isocraft_oscdimg_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let valid = dir.join("oscdimg.exe");
        let evil = dir.join("cmd.exe");
        std::fs::write(&valid, b"mz").unwrap();
        std::fs::write(&evil, b"mz").unwrap();

        assert!(is_valid_oscdimg_path(&valid));
        assert!(!is_valid_oscdimg_path(&evil));
        assert!(!is_valid_oscdimg_path(&dir));
        assert!(!is_valid_oscdimg_path(&dir.join("missing_oscdimg.exe")));

        #[cfg(windows)]
        {
            // On case-insensitive Windows NTFS `uppercase` resolves to the same file
            let uppercase = dir.join("OSCDIMG.EXE");
            assert!(is_valid_oscdimg_path(&uppercase));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
