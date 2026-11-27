use crate::fs_utils::read_file_bounded;
use crate::image::decode_text_bytes;
use crate::reg_parser::{OfflineHive, ParsedOpKind, ParsedRegFile, ParsedRegOp, parse_reg_content};
use crate::services::{
    RawOfflineService, ServiceApplyReport, ServiceStartChange, ServiceStartType, ServicesBuildPlan,
    is_manageable_win32_service_type, validate_services_plan,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const MAX_REG_FILE_BYTES: u64 = 64 * 1024 * 1024;

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

/// Returns true if deleting `resolved_subkey` inside `hive` would destroy a critical root
/// of the offline hive (e.g., hive root, `SOFTWARE\Classes`, `SYSTEM\Select`, `SYSTEM\ControlSetNNN`).
pub fn is_protected_root_delete(hive: OfflineHive, resolved_subkey: &str) -> bool {
    let norm = resolved_subkey.trim_matches('\\').to_ascii_lowercase();
    if norm.is_empty() {
        return true;
    }
    match hive {
        OfflineHive::Software => norm == "classes",
        OfflineHive::System => {
            norm == "select"
                || (norm.starts_with("controlset")
                    && norm.len() == 13
                    && norm[10..].bytes().all(|b| b.is_ascii_digit()))
        }
        _ => false,
    }
}

#[cfg(windows)]
mod win32_impl {
    use super::*;
    use crate::wim::{register_hive_in_state, unregister_hive_in_state};
    use std::ffi::c_void;
    use std::time::Duration;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER,
        ERROR_NO_MORE_ITEMS, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS, GetLastError, HANDLE, LUID,
        WIN32_ERROR,
    };
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE,
        KEY_WRITE, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_OPEN_CREATE_OPTIONS,
        REG_OPTION_BACKUP_RESTORE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
        RegCreateKeyExW, RegDeleteValueW, RegEnumKeyExW, RegFlushKey, RegLoadKeyW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW, RegUnLoadKeyW,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::{PCWSTR, PWSTR};

    const DELETE_ACCESS: u32 = 0x0001_0000;
    const REG_OPTION_OPEN_LINK: u32 = 0x0000_0008;
    const REG_LINK_VALUE_TYPE: u32 = 6;

    #[link(name = "ntdll")]
    unsafe extern "system" {
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

    fn reg_unload_with_retry(subkey_wide: &[u16]) -> WIN32_ERROR {
        let mut err = WIN32_ERROR(0);
        for attempt in 0..5 {
            err = unsafe { RegUnLoadKeyW(HKEY_LOCAL_MACHINE, PCWSTR(subkey_wide.as_ptr())) };
            if err != ERROR_ACCESS_DENIED || attempt == 4 {
                return err;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        err
    }

    pub fn unload_hive_by_name(temp_subkey: &str) -> Result<(), String> {
        if !is_valid_temp_hive_name(temp_subkey) {
            return Err(format!(
                "Refusing to unload untrusted hive name '{temp_subkey}'."
            ));
        }
        let _ = enable_backup_restore_privileges();
        let wide = to_wide_null(temp_subkey);
        let err = reg_unload_with_retry(&wide);
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
        loaded: Vec<(Option<OfflineHive>, String, String)>,
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
                .find(|(h, _, _)| *h == Some(hive))
                .map(|(_, _, k)| k.as_str())
        }

        fn load_hive_file<F>(
            &mut self,
            hive_tag: Option<OfflineHive>,
            hive_label: &str,
            temp_suffix: &str,
            hive_file: &Path,
            on_log: &mut F,
        ) -> Result<String, String>
        where
            F: FnMut(&str),
        {
            if !hive_file.is_file() {
                return Err(format!(
                    "Offline registry hive file missing: {}",
                    hive_file.display()
                ));
            }

            let temp_name = format!("ISOCRAFT_{}_{}", std::process::id(), temp_suffix);
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
                    hive_label,
                    hive_file.display(),
                    err.0
                ));
            }

            on_log(&format!(
                "Loaded offline hive {hive_label} -> HKLM\\{temp_name}"
            ));
            self.loaded
                .push((hive_tag, hive_label.to_string(), temp_name.clone()));
            Ok(temp_name)
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
            self.load_hive_file(
                Some(hive),
                hive.label(),
                hive.temp_suffix(),
                &hive_file,
                &mut on_log,
            )
        }

        fn unload_all<F>(&mut self, mut on_log: F) -> Result<(), String>
        where
            F: FnMut(&str),
        {
            let mut first_err: Option<String> = None;
            while let Some((_hive, hive_label, temp_name)) = self.loaded.pop() {
                let name_w = to_wide_null(&temp_name);
                // Flush before unloading
                let mut hkey = HKEY::default();
                let open_err = unsafe {
                    RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        PCWSTR(name_w.as_ptr()),
                        Some(0),
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

                let err = reg_unload_with_retry(&name_w);
                if err == ERROR_SUCCESS
                    || err == ERROR_FILE_NOT_FOUND
                    || err == ERROR_INVALID_PARAMETER
                {
                    let _ = unregister_hive_in_state(&self.workspace_dir, &temp_name);
                    on_log(&format!(
                        "Unloaded offline hive {hive_label} (HKLM\\{temp_name})"
                    ));
                } else if first_err.is_none() {
                    first_err = Some(format!(
                        "RegUnLoadKeyW failed for '{hive_label}' (HKLM\\{temp_name}): WIN32_ERROR({})",
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
                Some(REG_OPTION_BACKUP_RESTORE.0),
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

    fn check_not_reg_link(hkey: HKEY, display_path: &str) -> Result<(), String> {
        let sym_w = to_wide_null("SymbolicLinkValue");
        let mut reg_type = REG_VALUE_TYPE(0);
        let mut size = 0u32;
        let qerr = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(sym_w.as_ptr()),
                None,
                Some(&mut reg_type),
                None,
                Some(&mut size),
            )
        };
        if qerr == ERROR_SUCCESS && reg_type.0 == REG_LINK_VALUE_TYPE {
            return Err(format!(
                "Refusing to traverse registry symbolic link (REG_LINK) at '{display_path}'."
            ));
        }
        Ok(())
    }

    fn open_single_segment_no_link(
        parent: HKEY,
        segment: &str,
        access: REG_SAM_FLAGS,
        display_path: &str,
    ) -> Result<Option<OwnedHKey>, String> {
        let sub_w = to_wide_null(segment);
        let mut raw_hkey = HKEY::default();
        let err = unsafe {
            RegOpenKeyExW(
                parent,
                PCWSTR(sub_w.as_ptr()),
                Some(REG_OPTION_BACKUP_RESTORE.0 | REG_OPTION_OPEN_LINK),
                REG_SAM_FLAGS(access.0 | KEY_QUERY_VALUE.0),
                &mut raw_hkey,
            )
        };
        if err == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if err != ERROR_SUCCESS {
            return Err(format!(
                "Failed opening registry key '{display_path}': WIN32_ERROR({})",
                err.0
            ));
        }
        let owned = OwnedHKey(raw_hkey);
        check_not_reg_link(owned.0, display_path)?;
        Ok(Some(owned))
    }

    fn open_subkey_no_links(
        temp_root: &str,
        resolved_subkey: &str,
        final_access: REG_SAM_FLAGS,
    ) -> Result<Option<OwnedHKey>, String> {
        let segments: Vec<&str> = resolved_subkey
            .split('\\')
            .filter(|s| !s.is_empty())
            .collect();

        let root_access = if segments.is_empty() {
            final_access
        } else {
            KEY_QUERY_VALUE
        };
        let Some(mut current) =
            open_single_segment_no_link(HKEY_LOCAL_MACHINE, temp_root, root_access, temp_root)?
        else {
            return Ok(None);
        };

        let mut trace = temp_root.to_string();
        for (idx, seg) in segments.iter().enumerate() {
            trace.push('\\');
            trace.push_str(seg);
            let seg_access = if idx + 1 == segments.len() {
                final_access
            } else {
                KEY_QUERY_VALUE
            };
            let Some(next) = open_single_segment_no_link(current.0, seg, seg_access, &trace)?
            else {
                return Ok(None);
            };
            current = next;
        }
        Ok(Some(current))
    }

    fn create_subkey_no_links(
        temp_root: &str,
        resolved_subkey: &str,
        final_access: REG_SAM_FLAGS,
    ) -> Result<OwnedHKey, String> {
        let segments: Vec<&str> = resolved_subkey
            .split('\\')
            .filter(|s| !s.is_empty())
            .collect();

        let root_access = if segments.is_empty() {
            final_access
        } else {
            KEY_QUERY_VALUE
        };
        let mut current =
            open_single_segment_no_link(HKEY_LOCAL_MACHINE, temp_root, root_access, temp_root)?
                .ok_or_else(|| format!("Temporary offline hive root '{temp_root}' is missing."))?;

        let mut trace = temp_root.to_string();
        for (idx, seg) in segments.iter().enumerate() {
            trace.push('\\');
            trace.push_str(seg);
            let seg_access = if idx + 1 == segments.len() {
                final_access
            } else {
                KEY_QUERY_VALUE
            };
            if let Some(existing) = open_single_segment_no_link(current.0, seg, seg_access, &trace)?
            {
                current = existing;
                continue;
            }

            let seg_w = to_wide_null(seg);
            let mut raw_hkey = HKEY::default();
            let err = unsafe {
                RegCreateKeyExW(
                    current.0,
                    PCWSTR(seg_w.as_ptr()),
                    Some(0),
                    PCWSTR::null(),
                    REG_OPEN_CREATE_OPTIONS(REG_OPTION_BACKUP_RESTORE.0),
                    REG_SAM_FLAGS(seg_access.0 | KEY_QUERY_VALUE.0),
                    None,
                    &mut raw_hkey,
                    None,
                )
            };
            if err != ERROR_SUCCESS {
                return Err(format!(
                    "RegCreateKeyExW(REG_OPTION_BACKUP_RESTORE) failed for '{trace}': WIN32_ERROR({})",
                    err.0
                ));
            }
            let created = OwnedHKey(raw_hkey);
            check_not_reg_link(created.0, &trace)?;
            current = created;
        }
        Ok(current)
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
                    Some(PWSTR(buf.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
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

    fn delete_key_recursive_privileged(
        temp_root: &str,
        resolved_subkey: &str,
    ) -> Result<(), String> {
        const MAX_DELETE_DEPTH: usize = 512;

        struct StackFrame {
            handle: OwnedHKey,
            name: String,
            pending_children: Option<Vec<String>>,
        }

        let access = REG_SAM_FLAGS(KEY_ENUMERATE_SUB_KEYS.0 | KEY_QUERY_VALUE.0 | DELETE_ACCESS);
        let Some(root_owned) = open_subkey_no_links(temp_root, resolved_subkey, access)? else {
            // Key does not exist; treat [-Key] deletion as idempotent success.
            return Ok(());
        };

        let mut stack = vec![StackFrame {
            handle: root_owned,
            name: resolved_subkey.to_string(),
            pending_children: None,
        }];

        while !stack.is_empty() {
            let needs_enum = stack
                .last()
                .map(|f| f.pending_children.is_none())
                .unwrap_or(false);
            if needs_enum {
                let (children, cur_name, cur_depth) = {
                    let top = stack.last_mut().expect("stack non-empty");
                    let children = enum_subkeys(top.handle.0)?;
                    (children, top.name.clone(), stack.len())
                };
                if !children.is_empty() && cur_depth >= MAX_DELETE_DEPTH {
                    return Err(format!(
                        "Registry deletion depth limit ({MAX_DELETE_DEPTH}) exceeded at '{cur_name}'"
                    ));
                }
                stack.last_mut().expect("stack non-empty").pending_children = Some(children);
            }

            let next_child = {
                let top = stack.last_mut().expect("stack non-empty");
                let parent_name = top.name.clone();
                top.pending_children
                    .as_mut()
                    .and_then(|v| v.pop())
                    .map(|c| (top.handle.0, parent_name, c))
            };

            if let Some((parent_hkey, parent_name, child)) = next_child {
                let child_display = format!("{parent_name}\\{child}");
                if let Some(child_owned) =
                    open_single_segment_no_link(parent_hkey, &child, access, &child_display)?
                {
                    stack.push(StackFrame {
                        handle: child_owned,
                        name: child_display,
                        pending_children: None,
                    });
                }
            } else {
                let frame = stack.pop().expect("stack non-empty");
                let status = unsafe { NtDeleteKey(frame.handle.0.0) };
                if status != 0 {
                    return Err(format!(
                        "NtDeleteKey failed for '{}' with NTSTATUS 0x{:08x}",
                        frame.name, status as u32
                    ));
                }
            }
        }
        Ok(())
    }

    fn apply_parsed_ops_in_session<F, C>(
        session: &LoadedHiveSession,
        control_set_name: &str,
        operations: &[ParsedRegOp],
        is_cancelled: &C,
        on_log: &mut F,
    ) -> Result<(), String>
    where
        F: FnMut(&str),
        C: Fn() -> bool,
    {
        let _ = on_log;
        for op in operations {
            if is_cancelled() {
                return Err("Operation cancelled by user.".to_string());
            }
            let temp_root = session
                .temp_key_for(op.hive)
                .ok_or_else(|| format!("Hive {} not loaded", op.hive.label()))?;
            let resolved = op.resolved_subkey(control_set_name);

            match op.kind {
                ParsedOpKind::CreateKey => {
                    let _key = create_subkey_no_links(temp_root, &resolved, KEY_WRITE)?;
                }
                ParsedOpKind::DeleteKey => {
                    if is_protected_root_delete(op.hive, &resolved) {
                        return Err(format!(
                            "Refusing to delete protected root key '{}' in offline hive '{}'.",
                            op.original_key,
                            op.hive.label()
                        ));
                    }
                    delete_key_recursive_privileged(temp_root, &resolved)?;
                }
                ParsedOpKind::SetValue => {
                    let val_name = op.value_name.as_deref().unwrap_or("");
                    let val_data = op
                        .value_data
                        .as_ref()
                        .ok_or("Missing value data for SetValue")?;
                    let key = create_subkey_no_links(temp_root, &resolved, KEY_SET_VALUE)?;
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
                            Some(0),
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
                    if let Some(key) = open_subkey_no_links(temp_root, &resolved, KEY_SET_VALUE)? {
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
        Ok(())
    }

    pub fn apply_reg_ops_to_mount_cancellable<F, C>(
        mount_dir: &Path,
        workspace_dir: &Path,
        operations: &[ParsedRegOp],
        is_cancelled: C,
        mut on_log: F,
    ) -> Result<(), String>
    where
        F: FnMut(&str),
        C: Fn() -> bool,
    {
        if operations.is_empty() {
            return Ok(());
        }
        let mut needed_hives = BTreeSet::new();
        for op in operations {
            needed_hives.insert(op.hive);
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

            apply_parsed_ops_in_session(
                &session,
                &control_set_name,
                operations,
                &is_cancelled,
                &mut on_log,
            )
        })();

        // Guarantee all loaded hives are closed and unloaded BEFORE unmount_wim runs
        let unload_res = session.unload_all(&mut on_log);
        apply_res?;
        unload_res?;
        Ok(())
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
            let bytes = read_file_bounded(path, MAX_REG_FILE_BYTES, true)?;
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
                apply_parsed_ops_in_session(
                    &session,
                    &control_set_name,
                    &parsed.operations,
                    &is_cancelled,
                    &mut on_log,
                )?;
            }

            Ok(())
        })();

        // Guarantee all loaded hives are closed and unloaded BEFORE unmount_wim runs
        let unload_res = session.unload_all(&mut on_log);
        apply_res?;
        unload_res?;
        Ok(())
    }

    fn query_dword_value(hkey: HKEY, name: &str) -> Option<u32> {
        let val_w = to_wide_null(name);
        let mut reg_type = REG_VALUE_TYPE(0);
        let mut buf = [0u8; 4];
        let mut size = 4u32;
        let qerr = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(val_w.as_ptr()),
                None,
                Some(&mut reg_type),
                Some(buf.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if qerr == ERROR_SUCCESS && reg_type == REG_DWORD && size == 4 {
            Some(u32::from_le_bytes(buf))
        } else {
            None
        }
    }

    fn query_raw_value_bytes(hkey: HKEY, name: &str) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
        const MAX_SERVICE_VAL_BYTES: u32 = 256 * 1024;
        let val_w = to_wide_null(name);
        let mut reg_type = REG_VALUE_TYPE(0);
        let mut size = 0u32;
        let qerr = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(val_w.as_ptr()),
                None,
                Some(&mut reg_type),
                None,
                Some(&mut size),
            )
        };
        if qerr != ERROR_SUCCESS || size == 0 || size > MAX_SERVICE_VAL_BYTES {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let mut actual_size = size;
        let qerr2 = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(val_w.as_ptr()),
                None,
                Some(&mut reg_type),
                Some(buf.as_mut_ptr()),
                Some(&mut actual_size),
            )
        };
        if qerr2 != ERROR_SUCCESS {
            return None;
        }
        buf.truncate(actual_size as usize);
        Some((reg_type, buf))
    }

    fn query_string_value(hkey: HKEY, name: &str) -> Option<String> {
        let (reg_type, bytes) = query_raw_value_bytes(hkey, name)?;
        if reg_type != REG_SZ && reg_type != REG_EXPAND_SZ {
            return None;
        }
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .take_while(|&u| u != 0)
            .collect();
        let s = String::from_utf16_lossy(&units).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    fn query_multi_sz_value(hkey: HKEY, name: &str) -> Vec<String> {
        let Some((reg_type, bytes)) = query_raw_value_bytes(hkey, name) else {
            return Vec::new();
        };
        if reg_type != REG_MULTI_SZ && reg_type != REG_SZ {
            return Vec::new();
        }
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        let mut items = Vec::new();
        let mut start = 0usize;
        for (idx, &u) in units.iter().enumerate() {
            if u == 0 {
                if idx > start {
                    let s = String::from_utf16_lossy(&units[start..idx])
                        .trim()
                        .to_string();
                    if !s.is_empty() {
                        items.push(s);
                    }
                }
                start = idx + 1;
            }
        }
        if start < units.len() {
            let tail = String::from_utf16_lossy(&units[start..]).trim().to_string();
            if !tail.is_empty() {
                items.push(tail);
            }
        }
        items
    }

    fn has_subkey(parent: HKEY, name: &str) -> bool {
        let sub_w = to_wide_null(name);
        let mut raw = HKEY::default();
        let err = unsafe {
            RegOpenKeyExW(
                parent,
                PCWSTR(sub_w.as_ptr()),
                Some(REG_OPTION_BACKUP_RESTORE.0 | REG_OPTION_OPEN_LINK),
                KEY_QUERY_VALUE,
                &mut raw,
            )
        };
        if err == ERROR_SUCCESS && !raw.is_invalid() {
            let _owned = OwnedHKey(raw);
            true
        } else {
            false
        }
    }

    #[allow(clippy::permissions_set_readonly_false)]
    fn clear_file_readonly(path: &Path) {
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(path, perms);
            }
        }
    }

    fn cleanup_temp_system_hive_files(inspect_dir: &Path) {
        for suffix in ["temp_SYSTEM", "temp_SYSTEM.LOG1", "temp_SYSTEM.LOG2"] {
            let p = inspect_dir.join(suffix);
            if p.exists() {
                clear_file_readonly(&p);
                let _ = std::fs::remove_file(&p);
            }
        }
    }

    /// Reads Win32 services from a read-only mounted WIM by copying `SYSTEM` (+ logs)
    /// into `inspect_dir`, clearing ReadOnly, and loading it temporarily with `SeBackupPrivilege`.
    pub fn read_offline_services_from_mount<F, C>(
        mount_dir: &Path,
        inspect_dir: &Path,
        workspace_dir: &Path,
        is_cancelled: C,
        mut on_log: F,
    ) -> Result<Vec<RawOfflineService>, String>
    where
        F: FnMut(&str),
        C: Fn() -> bool,
    {
        let src_system = mount_dir.join(OfflineHive::System.relative_hive_path());
        if !src_system.is_file() {
            return Err(format!(
                "Offline registry hive file missing: {}",
                src_system.display()
            ));
        }

        std::fs::create_dir_all(inspect_dir)
            .map_err(|e| format!("Failed creating inspect dir: {e}"))?;
        cleanup_temp_system_hive_files(inspect_dir);

        let dst_system = inspect_dir.join("temp_SYSTEM");
        std::fs::copy(&src_system, &dst_system)
            .map_err(|e| format!("Failed copying SYSTEM hive for inspection: {e}"))?;
        clear_file_readonly(&dst_system);

        for log_ext in ["LOG1", "LOG2"] {
            let src_log = mount_dir.join(format!(
                "{}.{}",
                OfflineHive::System.relative_hive_path(),
                log_ext
            ));
            if src_log.is_file() {
                let dst_log = inspect_dir.join(format!("temp_SYSTEM.{log_ext}"));
                if std::fs::copy(&src_log, &dst_log).is_ok() {
                    clear_file_readonly(&dst_log);
                }
            }
        }

        let read_res: Result<Vec<RawOfflineService>, String> = (|| {
            enable_backup_restore_privileges()?;
            let mut session = LoadedHiveSession::new(workspace_dir);
            let inner: Result<Vec<RawOfflineService>, String> = (|| {
                if is_cancelled() {
                    return Err("Operation cancelled by user.".to_string());
                }
                let temp_root = session.load_hive_file(
                    None,
                    "SYSTEM (RO)",
                    "SYSTEM_RO",
                    &dst_system,
                    &mut on_log,
                )?;
                let control_set_name = read_current_control_set_name(&temp_root);
                let services_subkey = format!("{control_set_name}\\Services");

                let svc_access = REG_SAM_FLAGS(KEY_ENUMERATE_SUB_KEYS.0 | KEY_QUERY_VALUE.0);
                let Some(services_key) =
                    open_subkey_no_links(&temp_root, &services_subkey, svc_access)?
                else {
                    return Ok(Vec::new());
                };

                let names = enum_subkeys(services_key.0)?;
                let mut raw_services = Vec::with_capacity(names.len());
                for name in names {
                    if is_cancelled() {
                        return Err("Operation cancelled by user.".to_string());
                    }
                    let trace = format!("{temp_root}\\{services_subkey}\\{name}");
                    let Ok(Some(entry_key)) = open_single_segment_no_link(
                        services_key.0,
                        &name,
                        KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS,
                        &trace,
                    ) else {
                        continue;
                    };

                    let Some(service_type) = query_dword_value(entry_key.0, "Type") else {
                        continue;
                    };
                    if !is_manageable_win32_service_type(service_type) {
                        continue;
                    }
                    let Some(start) = query_dword_value(entry_key.0, "Start") else {
                        continue;
                    };
                    let delayed_autostart = query_dword_value(entry_key.0, "DelayedAutostart");
                    if ServiceStartType::from_registry_values(start, delayed_autostart).is_none() {
                        continue;
                    }

                    let display_name = query_string_value(entry_key.0, "DisplayName");
                    let description = query_string_value(entry_key.0, "Description");
                    let group = query_string_value(entry_key.0, "Group");
                    let depends_on = query_multi_sz_value(entry_key.0, "DependOnService");
                    let has_trigger_start = has_subkey(entry_key.0, "TriggerInfo");

                    raw_services.push(RawOfflineService {
                        name,
                        service_type,
                        start,
                        delayed_autostart,
                        display_name,
                        description,
                        group,
                        depends_on,
                        has_trigger_start,
                    });
                }
                Ok(raw_services)
            })();
            let unload_res = session.unload_all(&mut on_log);
            let data = inner?;
            unload_res?;
            Ok(data)
        })();

        cleanup_temp_system_hive_files(inspect_dir);
        read_res
    }

    pub fn apply_service_changes_to_mount_cancellable<F, C>(
        mount_dir: &Path,
        workspace_dir: &Path,
        changes: &[ServiceStartChange],
        is_cancelled: C,
        mut on_log: F,
    ) -> Result<ServiceApplyReport, String>
    where
        F: FnMut(&str),
        C: Fn() -> bool,
    {
        if changes.is_empty() {
            return Ok(ServiceApplyReport::default());
        }
        validate_services_plan(&ServicesBuildPlan {
            changes: changes.to_vec(),
        })?;

        enable_backup_restore_privileges()?;

        let mut session = LoadedHiveSession::new(workspace_dir);
        let mut report = ServiceApplyReport::default();

        let apply_res: Result<(), String> = (|| {
            if is_cancelled() {
                return Err("Operation cancelled by user.".to_string());
            }
            let sys_temp = session.load_hive(OfflineHive::System, mount_dir, &mut on_log)?;
            let control_set_name = read_current_control_set_name(&sys_temp);
            on_log(&format!(
                "Resolved SYSTEM CurrentControlSet -> {control_set_name} (for {} service change(s))",
                changes.len()
            ));

            let access = REG_SAM_FLAGS(KEY_QUERY_VALUE.0 | KEY_SET_VALUE.0);
            for change in changes {
                if is_cancelled() {
                    return Err("Operation cancelled by user.".to_string());
                }
                let subkey = format!("{control_set_name}\\Services\\{}", change.name);
                let Some(key) = open_subkey_no_links(&sys_temp, &subkey, access)? else {
                    on_log(&format!(
                        "Service '{}' not found in mounted edition; skipping.",
                        change.name
                    ));
                    report.skipped_missing += 1;
                    continue;
                };

                let Some(service_type) = query_dword_value(key.0, "Type") else {
                    on_log(&format!(
                        "Registry key '{}' has no service Type value; skipping.",
                        change.name
                    ));
                    report.skipped_non_win32 += 1;
                    continue;
                };
                if !is_manageable_win32_service_type(service_type) {
                    on_log(&format!(
                        "Entry '{}' is not a manageable Win32 service (Type=0x{service_type:x}); skipping.",
                        change.name
                    ));
                    report.skipped_non_win32 += 1;
                    continue;
                }

                let (start_dword, set_delayed) = change.start_type.to_registry_values();
                let start_w = to_wide_null("Start");
                let start_bytes = start_dword.to_le_bytes();
                let err = unsafe {
                    RegSetValueExW(
                        key.0,
                        PCWSTR(start_w.as_ptr()),
                        Some(0),
                        REG_DWORD,
                        Some(&start_bytes),
                    )
                };
                if err != ERROR_SUCCESS {
                    return Err(format!(
                        "RegSetValueExW failed for service '{}\\Start': WIN32_ERROR({})",
                        change.name, err.0
                    ));
                }

                let delayed_w = to_wide_null("DelayedAutostart");
                if set_delayed {
                    let one_bytes = 1u32.to_le_bytes();
                    let derr = unsafe {
                        RegSetValueExW(
                            key.0,
                            PCWSTR(delayed_w.as_ptr()),
                            Some(0),
                            REG_DWORD,
                            Some(&one_bytes),
                        )
                    };
                    if derr != ERROR_SUCCESS {
                        return Err(format!(
                            "RegSetValueExW failed for service '{}\\DelayedAutostart': WIN32_ERROR({})",
                            change.name, derr.0
                        ));
                    }
                } else {
                    let derr = unsafe { RegDeleteValueW(key.0, PCWSTR(delayed_w.as_ptr())) };
                    if derr != ERROR_SUCCESS && derr != ERROR_FILE_NOT_FOUND {
                        return Err(format!(
                            "RegDeleteValueW failed for service '{}\\DelayedAutostart': WIN32_ERROR({})",
                            change.name, derr.0
                        ));
                    }
                }

                report.applied += 1;
                on_log(&format!(
                    "Service '{}' -> {} (Start={start_dword}, DelayedAutostart={set_delayed})",
                    change.name,
                    change.start_type.label()
                ));
            }
            Ok(())
        })();

        // Guarantee loaded SYSTEM hive is closed and unloaded BEFORE unmount_wim runs
        let unload_res = session.unload_all(&mut on_log);
        apply_res?;
        unload_res?;
        Ok(report)
    }
}

#[cfg(windows)]
pub use win32_impl::{
    apply_reg_files_to_mount, apply_reg_files_to_mount_cancellable,
    apply_reg_ops_to_mount_cancellable, apply_service_changes_to_mount_cancellable,
    enable_backup_restore_privileges, read_offline_services_from_mount, unload_hive_by_name,
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
pub fn apply_reg_ops_to_mount_cancellable<F, C>(
    _mount_dir: &Path,
    _workspace_dir: &Path,
    _operations: &[ParsedRegOp],
    _is_cancelled: C,
    _on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    Err("Offline registry operations are only supported on Windows.".to_string())
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

#[cfg(not(windows))]
pub fn read_offline_services_from_mount<F, C>(
    _mount_dir: &Path,
    _inspect_dir: &Path,
    _workspace_dir: &Path,
    _is_cancelled: C,
    _on_log: F,
) -> Result<Vec<RawOfflineService>, String>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    Err("Offline registry operations are only supported on Windows.".to_string())
}

#[cfg(not(windows))]
pub fn apply_service_changes_to_mount_cancellable<F, C>(
    _mount_dir: &Path,
    _workspace_dir: &Path,
    _changes: &[ServiceStartChange],
    _is_cancelled: C,
    _on_log: F,
) -> Result<ServiceApplyReport, String>
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

    #[test]
    fn blocks_protected_root_deletions_and_allows_subkeys() {
        assert!(is_protected_root_delete(OfflineHive::Software, ""));
        assert!(is_protected_root_delete(OfflineHive::Software, "Classes"));
        assert!(is_protected_root_delete(
            OfflineHive::Software,
            "\\classes\\"
        ));
        assert!(is_protected_root_delete(OfflineHive::System, "Select"));
        assert!(is_protected_root_delete(
            OfflineHive::System,
            "ControlSet001"
        ));
        assert!(is_protected_root_delete(
            OfflineHive::System,
            "controlset002"
        ));

        assert!(!is_protected_root_delete(
            OfflineHive::Software,
            r"Classes\MyCustomExt"
        ));
        assert!(!is_protected_root_delete(
            OfflineHive::System,
            r"ControlSet001\Services\Foo"
        ));
        assert!(!is_protected_root_delete(
            OfflineHive::DefaultUser,
            r"Software\MyApp"
        ));
    }
}
