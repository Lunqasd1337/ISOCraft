use crate::env::find_powershell;
use crate::fs_utils::{
    DEFAULT_COPY_BUFFER_SIZE, copy_file_cancellable_with_buf, is_symlink_or_reparse,
    is_unc_or_device_path,
};
use crate::process::{
    ProcessGuard, run_tracked_command, run_tracked_command_raw, run_tracked_command_raw_with_env,
};
use crate::wim::{export_single_edition, is_swm_part_for_stem};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Iso,
    Wim,
    Esd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WimEditionInfo {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub edition_id: String,
    pub installation_type: String,
    pub architecture: String,
    pub version: String,
    pub build_number: u32,
    pub total_bytes: u64,
    pub languages: Vec<String>,
    pub is_supported: bool,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInspectionResult {
    pub source_path: String,
    pub source_kind: SourceKind,
    pub install_file_format: String,
    pub has_boot_wim: bool,
    pub has_sxs: bool,
    pub total_size_bytes: u64,
    pub editions: Vec<WimEditionInfo>,
    pub is_valid_win11_client_x64: bool,
    pub blocking_reason: Option<String>,
}

pub fn decode_text_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_be_bytes(c))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    // Heuristic for BOM-less UTF-16LE (common in wimlib XML output on Windows)
    let sample_len = bytes.len().min(128);
    if sample_len >= 4 && bytes.len().is_multiple_of(2) {
        let pairs = sample_len / 2;
        let zero_odd = (0..pairs).filter(|&i| bytes[i * 2 + 1] == 0).count();
        let has_ascii_even = (0..pairs).any(|i| bytes[i * 2].is_ascii_graphic());
        if has_ascii_even && zero_odd * 4 >= pairs * 3 {
            let u16s: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_le_bytes(c))
                .collect();
            return String::from_utf16_lossy(&u16s);
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn map_wim_arch(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "0" | "x86" | "i386" => "x86".to_string(),
        "5" | "arm" => "arm".to_string(),
        "6" | "ia64" => "ia64".to_string(),
        "9" | "amd64" | "x64" | "x86_64" => "x64".to_string(),
        "12" | "arm64" | "aarch64" => "arm64".to_string(),
        other => other.to_string(),
    }
}

pub fn validate_edition(edition: &mut WimEditionInfo) {
    if edition.installation_type.eq_ignore_ascii_case("WindowsPE")
        || edition.name.to_ascii_lowercase().contains("windows pe")
        || edition.name.to_ascii_lowercase().contains("windows setup")
    {
        edition.is_supported = false;
        edition.rejection_reason = Some("boot_wim".to_string());
        return;
    }

    if edition.architecture != "x64" {
        edition.is_supported = false;
        edition.rejection_reason = Some("not_x64".to_string());
        return;
    }

    if !edition.installation_type.eq_ignore_ascii_case("Client") {
        edition.is_supported = false;
        edition.rejection_reason = Some("not_client".to_string());
        return;
    }

    if edition.build_number < 22000 {
        edition.is_supported = false;
        edition.rejection_reason = Some("not_win11".to_string());
        return;
    }

    edition.is_supported = true;
    edition.rejection_reason = None;
}

fn decode_xml_entity(raw: &str) -> String {
    match raw {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        _ => {
            if let Some(hex) = raw.strip_prefix("#x").or_else(|| raw.strip_prefix("#X")) {
                if let Ok(code) = u32::from_str_radix(hex, 16)
                    && let Some(ch) = char::from_u32(code)
                {
                    return ch.to_string();
                }
            } else if let Some(dec) = raw.strip_prefix('#')
                && let Ok(code) = dec.parse::<u32>()
                && let Some(ch) = char::from_u32(code)
            {
                return ch.to_string();
            }
            format!("&{raw};")
        }
    }
}

