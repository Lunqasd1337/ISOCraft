use crate::image::decode_text_bytes;
use crate::reg_parser::{parse_reg_content, OfflineHive, ParsedOpKind, ParsedRegFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegQueueEntry {
    pub path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RegistryBuildPlan {
    pub reg_files: Vec<RegQueueEntry>,
}

/// Validates that a temporary offline hive subkey name matches the `ISOCRAFT_<pid>_<HIVE>` format
/// and cannot reference arbitrary `HKLM` subkeys or contain path separators / NUL bytes.
pub fn is_valid_temp_hive_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("ISOCRAFT_") else {
        return false;
    };
    !suffix.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(windows)]
mod win32_impl {
    use super::*;
    use crate::wim::{register_hive_in_state, unregister_hive_in_state};
    use std::ffi::c_void;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER,
        ERROR_NOT_ALL_ASSIGNED, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, LUID, WIN32_ERROR,
    };
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumKeyExW, RegFlushKey, RegLoadKeyW,
        RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, RegUnLoadKeyW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_ENUMERATE_SUB_KEYS, KEY_READ, KEY_SET_VALUE, KEY_WRITE, REG_DWORD,
        REG_OPEN_CREATE_OPTIONS, REG_OPTION_BACKUP_RESTORE, REG_SAM_FLAGS, REG_VALUE_TYPE,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const DELETE_ACCESS: u32 = 0x0001_0000;

    #[link(name = "ntdll")]
    extern "system" {
        fn NtDeleteKey(KeyHandle: *mut c_void) -> i32;
    }

    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct OwnedHKey(HKEY);

    impl Drop for OwnedHKey {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = RegCloseKey(self.0);
                }
            }
        }
    }

    fn enable_single_privilege(token: HANDLE, name: &str) -> Result<(), String> {
        let wide = to_wide_null(name);
        let mut luid = LUID::default();
        unsafe {
            LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(wide.as_ptr()), &mut luid)
                .map_err(|e| format!("LookupPrivilegeValueW({name}) failed: {e}"))?;

            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };

            AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)
                .map_err(|e| format!("AdjustTokenPrivileges({name}) failed: {e}"))?;

            if GetLastError() == ERROR_NOT_ALL_ASSIGNED {
                return Err(format!(
                    "Privilege '{name}' could not be assigned (Administrator elevation required)."
                ));
            }
        }
        Ok(())
    }

    pub fn enable_backup_restore_privileges() -> Result<(), String> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
            .map_err(|e| format!("OpenProcessToken failed: {e}"))?;

            let r1 = enable_single_privilege(token, "SeBackupPrivilege");
            let r2 = enable_single_privilege(token, "SeRestorePrivilege");
            let _ = CloseHandle(token);

            r1?;
            r2?;
        }
        Ok(())
    }

    pub fn unload_hive_by_name(temp_subkey: &str) -> Result<(), String> {
        if !is_valid_temp_hive_name(temp_subkey) {
            return Err(format!(
                "Refusing to unload untrusted hive name '{temp_subkey}'."
            ));
        }
        let _ = enable_backup_restore_privileges();
        let wide = to_wide_null(temp_subkey);
        let err = unsafe { RegUnLoadKeyW(HKEY_LOCAL_MACHINE, PCWSTR(wide.as_ptr())) };
        if err == ERROR_SUCCESS || err == ERROR_FILE_NOT_FOUND || err == ERROR_INVALID_PARAMETER {
            Ok(())
        } else {
            Err(format!(
                "RegUnLoadKeyW failed for '{temp_subkey}': WIN32_ERROR({})",
                err.0
            ))
        }
    }

    struct LoadedHiveSession {
        workspace_dir: PathBuf,
        loaded: Vec<(OfflineHive, String)>,
    }

    impl LoadedHiveSession {
        fn new(workspace_dir: &Path) -> Self {
            Self {
                workspace_dir: workspace_dir.to_path_buf(),
                loaded: Vec::new(),
            }
        }

        fn temp_key_for(&self, hive: OfflineHive) -> Option<&str> {
            self.loaded
                .iter()
                .find(|(h, _)| *h == hive)
                .map(|(_, k)| k.as_str())
        }

        fn load_hive<F>(
            &mut self,
            hive: OfflineHive,
            mount_dir: &Path,
            mut on_log: F,
        ) -> Result<String, String>
        where
            F: FnMut(&str),
        {
            if let Some(existing) = self.temp_key_for(hive) {
                return Ok(existing.to_string());
            }

            let hive_file = mount_dir.join(hive.relative_hive_path());
            if !hive_file.is_file() {
                return Err(format!(
                    "Offline registry hive file missing: {}",
                    hive_file.display()
                ));
            }

            let temp_name = format!("ISOCRAFT_{}_{}", std::process::id(), hive.temp_suffix());
            register_hive_in_state(&self.workspace_dir, &temp_name)?;

            let sub_w = to_wide_null(&temp_name);
            let file_w = to_wide_null(&hive_file.to_string_lossy());
            let err = unsafe {
                RegLoadKeyW(
                    HKEY_LOCAL_MACHINE,
                    PCWSTR(sub_w.as_ptr()),
                    PCWSTR(file_w.as_ptr()),
                )
            };

            if err != ERROR_SUCCESS {
                let _ = unregister_hive_in_state(&self.workspace_dir, &temp_name);
                return Err(format!(
                    "RegLoadKeyW failed for '{}' at '{}': WIN32_ERROR({})",
                    hive.label(),
                    hive_file.display(),
                    err.0
                ));
            }

            on_log(&format!(
                "Loaded offline hive {} -> HKLM\\{temp_name}",
                hive.label()
            ));
            self.loaded.push((hive, temp_name.clone()));
            Ok(temp_name)
        }

        fn unload_all<F>(&mut self, mut on_log: F) -> Result<(), String>
        where
            F: FnMut(&str),
        {
            let mut first_err: Option<String> = None;
            while let Some((hive, temp_name)) = self.loaded.pop() {
                let name_w = to_wide_null(&temp_name);
                // Flush before unloading
                let mut hkey = HKEY::default();
                let open_err = unsafe {
                    RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        PCWSTR(name_w.as_ptr()),
                        0,
                        KEY_READ,
                        &mut hkey,
                    )
                };
                if open_err == ERROR_SUCCESS && !hkey.is_invalid() {
                    unsafe {
                        let _ = RegFlushKey(hkey);
                        let _ = RegCloseKey(hkey);
                    }
                }

                let err = unsafe { RegUnLoadKeyW(HKEY_LOCAL_MACHINE, PCWSTR(name_w.as_ptr())) };
                if err == ERROR_SUCCESS
                    || err == ERROR_FILE_NOT_FOUND
                    || err == ERROR_INVALID_PARAMETER
                {
                    let _ = unregister_hive_in_state(&self.workspace_dir, &temp_name);
                    on_log(&format!(
                        "Unloaded offline hive {} (HKLM\\{temp_name})",
                        hive.label()
                    ));
                } else if first_err.is_none() {
                    first_err = Some(format!(
                        "RegUnLoadKeyW failed for '{}' (HKLM\\{temp_name}): WIN32_ERROR({})",
                        hive.label(),
                        err.0
                    ));
                }
            }
            if let Some(e) = first_err {
                Err(e)
            } else {
                Ok(())
            }
        }
    }

    impl Drop for LoadedHiveSession {
        fn drop(&mut self) {
            if !self.loaded.is_empty() {
                let _ = self.unload_all(|_| {});
            }
        }
    }

    fn read_current_control_set_name(system_temp_subkey: &str) -> String {
        let select_path = format!("{system_temp_subkey}\\Select");
        let select_w = to_wide_null(&select_path);
        let mut raw_hkey = HKEY::default();
        let err = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(select_w.as_ptr()),
                REG_OPTION_BACKUP_RESTORE.0,
                KEY_READ,
                &mut raw_hkey,
            )
        };
        if err != ERROR_SUCCESS || raw_hkey.is_invalid() {
            return "ControlSet001".to_string();
        }
        let owned = OwnedHKey(raw_hkey);

        let val_w = to_wide_null("Current");
        let mut reg_type = REG_VALUE_TYPE(0);
        let mut buf = [0u8; 4];
        let mut size = 4u32;
        let qerr = unsafe {
            RegQueryValueExW(
                owned.0,
                PCWSTR(val_w.as_ptr()),
                None,
                Some(&mut reg_type),
                Some(buf.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if qerr == ERROR_SUCCESS && reg_type == REG_DWORD && size == 4 {
            let num = u32::from_le_bytes(buf);
            if (1..=999).contains(&num) {
                return format!("ControlSet{num:03}");
            }
        }
        "ControlSet001".to_string()
    }

    fn open_key_backup_restore(
        parent: HKEY,
        subkey: &str,
        access: REG_SAM_FLAGS,
    ) -> Result<Option<OwnedHKey>, WIN32_ERROR> {
        let sub_w = to_wide_null(subkey);
        let mut raw_hkey = HKEY::default();
        let err = unsafe {
            RegOpenKeyExW(
                parent,
                PCWSTR(sub_w.as_ptr()),
                REG_OPTION_BACKUP_RESTORE.0,
                access,
                &mut raw_hkey,
            )
        };
        if err == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else if err == ERROR_SUCCESS {
            Ok(Some(OwnedHKey(raw_hkey)))
        } else {
            Err(err)
        }
    }

    fn create_key_backup_restore(
        parent: HKEY,
        subkey: &str,
        access: REG_SAM_FLAGS,
    ) -> Result<OwnedHKey, String> {
        let sub_w = to_wide_null(subkey);
        let mut raw_hkey = HKEY::default();
        let err = unsafe {
            RegCreateKeyExW(
                parent,
                PCWSTR(sub_w.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPEN_CREATE_OPTIONS(REG_OPTION_BACKUP_RESTORE.0),
                access,
                None,
                &mut raw_hkey,
                None,
            )
        };
        if err != ERROR_SUCCESS {
            return Err(format!(
                "RegCreateKeyExW(REG_OPTION_BACKUP_RESTORE) failed for '{subkey}': WIN32_ERROR({})",
                err.0
            ));
        }
        Ok(OwnedHKey(raw_hkey))
    }

    fn enum_subkeys(hkey: HKEY) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        let mut index = 0u32;
        let mut buf = vec![0u16; 512];
        loop {
            let mut len = buf.len() as u32;
            let err = unsafe {
                RegEnumKeyExW(
                    hkey,
                    index,
                    PWSTR(buf.as_mut_ptr()),
                    &mut len,
                    None,
                    PWSTR::null(),
                    None,
                    None,
                )
            };
            if err == ERROR_NO_MORE_ITEMS {
                break;
            }
            if err != ERROR_SUCCESS {
                return Err(format!("RegEnumKeyExW failed: WIN32_ERROR({})", err.0));
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
            index += 1;
        }
        Ok(names)
    }

    fn delete_key_recursive_privileged(parent: HKEY, subkey: &str) -> Result<(), String> {
        let access = REG_SAM_FLAGS(KEY_ENUMERATE_SUB_KEYS.0 | DELETE_ACCESS);
        let Some(owned) = open_key_backup_restore(parent, subkey, access).map_err(|e| {
            format!(
                "Failed opening key '{subkey}' for privileged deletion: WIN32_ERROR({})",
                e.0
            )
        })?
        else {
            // Key does not exist; treat [-Key] deletion as idempotent success.
            return Ok(());
        };

        let children = enum_subkeys(owned.0)?;
        for child in children {
            delete_key_recursive_privileged(owned.0, &child)?;
        }

        let status = unsafe { NtDeleteKey(owned.0 .0) };
        if status != 0 {
            return Err(format!(
                "NtDeleteKey failed for '{subkey}' with NTSTATUS 0x{:08x}",
                status as u32
            ));
        }
        Ok(())
    }

    fn full_temp_subkey(temp_root: &str, resolved_subkey: &str) -> String {
        if resolved_subkey.is_empty() {
            temp_root.to_string()
        } else {
            format!("{temp_root}\\{resolved_subkey}")
        }
    }

    pub fn apply_reg_files_to_mount<F>(
        mount_dir: &Path,
        workspace_dir: &Path,
        reg_paths: &[PathBuf],
        on_log: F,
    ) -> Result<(), String>
    where
        F: FnMut(&str),
    {
        apply_reg_files_to_mount_cancellable(mount_dir, workspace_dir, reg_paths, || false, on_log)
    }

    pub fn apply_reg_files_to_mount_cancellable<F, C>(
        mount_dir: &Path,
        workspace_dir: &Path,
        reg_paths: &[PathBuf],
        is_cancelled: C,
        mut on_log: F,
    ) -> Result<(), String>
    where
        F: FnMut(&str),
        C: Fn() -> bool,
    {
        if reg_paths.is_empty() {
            return Ok(());
        }

        // Parse & validate all files before mounting any hives
        let mut parsed_files: Vec<(PathBuf, ParsedRegFile)> = Vec::with_capacity(reg_paths.len());
        let mut needed_hives = BTreeSet::new();

        for path in reg_paths {
            if is_cancelled() {
                return Err("Operation cancelled by user.".to_string());
            }
            let bytes = fs::read(path)
                .map_err(|e| format!("Failed reading .reg file '{}': {e}", path.display()))?;
            let content = decode_text_bytes(&bytes);
            let parsed = parse_reg_content(&content);
            if !parsed.errors.is_empty() {
                return Err(format!(
                    "Syntax error(s) in '{}': {}",
                    path.display(),
                    parsed.errors.join("; ")
                ));
            }
            for warn in &parsed.warnings {
                on_log(&format!("[{}] {warn}", path.display()));
            }
            for op in &parsed.operations {
                needed_hives.insert(op.hive);
            }
            parsed_files.push((path.clone(), parsed));
        }

        if needed_hives.is_empty() {
            on_log("No supported offline registry operations found in queued .reg files.");
            return Ok(());
        }

        enable_backup_restore_privileges()?;

        let mut session = LoadedHiveSession::new(workspace_dir);
        let apply_res: Result<(), String> = (|| {
            for hive in &needed_hives {
                if is_cancelled() {
                    return Err("Operation cancelled by user.".to_string());
                }
                session.load_hive(*hive, mount_dir, &mut on_log)?;
            }

            let control_set_name = if let Some(sys_temp) = session.temp_key_for(OfflineHive::System)
            {
                let cs = read_current_control_set_name(sys_temp);
                on_log(&format!("Resolved SYSTEM CurrentControlSet -> {cs}"));
                cs
            } else {
                "ControlSet001".to_string()
            };

            for (path, parsed) in &parsed_files {
                if is_cancelled() {
                    return Err("Operation cancelled by user.".to_string());
                }
                on_log(&format!(
                    "Applying .reg file ({} operations): {}",
                    parsed.operations.len(),
                    path.display()
                ));

                for op in &parsed.operations {
                    if is_cancelled() {
                        return Err("Operation cancelled by user.".to_string());
                    }
                    let temp_root = session
                        .temp_key_for(op.hive)
                        .ok_or_else(|| format!("Hive {} not loaded", op.hive.label()))?;
                    let resolved = op.resolved_subkey(&control_set_name);
                    let target = full_temp_subkey(temp_root, &resolved);

                    match op.kind {
                        ParsedOpKind::CreateKey => {
                            let _key =
                                create_key_backup_restore(HKEY_LOCAL_MACHINE, &target, KEY_WRITE)?;
                        }
                        ParsedOpKind::DeleteKey => {
                            if resolved.is_empty() {
                                return Err(format!(
                                    "Refusing to delete root of offline hive '{}'",
                                    op.hive.label()
                                ));
                            }
                            delete_key_recursive_privileged(HKEY_LOCAL_MACHINE, &target)?;
                        }
                        ParsedOpKind::SetValue => {
                            let val_name = op.value_name.as_deref().unwrap_or("");
                            let val_data = op
                                .value_data
                                .as_ref()
                                .ok_or("Missing value data for SetValue")?;
                            let key = create_key_backup_restore(
                                HKEY_LOCAL_MACHINE,
                                &target,
                                KEY_SET_VALUE,
                            )?;
                            let name_w = to_wide_null(val_name);
                            let name_ptr = if val_name.is_empty() {
                                PCWSTR::null()
                            } else {
                                PCWSTR(name_w.as_ptr())
                            };
                            let data_bytes = val_data.to_win32_bytes();
                            let err = unsafe {
                                RegSetValueExW(
                                    key.0,
                                    name_ptr,
                                    0,
                                    REG_VALUE_TYPE(val_data.reg_type_id()),
                                    Some(&data_bytes),
                                )
                            };
                            if err != ERROR_SUCCESS {
                                return Err(format!(
                                    "RegSetValueExW failed for '{}\\{val_name}': WIN32_ERROR({})",
                                    op.original_key, err.0
                                ));
                            }
                        }
                        ParsedOpKind::DeleteValue => {
                            let val_name = op.value_name.as_deref().unwrap_or("");
                            if let Some(key) =
                                open_key_backup_restore(HKEY_LOCAL_MACHINE, &target, KEY_SET_VALUE)
                                    .map_err(|e| {
                                        format!(
                                    "Failed opening '{target}' for DeleteValue: WIN32_ERROR({})",
                                    e.0
                                )
                                    })?
                            {
                                let name_w = to_wide_null(val_name);
                                let name_ptr = if val_name.is_empty() {
                                    PCWSTR::null()
                                } else {
                                    PCWSTR(name_w.as_ptr())
                                };
                                let err = unsafe { RegDeleteValueW(key.0, name_ptr) };
                                if err != ERROR_SUCCESS && err != ERROR_FILE_NOT_FOUND {
                                    return Err(format!(
                                        "RegDeleteValueW failed for '{}\\{val_name}': WIN32_ERROR({})",
                                        op.original_key, err.0
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        })();

        // Guarantee all loaded hives are closed and unloaded BEFORE unmount_wim runs
        let unload_res = session.unload_all(&mut on_log);
        apply_res?;
        unload_res?;
        Ok(())
    }
}

#[cfg(windows)]
pub use win32_impl::{
    apply_reg_files_to_mount, apply_reg_files_to_mount_cancellable,
    enable_backup_restore_privileges, unload_hive_by_name,
};

#[cfg(not(windows))]
pub fn enable_backup_restore_privileges() -> Result<(), String> {
    Err("Offline registry operations are only supported on Windows.".to_string())
}

#[cfg(not(windows))]
pub fn unload_hive_by_name(temp_subkey: &str) -> Result<(), String> {
    if !is_valid_temp_hive_name(temp_subkey) {
        return Err(format!(
            "Refusing to unload untrusted hive name '{temp_subkey}'."
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn apply_reg_files_to_mount<F>(
    mount_dir: &Path,
    workspace_dir: &Path,
    reg_paths: &[PathBuf],
    on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    apply_reg_files_to_mount_cancellable(mount_dir, workspace_dir, reg_paths, || false, on_log)
}

#[cfg(not(windows))]
pub fn apply_reg_files_to_mount_cancellable<F, C>(
    _mount_dir: &Path,
    _workspace_dir: &Path,
    _reg_paths: &[PathBuf],
    _is_cancelled: C,
    _on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    Err("Offline registry operations are only supported on Windows.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_temp_hive_names_strictly() {
        assert!(is_valid_temp_hive_name("ISOCRAFT_1234_SOFTWARE"));
        assert!(is_valid_temp_hive_name("ISOCRAFT_99_NTUSER"));

        assert!(!is_valid_temp_hive_name(""));
        assert!(!is_valid_temp_hive_name("ISOCRAFT_"));
        assert!(!is_valid_temp_hive_name("SOFTWARE"));
        assert!(!is_valid_temp_hive_name("SYSTEM\\CurrentControlSet"));
        assert!(!is_valid_temp_hive_name("ISOCRAFT_123\\SYSTEM"));
        assert!(!is_valid_temp_hive_name("ISOCRAFT_123\0"));
        assert!(!is_valid_temp_hive_name("../ISOCRAFT_1"));

        assert!(unload_hive_by_name("SOFTWARE").is_err());
        assert!(unload_hive_by_name("ISOCRAFT_123\\SYSTEM").is_err());
    }
}
