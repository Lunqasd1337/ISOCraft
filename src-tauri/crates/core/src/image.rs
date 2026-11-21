use crate::env::find_powershell;
use crate::process::{run_tracked_command, run_tracked_command_with_env, ProcessGuard};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    // Heuristic for BOM-less UTF-16LE (common in wimlib XML output on Windows)
    if bytes.len() >= 4 && bytes[1] == 0 && bytes[3] == 0 {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
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

pub fn parse_wim_xml(xml: &str) -> Result<Vec<WimEditionInfo>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut editions = Vec::new();
    let mut current: Option<WimEditionInfo> = None;
    let mut display_name = String::new();
    let mut display_desc = String::new();
    let mut major = 0u32;
    let mut minor = 0u32;
    let mut build = 0u32;
    let mut spbuild = 0u32;
    let mut path_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = e.name().as_ref().to_ascii_uppercase();
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
                if let Some(ed) = current.as_mut() {
                    let text = t.as_ref().trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
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
            Ok(Event::End(e)) => {
                let tag = e.name().as_ref().to_ascii_uppercase();
                if tag == "IMAGE" {
                    if let Some(mut ed) = current.take() {
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

pub fn clear_readonly_recursive(root: &Path) -> Result<usize, String> {
    let mut cleared = 0usize;
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("Failed to read metadata for '{}': {e}", path.display()))?;
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

fn copy_dir_recursive<C, P>(
    src: &Path,
    dst: &Path,
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
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];

    while let Some((cur_src, cur_dst)) = stack.pop() {
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
            let src_path = entry.path();
            let dst_path = cur_dst.join(entry.file_name());
            let meta = fs::symlink_metadata(&src_path)
                .map_err(|e| format!("Failed reading metadata '{}': {e}", src_path.display()))?;
            if meta.is_dir() {
                fs::create_dir_all(&dst_path)
                    .map_err(|e| format!("Failed creating dir '{}': {e}", dst_path.display()))?;
                stack.push((src_path, dst_path));
            } else if meta.is_file() {
                fs::copy(&src_path, &dst_path).map_err(|e| {
                    format!(
                        "Failed copying '{}' to '{}': {e}",
                        src_path.display(),
                        dst_path.display()
                    )
                })?;
                total_files += 1;
                if total_files % 50 == 0 {
                    on_progress(total_files);
                }
            }
        }
    }
    on_progress(total_files);
    Ok(total_files)
}

async fn read_wim_xml_with_wimlib(wimlib_path: &Path, image_path: &Path) -> Result<String, String> {
    let mut cmd = Command::new(wimlib_path);
    cmd.args(["info", &image_path.to_string_lossy(), "--xml"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to execute wimlib-imagex: {e}"))?;

    if !output.status.success() {
        let stderr = decode_text_bytes(&output.stderr);
        return Err(format!(
            "wimlib-imagex info failed (code {:?}): {}",
            output.status.code(),
            stderr.trim()
        ));
    }

    Ok(decode_text_bytes(&output.stdout))
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
    let out = run_tracked_command_with_env(&ps_path, &args, None, envs, guard, |_| {}).await?;

    if out.exit_code != 0 {
        return Err(format!(
            "PowerShell failed (code {}): {} {}",
            out.exit_code,
            out.stderr.trim(),
            out.stdout.trim()
        ));
    }

    Ok(out.stdout)
}

pub async fn mount_iso_drive_letter(
    iso_path: &Path,
    guard: Option<&ProcessGuard>,
) -> Result<String, String> {
    let script = "$p = $env:ISOCRAFT_TARGET_PATH; \
         $img = Mount-DiskImage -ImagePath $p -PassThru; \
         $vol = $img | Get-Volume | Where-Object { $_.DriveLetter } | Select-Object -First 1; \
         if (-not $vol) { Dismount-DiskImage -ImagePath $p -ErrorAction SilentlyContinue; throw 'Mounted ISO has no drive letter.' }; \
         Write-Output $vol.DriveLetter";
    let out = run_powershell_script_with_env(
        script,
        &[("ISOCRAFT_TARGET_PATH", iso_path.as_os_str())],
        guard,
    )
    .await?;
    let letter = out
        .trim()
        .chars()
        .next()
        .ok_or("Empty drive letter from Mount-DiskImage")?;
    Ok(format!("{letter}:"))
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
) -> Result<ImageInspectionResult, String> {
    if !source_path.is_file() {
        return Err(format!("Image file not found: {}", source_path.display()));
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
            ))
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
        let xml = read_wim_xml_with_wimlib(wimlib_path, source_path).await?;
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
    let drive = mount_iso_drive_letter(source_path, None).await?;
    let drive_root = PathBuf::from(format!("{drive}\\"));

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
        let xml = read_wim_xml_with_wimlib(wimlib_path, &install_path).await?;
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

    dismount_iso(source_path).await;
    inspect_res
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
    if let Some(g) = guard {
        if g.is_cancel_requested() {
            return Err("Operation cancelled by user.".to_string());
        }
    }

    if dest_iso_root.exists() {
        let _ = clear_readonly_recursive(dest_iso_root);
        fs::remove_dir_all(dest_iso_root)
            .map_err(|e| format!("Failed cleaning existing iso_root: {e}"))?;
    }
    fs::create_dir_all(dest_iso_root).map_err(|e| format!("Failed creating iso_root: {e}"))?;

    on_log(&format!("Mounting ISO: {}", iso_path.display()));
    let drive = mount_iso_drive_letter(iso_path, guard).await?;
    let drive_root = PathBuf::from(format!("{drive}\\"));

    let cancel_flag = guard.map(|g| g.cancel_handle());
    let copy_result = tokio::task::spawn_blocking({
        let src = drive_root.clone();
        let dst = dest_iso_root.to_path_buf();
        move || -> Result<(u64, usize), String> {
            let copied = copy_dir_recursive(
                &src,
                &dst,
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

    dismount_iso(iso_path).await;
    on_log("ISO dismounted.");

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
}