pub fn parse_wim_xml(xml: &str) -> Result<Vec<WimEditionInfo>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut editions = Vec::new();
    let mut current: Option<WimEditionInfo> = None;
    let mut display_name = String::new();
    let mut display_desc = String::new();
    let mut major = 0u32;
    let mut minor = 0u32;
    let mut build = 0u32;
    let mut spbuild = 0u32;
    let mut path_stack: Vec<String> = Vec::new();
    let mut current_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = e.name().as_ref().to_ascii_uppercase();
                current_text.clear();
                if tag == "IMAGE" {
                    let mut idx = 0u32;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().eq_ignore_ascii_case("INDEX") {
                            idx = attr.value.trim().parse::<u32>().unwrap_or(0);
                        }
                    }
                    current = Some(WimEditionInfo {
                        index: idx,
                        name: String::new(),
                        description: String::new(),
                        edition_id: String::new(),
                        installation_type: String::new(),
                        architecture: String::new(),
                        version: String::new(),
                        build_number: 0,
                        total_bytes: 0,
                        languages: Vec::new(),
                        is_supported: false,
                        rejection_reason: None,
                    });
                    display_name.clear();
                    display_desc.clear();
                    major = 0;
                    minor = 0;
                    build = 0;
                    spbuild = 0;
                }
                path_stack.push(tag);
            }
            Ok(Event::Text(t)) => {
                if current.is_some() {
                    current_text.push_str(t.as_ref());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if current.is_some() {
                    current_text.push_str(&decode_xml_entity(r.as_ref()));
                }
            }
            Ok(Event::End(e)) => {
                if let Some(ed) = current.as_mut() {
                    let text = current_text.trim().to_string();
                    if !text.is_empty() {
                        let path = path_stack.join("/");
                        match path.as_str() {
                            "WIM/IMAGE/NAME" => ed.name = text,
                            "WIM/IMAGE/DESCRIPTION" => ed.description = text,
                            "WIM/IMAGE/DISPLAYNAME" => display_name = text,
                            "WIM/IMAGE/DISPLAYDESCRIPTION" => display_desc = text,
                            "WIM/IMAGE/TOTALBYTES" => {
                                ed.total_bytes = text.parse::<u64>().unwrap_or(0);
                            }
                            "WIM/IMAGE/WINDOWS/ARCH" => {
                                ed.architecture = map_wim_arch(&text);
                            }
                            "WIM/IMAGE/WINDOWS/EDITIONID" => ed.edition_id = text,
                            "WIM/IMAGE/WINDOWS/INSTALLATIONTYPE" => ed.installation_type = text,
                            "WIM/IMAGE/WINDOWS/LANGUAGES/LANGUAGE" => {
                                if !ed.languages.contains(&text) {
                                    ed.languages.push(text);
                                }
                            }
                            "WIM/IMAGE/WINDOWS/VERSION/MAJOR" => {
                                major = text.parse::<u32>().unwrap_or(0);
                            }
                            "WIM/IMAGE/WINDOWS/VERSION/MINOR" => {
                                minor = text.parse::<u32>().unwrap_or(0);
                            }
                            "WIM/IMAGE/WINDOWS/VERSION/BUILD" => {
                                build = text.parse::<u32>().unwrap_or(0);
                            }
                            "WIM/IMAGE/WINDOWS/VERSION/SPBUILD" => {
                                spbuild = text.parse::<u32>().unwrap_or(0);
                            }
                            _ => {}
                        }
                    }
                }
                current_text.clear();

                let tag = e.name().as_ref().to_ascii_uppercase();
                if tag == "IMAGE"
                    && let Some(mut ed) = current.take()
                {
                    if !display_name.is_empty() {
                        ed.name = display_name.clone();
                    }
                    if !display_desc.is_empty() {
                        ed.description = display_desc.clone();
                    }
                    ed.build_number = build;
                    ed.version = if spbuild > 0 {
                        format!("{major}.{minor}.{build}.{spbuild}")
                    } else if build > 0 {
                        format!("{major}.{minor}.{build}")
                    } else {
                        String::new()
                    };
                    validate_edition(&mut ed);
                    editions.push(ed);
                }
                path_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Failed to parse WIM XML metadata: {e}")),
            _ => {}
        }
    }

    if editions.is_empty() {
        return Err("No image indices found in WIM/ESD metadata.".to_string());
    }
    Ok(editions)
}

