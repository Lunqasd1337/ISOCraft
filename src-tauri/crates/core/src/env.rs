pub use crate::fs_utils::strip_windows_verbatim_prefix;
use crate::fs_utils::{is_path_strictly_within_dir, is_symlink_or_reparse, is_unc_or_device_path};
use crate::settings::{AppSettings, validate_workspace_dir};
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
    pub dirty_mounts_count: usize,
    pub dirty_hives_count: usize,
    pub dirty_workspace_details: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStorageStats {
    pub cache_bytes: u64,
    pub cache_files: usize,
    pub downloads_bytes: u64,
    pub downloads_files: usize,
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
    let space_opt = query_free_space(&workspace_path);
    let (free_space_bytes, free_space_gb) = space_opt.unwrap_or((0, 0.0));
    let low_space_warning = matches!(space_opt, Some((_, gb)) if gb < 40.0);

    let (ws, is_corrupted) = crate::wim::read_workspace_state_status(&workspace_path);
    let (has_dirty_workspace, dirty_mounts_count, dirty_hives_count, dirty_workspace_details) =
        if is_corrupted {
            (
                true,
                0,
                0,
                Some("corrupted workspace_state.json".to_string()),
            )
        } else if !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
            (
                true,
                ws.active_mounts.len(),
                ws.loaded_hives.len(),
                Some(format!(
                    "mounts: {}, hives: {}",
                    ws.active_mounts.len(),
                    ws.loaded_hives.len()
                )),
            )
        } else {
            (false, 0, 0, None)
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
        dirty_mounts_count,
        dirty_hives_count,
        dirty_workspace_details,
    }
}