#[allow(clippy::permissions_set_readonly_false)]
pub fn clear_readonly_recursive(root: &Path) -> Result<usize, String> {
    let mut cleared = 0usize;
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("Failed to read metadata for '{}': {e}", path.display()))?;
        if is_symlink_or_reparse(&meta) {
            continue;
        }
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            fs::set_permissions(&path, perms)
                .map_err(|e| format!("Failed to clear ReadOnly on '{}': {e}", path.display()))?;
            cleared += 1;
        }
        if meta.is_dir() {
            let entries = fs::read_dir(&path)
                .map_err(|e| format!("Failed to read directory '{}': {e}", path.display()))?;
            for entry in entries.flatten() {
                stack.push(entry.path());
            }
        }
    }

    Ok(cleared)
}

fn should_skip_iso_install_payload(rel_path: &Path) -> bool {
    let mut comps = rel_path.components();
    let Some(first) = comps.next() else {
        return false;
    };
    let Some(second) = comps.next() else {
        return false;
    };
    if comps.next().is_some() {
        return false;
    }
    if !first
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case("sources")
    {
        return false;
    }
    let file_lower = second.as_os_str().to_string_lossy().to_ascii_lowercase();
    file_lower == "install.wim"
        || file_lower == "install.esd"
        || is_swm_part_for_stem(&file_lower, "install")
}

fn copy_dir_recursive<C, P>(
    src: &Path,
    dst: &Path,
    skip_install_payload: bool,
    is_cancelled: C,
    mut on_progress: P,
) -> Result<u64, String>
where
    C: Fn() -> bool,
    P: FnMut(u64),
{
    fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create directory '{}': {e}", dst.display()))?;
    let mut total_files = 0u64;
    let mut copy_buf = vec![0u8; DEFAULT_COPY_BUFFER_SIZE];
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf(), PathBuf::new())];

    while let Some((cur_src, cur_dst, cur_rel)) = stack.pop() {
        if is_cancelled() {
            return Err("Operation cancelled by user.".to_string());
        }
        let entries = fs::read_dir(&cur_src)
            .map_err(|e| format!("Failed to list '{}': {e}", cur_src.display()))?;
        for entry in entries {
            if is_cancelled() {
                return Err("Operation cancelled by user.".to_string());
            }
            let entry = entry.map_err(|e| format!("Failed reading dir entry: {e}"))?;
            let file_name = entry.file_name();
            let next_rel = cur_rel.join(&file_name);
            if skip_install_payload && should_skip_iso_install_payload(&next_rel) {
                continue;
            }
            let src_path = entry.path();
            let dst_path = cur_dst.join(&file_name);
            let meta = fs::symlink_metadata(&src_path)
                .map_err(|e| format!("Failed reading metadata '{}': {e}", src_path.display()))?;
            if is_symlink_or_reparse(&meta) {
                continue;
            }
            if meta.is_dir() {
                fs::create_dir_all(&dst_path)
                    .map_err(|e| format!("Failed creating dir '{}': {e}", dst_path.display()))?;
                stack.push((src_path, dst_path, next_rel));
            } else if meta.is_file() {
                copy_file_cancellable_with_buf(&src_path, &dst_path, &mut copy_buf, &is_cancelled)?;
                total_files += 1;
                if total_files.is_multiple_of(50) {
                    on_progress(total_files);
                }
            }
        }
    }
    on_progress(total_files);
    Ok(total_files)
}

async fn read_wim_xml_with_wimlib(
    wimlib_path: &Path,
    image_path: &Path,
    guard: Option<&ProcessGuard>,
) -> Result<String, String> {
    let args = vec![
        "info".to_string(),
        image_path.to_string_lossy().to_string(),
        "--xml".to_string(),
    ];
    let out = run_tracked_command_raw(&wimlib_path.to_string_lossy(), &args, None, guard).await?;

    if out.exit_code != 0 {
        let stderr = decode_text_bytes(&out.stderr);
        return Err(format!(
            "wimlib-imagex info failed (code {}): {}",
            out.exit_code,
            stderr.trim()
        ));
    }

    if out.stdout_truncated {
        return Err("wimlib-imagex XML metadata exceeded the 32 MiB capture limit.".to_string());
    }

    Ok(decode_text_bytes(&out.stdout))
}

pub fn escape_ps_single_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => {
                out.push(ch);
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

pub async fn run_powershell_script(script: &str) -> Result<String, String> {
    run_powershell_script_tracked(script, None).await
}

pub async fn run_powershell_script_tracked(
    script: &str,
    guard: Option<&ProcessGuard>,
) -> Result<String, String> {
    run_powershell_script_with_env(script, &[], guard).await
}

pub async fn run_powershell_script_with_env(
    script: &str,
    envs: &[(&str, &std::ffi::OsStr)],
    guard: Option<&ProcessGuard>,
) -> Result<String, String> {
    let ps_path = find_powershell()
        .path
        .ok_or_else(|| "powershell.exe not found in Windows system directory.".to_string())?;
    let full_script = format!(
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); $OutputEncoding = [System.Text.UTF8Encoding]::new($false); {script}"
    );
    let args = vec![
        "-NoLogo".to_string(),
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-Command".to_string(),
        full_script,
    ];
    let out = run_tracked_command_raw_with_env(&ps_path, &args, None, envs, guard).await?;

    if out.exit_code != 0 {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(format!(
            "PowerShell failed (code {}): {} {}",
            out.exit_code,
            stderr.trim(),
            stdout.trim()
        ));
    }

    if out.stdout_truncated {
        return Err("PowerShell output exceeded the 32 MiB capture limit.".to_string());
    }

    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedIso {
    pub drive_letter: String,
    pub was_already_mounted: bool,
}

pub fn parse_mounted_iso_marker(stdout: &str) -> Result<MountedIso, String> {
    for line in stdout.lines().rev() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("ISOCRAFT_ISO:")
            && let Some((attached_flag, letter_raw)) = rest.split_once(':')
        {
            let letter = letter_raw.trim();
            let mut chars = letter.chars();
            if let (Some(ch), None) = (chars.next(), chars.next())
                && ch.is_ascii_alphabetic()
                && (attached_flag == "0" || attached_flag == "1")
            {
                return Ok(MountedIso {
                    drive_letter: format!("{}:", ch.to_ascii_uppercase()),
                    was_already_mounted: attached_flag == "1",
                });
            }
        }
    }
    Err("Failed to determine mounted ISO drive letter from PowerShell output.".to_string())
}

pub async fn mount_iso_drive_letter(
    iso_path: &Path,
    guard: Option<&ProcessGuard>,
) -> Result<MountedIso, String> {
    let script = "$p = $env:ISOCRAFT_TARGET_PATH; \
         $existing = Get-DiskImage -ImagePath $p -ErrorAction SilentlyContinue; \
         $wasAttached = [bool]($existing -and $existing.Attached); \
         $img = if ($wasAttached) { $existing } else { Mount-DiskImage -ImagePath $p -PassThru }; \
         $vol = $img | Get-Volume | Where-Object { $_.DriveLetter } | Select-Object -First 1; \
         if (-not $vol) { if (-not $wasAttached) { Dismount-DiskImage -ImagePath $p -ErrorAction SilentlyContinue }; throw 'Mounted ISO has no drive letter.' }; \
         $flag = if ($wasAttached) { '1' } else { '0' }; \
         Write-Output (\"ISOCRAFT_ISO:\" + $flag + \":\" + $vol.DriveLetter)";
    let out = match run_powershell_script_with_env(
        script,
        &[("ISOCRAFT_TARGET_PATH", iso_path.as_os_str())],
        guard,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            dismount_iso(iso_path).await;
            return Err(e);
        }
    };
    match parse_mounted_iso_marker(&out) {
        Ok(m) => Ok(m),
        Err(e) => {
            dismount_iso(iso_path).await;
            Err(e)
        }
    }
}

pub async fn dismount_iso(iso_path: &Path) {
    let script = "Dismount-DiskImage -ImagePath $env:ISOCRAFT_TARGET_PATH -ErrorAction SilentlyContinue | Out-Null";
    let _ = run_powershell_script_with_env(
        script,
        &[("ISOCRAFT_TARGET_PATH", iso_path.as_os_str())],
        None,
    )
    .await;
}