#[cfg(windows)]
fn windows_root_dir() -> PathBuf {
    use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;

    let mut buf = vec![0u16; 260];
    let len = unsafe { GetSystemWindowsDirectoryW(Some(&mut buf)) } as usize;
    if len > 0 && len < buf.len() {
        return PathBuf::from(String::from_utf16_lossy(&buf[..len]));
    }
    if len >= buf.len() {
        let mut larger = vec![0u16; len + 1];
        let len2 = unsafe { GetSystemWindowsDirectoryW(Some(&mut larger)) } as usize;
        if len2 > 0 && len2 < larger.len() {
            return PathBuf::from(String::from_utf16_lossy(&larger[..len2]));
        }
    }

    std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

#[cfg(not(windows))]
fn windows_root_dir() -> PathBuf {
    std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

fn find_system_binary(sys_exe: PathBuf, exe_name: &str) -> ToolStatus {
    if sys_exe.is_file() {
        let clean = strip_windows_verbatim_prefix(&sys_exe);
        return ToolStatus {
            found: true,
            path: Some(clean.to_string_lossy().to_string()),
            source: "system".to_string(),
        };
    }
    #[cfg(debug_assertions)]
    if let Some(p) = search_path(exe_name) {
        let clean = strip_windows_verbatim_prefix(&p);
        return ToolStatus {
            found: true,
            path: Some(clean.to_string_lossy().to_string()),
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

/// Locates the system pnputil.exe utility used for selective host driver export.
pub fn find_pnputil() -> ToolStatus {
    find_system_binary(
        windows_root_dir().join("System32").join("pnputil.exe"),
        "pnputil.exe",
    )
}

/// Locates the system curl.exe utility used for Microsoft Update Catalog HTTP operations.
pub fn find_curl() -> ToolStatus {
    find_system_binary(
        windows_root_dir().join("System32").join("curl.exe"),
        "curl.exe",
    )
}

/// Locates the system tar.exe (bsdtar) utility used for extracting Winget source2.msix.
pub fn find_tar() -> ToolStatus {
    find_system_binary(
        windows_root_dir().join("System32").join("tar.exe"),
        "tar.exe",
    )
}

/// Locates the built-in Windows SQLite library (`winsqlite3.dll`).
pub fn find_winsqlite3() -> ToolStatus {
    find_system_binary(
        windows_root_dir().join("System32").join("winsqlite3.dll"),
        "winsqlite3.dll",
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
        let clean_res = strip_windows_verbatim_prefix(res_dir);
        search_roots.push(clean_res.join("resources").join("bin"));
        search_roots.push(clean_res.join("bin"));
    }
    if let Ok(exe_path) = std::env::current_exe() {
        let clean_exe = strip_windows_verbatim_prefix(&exe_path);
        if let Some(exe_dir) = clean_exe.parent() {
            search_roots.push(exe_dir.join("resources").join("bin"));
            search_roots.push(exe_dir.join("bin"));
            #[cfg(debug_assertions)]
            if let Some(target_dir) = exe_dir.parent().and_then(|p| p.parent())
                && let Some(src_tauri_dir) = target_dir.parent()
            {
                search_roots.push(src_tauri_dir.join("resources").join("bin"));
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
            let mut candidate = root.clone();
            for segment in rel.split(['/', '\\']) {
                if !segment.is_empty() {
                    candidate.push(segment);
                }
            }
            if candidate.is_file() {
                let clean = strip_windows_verbatim_prefix(&candidate);
                return ToolStatus {
                    found: true,
                    path: Some(clean.to_string_lossy().to_string()),
                    source: "bundled".to_string(),
                };
            }
        }
    }

    #[cfg(debug_assertions)]
    if let Some(p) = search_path(exe_name) {
        let clean = strip_windows_verbatim_prefix(&p);
        return ToolStatus {
            found: true,
            path: Some(clean.to_string_lossy().to_string()),
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

fn measure_dir_contents(dir: &Path) -> (u64, usize) {
    let Ok(top_meta) = std::fs::symlink_metadata(dir) else {
        return (0, 0);
    };
    if is_symlink_or_reparse(&top_meta) || !top_meta.is_dir() {
        return (0, 0);
    }

    let mut total_bytes = 0u64;
    let mut total_files = 0usize;
    let mut stack = vec![dir.to_path_buf()];

    while let Some(cur) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&cur) else {
            continue;
        };
        for entry in entries.flatten() {
            let child = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&child) else {
                continue;
            };
            if is_symlink_or_reparse(&meta) {
                continue;
            }
            if meta.is_dir() {
                stack.push(child);
            } else if meta.is_file() {
                total_bytes = total_bytes.saturating_add(meta.len());
                total_files = total_files.saturating_add(1);
            }
        }
    }

    (total_bytes, total_files)
}

pub fn compute_workspace_storage_stats(workspace_dir: &Path) -> WorkspaceStorageStats {
    let (cache_bytes, cache_files) = measure_dir_contents(&workspace_dir.join("cache"));
    let (downloads_bytes, downloads_files) =
        measure_dir_contents(&workspace_dir.join("downloads").join("updates"));
    WorkspaceStorageStats {
        cache_bytes,
        cache_files,
        downloads_bytes,
        downloads_files,
    }
}

pub fn clear_workspace_storage(workspace_dir: &Path, target: &str) -> Result<u64, String> {
    validate_workspace_dir(workspace_dir)?;

    let (ws, is_corrupted) = crate::wim::read_workspace_state_status(workspace_dir);
    if is_corrupted || !ws.active_mounts.is_empty() || !ws.loaded_hives.is_empty() {
        return Err(
            "Cannot clear workspace storage while mounts or registry hives are active. Run workspace recovery first."
                .to_string(),
        );
    }

    let target_dir = match target {
        "cache" => workspace_dir.join("cache"),
        "downloads" => {
            let dl_root = workspace_dir.join("downloads");
            if let Ok(dl_meta) = std::fs::symlink_metadata(&dl_root)
                && (is_symlink_or_reparse(&dl_meta) || !dl_meta.is_dir())
            {
                return Ok(0);
            }
            dl_root.join("updates")
        }
        _ => return Err("Unsupported cache target.".to_string()),
    };
    let Ok(top_meta) = std::fs::symlink_metadata(&target_dir) else {
        return Ok(0);
    };
    if is_symlink_or_reparse(&top_meta) || !top_meta.is_dir() {
        return Ok(0);
    }

    let (freed_bytes, _) = measure_dir_contents(&target_dir);
    let entries = std::fs::read_dir(&target_dir)
        .map_err(|e| format!("Failed reading folder '{}': {e}", target_dir.display()))?;
    for entry in entries.flatten() {
        let child = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&child) else {
            continue;
        };
        if is_symlink_or_reparse(&meta) {
            continue;
        }
        if meta.is_dir() {
            std::fs::remove_dir_all(&child)
                .map_err(|e| format!("Failed cleaning '{}': {e}", child.display()))?;
        } else {
            std::fs::remove_file(&child)
                .map_err(|e| format!("Failed removing '{}': {e}", child.display()))?;
        }
    }
    Ok(freed_bytes)
}

pub fn is_microsoft_signer_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "microsoft corporation"
        || lower == "microsoft windows"
        || lower.starts_with("microsoft corporation ")
        || lower.starts_with("microsoft windows ")
}

pub fn is_in_protected_system_dir(path: &Path) -> bool {
    if is_unc_or_device_path(path) || !path.is_absolute() {
        return false;
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    for env_key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(val) = std::env::var_os(env_key)
            && !val.is_empty()
        {
            dirs.push(PathBuf::from(val));
        }
    }
    dirs.push(windows_root_dir());
    dirs.iter()
        .any(|dir| is_path_strictly_within_dir(path, dir))
}

#[cfg(all(windows, not(test)))]
pub fn verify_microsoft_authenticode(path: &Path) -> bool {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct WintrustFileInfo {
        cb_struct: u32,
        pcwsz_file_path: *const u16,
        h_file: *mut c_void,
        pg_known_subject: *const Guid,
    }

    #[repr(C)]
    struct WintrustData {
        cb_struct: u32,
        p_policy_callback_data: *mut c_void,
        p_sip_client_data: *mut c_void,
        dw_ui_choice: u32,
        fdw_revocation_checks: u32,
        dw_union_choice: u32,
        p_file: *mut WintrustFileInfo,
        dw_state_action: u32,
        h_wvt_state_data: *mut c_void,
        pwsz_url_reference: *const u16,
        dw_prov_flags: u32,
        dw_ui_context: u32,
        p_signature_settings: *mut c_void,
    }

    #[repr(C)]
    struct CryptProviderCertHead {
        cb_struct: u32,
        p_cert: *const c_void,
    }

    #[link(name = "wintrust")]
    unsafe extern "system" {
        fn WinVerifyTrust(
            hwnd: *mut c_void,
            pg_action_id: *const Guid,
            p_wvt_data: *mut WintrustData,
        ) -> i32;
        fn WTHelperProvDataFromStateData(h_state_data: *mut c_void) -> *mut c_void;
        fn WTHelperGetProvSignerFromChain(
            p_prov_data: *mut c_void,
            idx_signer: u32,
            f_counter_signer: i32,
            idx_counter_signer: u32,
        ) -> *mut c_void;
        fn WTHelperGetProvCertFromChain(
            p_sgnr: *mut c_void,
            idx_cert: u32,
        ) -> *mut CryptProviderCertHead;
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CertGetNameStringW(
            p_cert_context: *const c_void,
            dw_type: u32,
            dw_flags: u32,
            pv_type_para: *const c_void,
            psz_name_string: *mut u16,
            cch_name_string: u32,
        ) -> u32;
    }

    const WTD_UI_NONE: u32 = 2;
    const WTD_REVOKE_NONE: u32 = 0;
    const WTD_CHOICE_FILE: u32 = 1;
    const WTD_STATEACTION_VERIFY: u32 = 1;
    const WTD_STATEACTION_CLOSE: u32 = 2;
    const CERT_NAME_SIMPLE_DISPLAY_TYPE: u32 = 4;
    const INVALID_HANDLE: *mut c_void = -1isize as *mut c_void;

    let action_id = Guid {
        data1: 0x00aa_c56b,
        data2: 0xcd44,
        data3: 0x11d0,
        data4: [0x8c, 0xc2, 0x00, 0xc0, 0x4f, 0xc2, 0x95, 0xee],
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut file_info = WintrustFileInfo {
        cb_struct: std::mem::size_of::<WintrustFileInfo>() as u32,
        pcwsz_file_path: wide.as_ptr(),
        h_file: std::ptr::null_mut(),
        pg_known_subject: std::ptr::null(),
    };

    let mut data = WintrustData {
        cb_struct: std::mem::size_of::<WintrustData>() as u32,
        p_policy_callback_data: std::ptr::null_mut(),
        p_sip_client_data: std::ptr::null_mut(),
        dw_ui_choice: WTD_UI_NONE,
        fdw_revocation_checks: WTD_REVOKE_NONE,
        dw_union_choice: WTD_CHOICE_FILE,
        p_file: &mut file_info,
        dw_state_action: WTD_STATEACTION_VERIFY,
        h_wvt_state_data: std::ptr::null_mut(),
        pwsz_url_reference: std::ptr::null(),
        dw_prov_flags: 0,
        dw_ui_context: 0,
        p_signature_settings: std::ptr::null_mut(),
    };

    unsafe {
        let status = WinVerifyTrust(INVALID_HANDLE, &action_id, &mut data);
        let mut is_ms = false;
        if status == 0 && !data.h_wvt_state_data.is_null() {
            let prov_data = WTHelperProvDataFromStateData(data.h_wvt_state_data);
            if !prov_data.is_null() {
                let signer = WTHelperGetProvSignerFromChain(prov_data, 0, 0, 0);
                if !signer.is_null() {
                    let prov_cert = WTHelperGetProvCertFromChain(signer, 0);
                    if !prov_cert.is_null() {
                        let cert_ctx = (*prov_cert).p_cert;
                        if !cert_ctx.is_null() {
                            let mut name_buf = [0u16; 256];
                            let chars = CertGetNameStringW(
                                cert_ctx,
                                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                                0,
                                std::ptr::null(),
                                name_buf.as_mut_ptr(),
                                name_buf.len() as u32,
                            );
                            if chars > 1 {
                                let name =
                                    String::from_utf16_lossy(&name_buf[..(chars as usize - 1)]);
                                is_ms = is_microsoft_signer_name(&name);
                            }
                        }
                    }
                }
            }
        }
        data.dw_state_action = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(INVALID_HANDLE, &action_id, &mut data);
        status == 0 && is_ms
    }
}

pub fn is_valid_oscdimg_path(path: &Path) -> bool {
    if is_unc_or_device_path(path) {
        return false;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if is_symlink_or_reparse(&meta) || !meta.is_file() {
        return false;
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("oscdimg.exe"))
        .unwrap_or(false)
}

pub fn is_trusted_oscdimg_path(path: &Path) -> bool {
    #[cfg(all(windows, not(test)))]
    {
        is_valid_oscdimg_path(path)
            && is_in_protected_system_dir(path)
            && verify_microsoft_authenticode(path)
    }
    #[cfg(not(all(windows, not(test))))]
    {
        is_valid_oscdimg_path(path)
    }
}

pub fn find_oscdimg(custom_path: Option<&str>) -> ToolStatus {
    if let Some(custom) = custom_path.filter(|s| !s.trim().is_empty()) {
        let p = PathBuf::from(custom.trim());
        if is_trusted_oscdimg_path(&p) {
            let clean = strip_windows_verbatim_prefix(&p);
            return ToolStatus {
                found: true,
                path: Some(clean.to_string_lossy().to_string()),
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
        if is_trusted_oscdimg_path(p) {
            return ToolStatus {
                found: true,
                path: Some(candidate.to_string()),
                source: "adk".to_string(),
            };
        }
    }

    #[cfg(debug_assertions)]
    if let Some(p) = search_path("oscdimg.exe")
        && is_trusted_oscdimg_path(&p)
    {
        let clean = strip_windows_verbatim_prefix(&p);
        return ToolStatus {
            found: true,
            path: Some(clean.to_string_lossy().to_string()),
            source: "path".to_string(),
        };
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
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
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
fn query_free_space(path: &Path) -> Option<(u64, f64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

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
            return Some((free_bytes_available, gb));
        }
    }
    None
}

#[cfg(not(windows))]
fn query_free_space(_path: &Path) -> Option<(u64, f64)> {
    None
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

    #[test]
    fn strips_windows_verbatim_prefix_safely() {
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(
                r"\\?\C:\Program Files\ISOCraft\resources\bin\wimlib\wimlib-imagex.exe"
            )),
            PathBuf::from(r"C:\Program Files\ISOCraft\resources\bin\wimlib\wimlib-imagex.exe")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\UNC\server\share\tool.exe")),
            PathBuf::from(r"\\server\share\tool.exe")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\unc\server\share\tool.exe")),
            PathBuf::from(r"\\server\share\tool.exe")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\Volume{1234}\foo")),
            PathBuf::from(r"\\?\Volume{1234}\foo")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"C:\Windows\System32\dism.exe")),
            PathBuf::from(r"C:\Windows\System32\dism.exe")
        );
        let long_tail = "a".repeat(260);
        let long_verbatim = format!(r"\\?\C:\{long_tail}");
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(&long_verbatim)),
            PathBuf::from(&long_verbatim)
        );
    }

    #[test]
    fn find_bundled_or_path_strips_verbatim_prefix_and_normalizes_separators() {
        let dir = std::env::temp_dir().join(format!(
            "isocraft_bundled_verbatim_test_{}",
            std::process::id()
        ));
        let wimlib_dir = dir.join("resources").join("bin").join("wimlib");
        std::fs::create_dir_all(&wimlib_dir).unwrap();
        let exe = wimlib_dir.join("wimlib-imagex.exe");
        std::fs::write(&exe, b"mz").unwrap();

        #[cfg(windows)]
        let res_dir = {
            let abs = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            let abs_str = abs.to_string_lossy();
            if abs_str.starts_with(r"\\?\") {
                PathBuf::from(abs_str.to_string())
            } else {
                PathBuf::from(format!(r"\\?\{abs_str}"))
            }
        };
        #[cfg(not(windows))]
        let res_dir = dir.clone();

        let status = find_bundled_or_path(
            Some(&res_dir),
            &["wimlib/wimlib-imagex.exe", "wimlib-imagex.exe"],
            "wimlib-imagex.exe",
        );

        assert!(status.found);
        assert_eq!(status.source, "bundled");
        let resolved = status.path.expect("resolved path");
        assert!(
            !resolved.starts_with(r"\\?\"),
            "unexpected verbatim prefix in {resolved}"
        );
        #[cfg(windows)]
        assert!(
            !resolved.contains('/'),
            "unexpected forward slash in {resolved}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_environment_reports_dirty_mounts_and_hives_counts() {
        let dir = std::env::temp_dir().join(format!(
            "isocraft_dirty_workspace_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let ws_state = WorkspaceState {
            active_mounts: vec![
                r"C:\work\mount".to_string(),
                r"C:\work\boot_mount".to_string(),
            ],
            loaded_hives: vec!["ISOCRAFT_123_SOFTWARE".to_string()],
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        std::fs::write(
            dir.join("workspace_state.json"),
            serde_json::to_string(&ws_state).unwrap(),
        )
        .unwrap();

        let settings = AppSettings {
            workspace_dir: dir.to_string_lossy().to_string(),
            oscdimg_path: None,
            theme: "system".to_string(),
            language: "auto".to_string(),
            ..AppSettings::default()
        };

        let status = check_environment_status(None, &settings);
        assert!(status.has_dirty_workspace);
        assert_eq!(status.dirty_mounts_count, 2);
        assert_eq!(status.dirty_hives_count, 1);
        assert_eq!(
            status.dirty_workspace_details.as_deref(),
            Some("mounts: 2, hives: 1")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn computes_and_clears_workspace_storage_safely() {
        let dir = std::env::temp_dir().join(format!(
            "isocraft_storage_stats_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let cache_dir = dir.join("cache");
        let downloads_dir = dir.join("downloads").join("updates");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(&downloads_dir).unwrap();

        std::fs::write(cache_dir.join("catalog_1.json"), b"12345").unwrap();
        std::fs::write(downloads_dir.join("kb123.msu"), b"1234567890").unwrap();

        let stats = compute_workspace_storage_stats(&dir);
        assert_eq!(stats.cache_bytes, 5);
        assert_eq!(stats.cache_files, 1);
        assert_eq!(stats.downloads_bytes, 10);
        assert_eq!(stats.downloads_files, 1);

        assert!(clear_workspace_storage(&dir, "invalid").is_err());

        let freed_cache = clear_workspace_storage(&dir, "cache").unwrap();
        assert_eq!(freed_cache, 5);
        let after_cache = compute_workspace_storage_stats(&dir);
        assert_eq!(after_cache.cache_bytes, 0);
        assert_eq!(after_cache.cache_files, 0);
        assert_eq!(after_cache.downloads_bytes, 10);
        assert_eq!(after_cache.downloads_files, 1);

        let freed_dl = clear_workspace_storage(&dir, "downloads").unwrap();
        assert_eq!(freed_dl, 10);
        assert_eq!(clear_workspace_storage(&dir, "downloads").unwrap(), 0);

        // Dirty workspace must block storage clearing
        std::fs::write(
            dir.join("workspace_state.json"),
            r#"{"activeMounts":["C:\\mount"],"loadedHives":[],"updatedAt":"now"}"#,
        )
        .unwrap();
        assert!(clear_workspace_storage(&dir, "cache").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validates_microsoft_signer_names_and_protected_dirs() {
        assert!(is_microsoft_signer_name("Microsoft Corporation"));
        assert!(is_microsoft_signer_name("Microsoft Windows"));
        assert!(is_microsoft_signer_name("  microsoft corporation  "));
        assert!(!is_microsoft_signer_name("Evil Corp"));
        assert!(!is_microsoft_signer_name("Microsoft Fake"));

        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let p = PathBuf::from(pf)
                .join("Windows Kits")
                .join("10")
                .join("oscdimg.exe");
            assert!(is_in_protected_system_dir(&p));
        }
        assert!(!is_in_protected_system_dir(Path::new(
            r"C:\Users\Public\oscdimg.exe"
        )));
        assert!(!is_in_protected_system_dir(Path::new(
            r"\\server\share\oscdimg.exe"
        )));
    }
}