pub async fn inspect_image(
    source_path: &Path,
    wimlib_path: &Path,
    guard: Option<&ProcessGuard>,
) -> Result<ImageInspectionResult, String> {
    if guard.is_some_and(|g| g.is_cancel_requested()) {
        return Err("Operation cancelled by user.".to_string());
    }

    if is_unc_or_device_path(source_path) {
        return Err(format!(
            "Source image must be a regular non-symlink file: '{}'",
            source_path.display()
        ));
    }

    let meta = fs::symlink_metadata(source_path)
        .map_err(|_| format!("Image file not found: {}", source_path.display()))?;
    if is_symlink_or_reparse(&meta) || !meta.is_file() {
        return Err(format!(
            "Source image must be a regular non-symlink file: '{}'",
            source_path.display()
        ));
    }

    let file_name_lower = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let source_kind = match ext.as_str() {
        "iso" => SourceKind::Iso,
        "wim" => SourceKind::Wim,
        "esd" => SourceKind::Esd,
        _ => {
            return Err(format!(
                "Unsupported image extension '.{ext}'. Expected .iso, .wim, or .esd."
            ));
        }
    };

    let total_size_bytes = fs::metadata(source_path).map(|m| m.len()).unwrap_or(0);

    if file_name_lower == "boot.wim" {
        return Ok(ImageInspectionResult {
            source_path: source_path.to_string_lossy().to_string(),
            source_kind,
            install_file_format: "wim".to_string(),
            has_boot_wim: false,
            has_sxs: false,
            total_size_bytes,
            editions: Vec::new(),
            is_valid_win11_client_x64: false,
            blocking_reason: Some("boot_wim".to_string()),
        });
    }

    if source_kind != SourceKind::Iso {
        let xml = read_wim_xml_with_wimlib(wimlib_path, source_path, guard).await?;
        let editions = parse_wim_xml(&xml)?;
        let valid = editions.iter().any(|e| e.is_supported);
        let blocking_reason = if valid {
            None
        } else {
            editions
                .first()
                .and_then(|e| e.rejection_reason.clone())
                .or_else(|| Some("unsupported_image".to_string()))
        };

        return Ok(ImageInspectionResult {
            source_path: source_path.to_string_lossy().to_string(),
            source_kind: source_kind.clone(),
            install_file_format: if source_kind == SourceKind::Esd {
                "esd".to_string()
            } else {
                "wim".to_string()
            },
            has_boot_wim: false,
            has_sxs: false,
            total_size_bytes,
            editions,
            is_valid_win11_client_x64: valid,
            blocking_reason,
        });
    }

    // ISO flow: mount, inspect sources\install.wim / install.esd, always dismount
    let mounted = mount_iso_drive_letter(source_path, guard).await?;
    let drive_root = PathBuf::from(format!("{}\\", mounted.drive_letter));

    let inspect_res = async {
        let install_wim = drive_root.join("sources").join("install.wim");
        let install_esd = drive_root.join("sources").join("install.esd");
        let (install_path, format) = if install_wim.is_file() {
            (install_wim, "wim".to_string())
        } else if install_esd.is_file() {
            (install_esd, "esd".to_string())
        } else {
            return Ok(ImageInspectionResult {
                source_path: source_path.to_string_lossy().to_string(),
                source_kind: SourceKind::Iso,
                install_file_format: "unknown".to_string(),
                has_boot_wim: drive_root.join("sources").join("boot.wim").is_file(),
                has_sxs: drive_root.join("sources").join("sxs").is_dir(),
                total_size_bytes,
                editions: Vec::new(),
                is_valid_win11_client_x64: false,
                blocking_reason: Some("missing_install_image".to_string()),
            });
        };

        let has_boot_wim = drive_root.join("sources").join("boot.wim").is_file();
        let has_sxs = drive_root.join("sources").join("sxs").is_dir();
        let xml = read_wim_xml_with_wimlib(wimlib_path, &install_path, guard).await?;
        let editions = parse_wim_xml(&xml)?;
        let valid = editions.iter().any(|e| e.is_supported);
        let blocking_reason = if valid {
            None
        } else {
            editions
                .first()
                .and_then(|e| e.rejection_reason.clone())
                .or_else(|| Some("unsupported_image".to_string()))
        };

        Ok(ImageInspectionResult {
            source_path: source_path.to_string_lossy().to_string(),
            source_kind: SourceKind::Iso,
            install_file_format: format,
            has_boot_wim,
            has_sxs,
            total_size_bytes,
            editions,
            is_valid_win11_client_x64: valid,
            blocking_reason,
        })
    }
    .await;

    if !mounted.was_already_mounted {
        dismount_iso(source_path).await;
    }
    inspect_res
}

async fn reset_iso_root_dir(dest_iso_root: &Path) -> Result<(), String> {
    let dir = dest_iso_root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if dir.exists() {
            let _ = clear_readonly_recursive(&dir);
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Failed cleaning existing iso_root: {e}"))?;
        }
        fs::create_dir_all(&dir).map_err(|e| format!("Failed creating iso_root: {e}"))
    })
    .await
    .map_err(|e| format!("Failed preparing iso_root task: {e}"))?
}

pub async fn extract_iso_to_workspace<F>(
    iso_path: &Path,
    dest_iso_root: &Path,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<PathBuf, String>
where
    F: FnMut(&str) + Send,
{
    if let Some(g) = guard
        && g.is_cancel_requested()
    {
        return Err("Operation cancelled by user.".to_string());
    }

    reset_iso_root_dir(dest_iso_root).await?;

    on_log(&format!("Mounting ISO: {}", iso_path.display()));
    let mounted = mount_iso_drive_letter(iso_path, guard).await?;
    let drive_root = PathBuf::from(format!("{}\\", mounted.drive_letter));

    let cancel_flag = guard.map(|g| g.cancel_handle());
    let copy_result = tokio::task::spawn_blocking({
        let src = drive_root.clone();
        let dst = dest_iso_root.to_path_buf();
        move || -> Result<(u64, usize), String> {
            let copied = copy_dir_recursive(
                &src,
                &dst,
                false,
                || {
                    cancel_flag
                        .as_ref()
                        .map(|f| f.load(Ordering::SeqCst))
                        .unwrap_or(false)
                },
                |_| {},
            )?;
            if cancel_flag
                .as_ref()
                .map(|f| f.load(Ordering::SeqCst))
                .unwrap_or(false)
            {
                return Err("Operation cancelled by user.".to_string());
            }
            let cleared = clear_readonly_recursive(&dst)?;
            Ok((copied, cleared))
        }
    })
    .await
    .map_err(|e| format!("ISO copy task panicked: {e}"))?;

    if !mounted.was_already_mounted {
        dismount_iso(iso_path).await;
        on_log("ISO dismounted.");
    }

    let (copied_files, cleared_ro) = copy_result?;
    on_log(&format!(
        "Extracted {copied_files} files from ISO (cleared ReadOnly on {cleared_ro} items)."
    ));

    let wim = dest_iso_root.join("sources").join("install.wim");
    let esd = dest_iso_root.join("sources").join("install.esd");
    if wim.is_file() {
        Ok(wim)
    } else if esd.is_file() {
        Ok(esd)
    } else {
        Err(
            "Extracted ISO does not contain sources\\install.wim or sources\\install.esd"
                .to_string(),
        )
    }
}

/// Mounts the source ISO once, copies boot/setup files (skipping `sources\install.*` payloads)
/// to `dest_iso_root`, and directly exports `selected_index` from the mounted ISO into `dest_work_wim`
/// before dismounting — avoiding redundant multi-GB disk writes.
#[allow(clippy::too_many_arguments)]
pub async fn extract_iso_and_export_edition<FLog, FExportLog>(
    iso_path: &Path,
    dest_iso_root: &Path,
    wimlib_path: &Path,
    selected_index: u32,
    dest_work_wim: &Path,
    guard: Option<&ProcessGuard>,
    mut on_extract_log: FLog,
    mut on_export_log: FExportLog,
) -> Result<(), String>
where
    FLog: FnMut(&str) + Send,
    FExportLog: FnMut(&str) + Send,
{
    if guard.is_some_and(|g| g.is_cancel_requested()) {
        return Err("Operation cancelled by user.".to_string());
    }

    reset_iso_root_dir(dest_iso_root).await?;

    on_extract_log(&format!("Mounting ISO: {}", iso_path.display()));
    let mounted = mount_iso_drive_letter(iso_path, guard).await?;
    let drive_root = PathBuf::from(format!("{}\\", mounted.drive_letter));

    let res: Result<(), String> = async {
        let cancel_flag = guard.map(|g| g.cancel_handle());
        let copy_result = tokio::task::spawn_blocking({
            let src = drive_root.clone();
            let dst = dest_iso_root.to_path_buf();
            move || -> Result<(u64, usize), String> {
                let copied = copy_dir_recursive(
                    &src,
                    &dst,
                    true,
                    || {
                        cancel_flag
                            .as_ref()
                            .map(|f| f.load(Ordering::SeqCst))
                            .unwrap_or(false)
                    },
                    |_| {},
                )?;
                if cancel_flag
                    .as_ref()
                    .map(|f| f.load(Ordering::SeqCst))
                    .unwrap_or(false)
                {
                    return Err("Operation cancelled by user.".to_string());
                }
                let cleared = clear_readonly_recursive(&dst)?;
                Ok((copied, cleared))
            }
        })
        .await
        .map_err(|e| format!("ISO copy task panicked: {e}"))??;

        on_extract_log(&format!(
            "Extracted {} boot/setup files from ISO (cleared ReadOnly on {} items).",
            copy_result.0, copy_result.1
        ));

        let wim = drive_root.join("sources").join("install.wim");
        let esd = drive_root.join("sources").join("install.esd");
        let src_install = if wim.is_file() {
            wim
        } else if esd.is_file() {
            esd
        } else {
            return Err(
                "Extracted ISO does not contain sources\\install.wim or sources\\install.esd"
                    .to_string(),
            );
        };

        export_single_edition(
            wimlib_path,
            &src_install,
            selected_index,
            dest_work_wim,
            guard,
            |msg| on_export_log(msg),
        )
        .await
    }
    .await;

    if !mounted.was_already_mounted {
        dismount_iso(iso_path).await;
        on_extract_log("ISO dismounted.");
    }

    res
}

/// Helper to run wimlib-imagex with progress tracking when exporting/splitting
pub async fn run_wimlib_command<F>(
    wimlib_path: &Path,
    args: &[String],
    guard: Option<&ProcessGuard>,
    on_line: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let out =
        run_tracked_command(&wimlib_path.to_string_lossy(), args, None, guard, on_line).await?;

    if out.exit_code != 0 {
        return Err(format!(
            "wimlib-imagex failed (exit code {}): {}",
            out.exit_code,
            out.stderr.trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_xml(arch: &str, inst_type: &str, build: &str, name: &str) -> String {
        format!(
            r#"<WIM>
  <TOTALBYTES>5000000000</TOTALBYTES>
  <IMAGE INDEX="1">
    <NAME>{name}</NAME>
    <DESCRIPTION>{name}</DESCRIPTION>
    <DISPLAYNAME>{name}</DISPLAYNAME>
    <TOTALBYTES>18000000000</TOTALBYTES>
    <WINDOWS>
      <ARCH>{arch}</ARCH>
      <EDITIONID>Professional</EDITIONID>
      <INSTALLATIONTYPE>{inst_type}</INSTALLATIONTYPE>
      <LANGUAGES>
        <LANGUAGE>en-US</LANGUAGE>
      </LANGUAGES>
      <VERSION>
        <MAJOR>10</MAJOR>
        <MINOR>0</MINOR>
        <BUILD>{build}</BUILD>
        <SPBUILD>1742</SPBUILD>
      </VERSION>
    </WINDOWS>
  </IMAGE>
</WIM>"#
        )
    }

    #[test]
    fn accepts_win11_client_x64() {
        let xml = sample_xml("9", "Client", "26100", "Windows 11 Pro");
        let eds = parse_wim_xml(&xml).unwrap();
        assert_eq!(eds.len(), 1);
        assert!(eds[0].is_supported);
        assert_eq!(eds[0].architecture, "x64");
        assert_eq!(eds[0].build_number, 26100);
        assert_eq!(eds[0].version, "10.0.26100.1742");
        assert_eq!(eds[0].rejection_reason, None);
    }

    #[test]
    fn rejects_win10_build() {
        let xml = sample_xml("9", "Client", "19045", "Windows 10 Pro");
        let eds = parse_wim_xml(&xml).unwrap();
        assert!(!eds[0].is_supported);
        assert_eq!(eds[0].rejection_reason.as_deref(), Some("not_win11"));
    }

    #[test]
    fn rejects_windows_server() {
        let xml = sample_xml("9", "Server", "26100", "Windows Server 2025");
        let eds = parse_wim_xml(&xml).unwrap();
        assert!(!eds[0].is_supported);
        assert_eq!(eds[0].rejection_reason.as_deref(), Some("not_client"));
    }

    #[test]
    fn rejects_arm64_and_x86() {
        let arm = parse_wim_xml(&sample_xml("12", "Client", "26100", "Win11 ARM64")).unwrap();
        assert!(!arm[0].is_supported);
        assert_eq!(arm[0].rejection_reason.as_deref(), Some("not_x64"));

        let x86 = parse_wim_xml(&sample_xml("0", "Client", "22621", "Win11 x86")).unwrap();
        assert!(!x86[0].is_supported);
        assert_eq!(x86[0].rejection_reason.as_deref(), Some("not_x64"));
    }

    #[test]
    fn rejects_winpe_boot_wim() {
        let xml = sample_xml("9", "WindowsPE", "26100", "Microsoft Windows Setup (x64)");
        let eds = parse_wim_xml(&xml).unwrap();
        assert!(!eds[0].is_supported);
        assert_eq!(eds[0].rejection_reason.as_deref(), Some("boot_wim"));
    }

    #[test]
    fn clears_readonly_attributes_recursively() {
        let dir = std::env::temp_dir().join(format!("w11_ro_test_{}", std::process::id()));
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let file = sub.join("test.txt");
        fs::write(&file, b"hello").unwrap();

        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file, perms).unwrap();
        assert!(fs::metadata(&file).unwrap().permissions().readonly());

        let count = clear_readonly_recursive(&dir).unwrap();
        assert!(count >= 1);
        assert!(!fs::metadata(&file).unwrap().permissions().readonly());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn escapes_powershell_single_quoted_paths_with_spaces_apostrophes_and_cyrillic() {
        let escaped = escape_ps_single_quoted(r"D:\Образы\O'Brien's Win11 x64\install.wim");
        assert_eq!(escaped, r"D:\Образы\O''Brien''s Win11 x64\install.wim");

        let unicode_quotes = escape_ps_single_quoted("a\u{2018}b\u{2019}c\u{201A}d\u{201B}e");
        assert_eq!(
            unicode_quotes,
            "a\u{2018}\u{2018}b\u{2019}\u{2019}c\u{201A}\u{201A}d\u{201B}\u{201B}e"
        );
    }

    #[test]
    fn decodes_xml_entities_inside_wim_edition_names() {
        let xml = sample_xml("9", "Client", "26100", "Windows 11 Pro &amp; Workstation");
        let eds = parse_wim_xml(&xml).unwrap();
        assert_eq!(eds[0].name, "Windows 11 Pro & Workstation");
    }

    #[test]
    fn parses_mounted_iso_marker_strictly_and_detects_text_encodings() {
        let m1 = parse_mounted_iso_marker("WARNING: something\nISOCRAFT_ISO:0:f\n").unwrap();
        assert_eq!(m1.drive_letter, "F:");
        assert!(!m1.was_already_mounted);

        let m2 = parse_mounted_iso_marker("ISOCRAFT_ISO:1:G\r\n").unwrap();
        assert_eq!(m2.drive_letter, "G:");
        assert!(m2.was_already_mounted);

        assert!(parse_mounted_iso_marker("Invalid output").is_err());

        // BOM-less UTF-16LE XML vs binary data
        let xml_u16: Vec<u8> = "<WIM>"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(decode_text_bytes(&xml_u16), "<WIM>");

        let binary_stub = [0x01, 0x00, 0x02, 0x00, 0xFF, 0xFF, 0xAA, 0xBB];
        assert_eq!(
            decode_text_bytes(&binary_stub),
            String::from_utf8_lossy(&binary_stub)
        );
    }
}
