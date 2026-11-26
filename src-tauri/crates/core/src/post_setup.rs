use crate::fs_utils::{
    copy_file_cancellable, is_path_within_dir, is_symlink_or_reparse, is_unc_or_device_path,
    read_file_bounded,
};
use crate::image::decode_text_bytes;
use crate::iso::{validate_autounattend_bytes, validate_autounattend_with_bytes};
use crate::reg_parser::{OfflineHive, ParsedOpKind, ParsedRegOp, RegValueData};
use crate::registry::apply_reg_ops_to_mount_cancellable;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_AUTOUNATTEND_BYTES: u64 = 16 * 1024 * 1024;
const BEFORE_LOGON_CALL_MARKER: &str = r#"ISOCraft\before_logon.cmd"#;
const RUNONCE_VALUE_NAME: &str = "ISOCraftPostSetup";
const RUNONCE_COMMAND: &str = r#"cmd.exe /c "C:\ProgramData\ISOCraft\PostSetup\first_logon.cmd""#;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PostSetupPhase {
    BeforeLogon,
    FirstLogon,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirstLogonMode {
    #[default]
    RunOnce,
    Autounattend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PostSetupItemKind {
    Installer,
    Script,
    Winget,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WingetScope {
    #[default]
    Default,
    Machine,
    User,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WingetSource {
    #[default]
    Winget,
    Msstore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSetupItem {
    #[serde(default)]
    pub id: String,
    pub kind: PostSetupItemKind,
    pub phase: PostSetupPhase,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub winget_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub winget_source: WingetSource,
    #[serde(default)]
    pub winget_scope: WingetScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostSetupBuildPlan {
    pub items: Vec<PostSetupItem>,
    pub first_logon_mode: FirstLogonMode,
    #[serde(default = "default_true")]
    pub cleanup_temp_files: bool,
    #[serde(default = "default_true")]
    pub wait_for_network: bool,
}

impl Default for PostSetupBuildPlan {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            first_logon_mode: FirstLogonMode::RunOnce,
            cleanup_temp_files: true,
            wait_for_network: true,
        }
    }
}

impl PostSetupBuildPlan {
    pub fn has_enabled_items(&self) -> bool {
        self.items.iter().any(|i| i.enabled)
    }

    pub fn has_enabled_before_logon(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.enabled && i.phase == PostSetupPhase::BeforeLogon)
    }

    pub fn has_enabled_first_logon(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.enabled && i.phase == PostSetupPhase::FirstLogon)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSetupFileScanResult {
    pub path: String,
    pub file_name: String,
    pub kind: PostSetupItemKind,
    pub extension: String,
    pub size_bytes: u64,
    pub suggested_args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSetupPreview {
    pub setup_complete_snippet: Option<String>,
    pub before_logon_cmd: Option<String>,
    pub first_logon_cmd: Option<String>,
    pub first_logon_ps1: Option<String>,
    pub run_once_reg_entry: Option<String>,
    pub autounattend_snippet: Option<String>,
}

pub fn validate_winget_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("Winget package ID must not be empty.".to_string());
    }
    if trimmed.len() > 128 {
        return Err(format!("Winget package ID '{trimmed}' is too long."));
    }
    if trimmed.starts_with('-') {
        return Err(format!(
            "Invalid Winget package ID '{trimmed}': must not start with '-'."
        ));
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
    {
        return Err(format!(
            "Invalid Winget package ID '{trimmed}': only letters, digits, '.', '_', '+', '-' are allowed."
        ));
    }
    Ok(())
}

pub fn validate_args_safe(args: &str) -> Result<(), String> {
    if args.contains(['\0', '\r', '\n']) {
        return Err("Post-Setup arguments must not contain newline or NUL characters.".to_string());
    }
    Ok(())
}

pub fn scan_post_setup_file(path: &Path) -> Result<PostSetupFileScanResult, String> {
    let raw = path.to_string_lossy();
    if raw.trim().is_empty() {
        return Err("Post-Setup file path must not be empty.".to_string());
    }
    if is_unc_or_device_path(path) {
        return Err(format!(
            "UNC network or device paths are not allowed for '{}'.",
            path.display()
        ));
    }
    let meta = fs::symlink_metadata(path)
        .map_err(|_| format!("Post-Setup file does not exist: '{}'", path.display()))?;
    if is_symlink_or_reparse(&meta) {
        return Err(format!(
            "Symbolic links and junctions are not supported for Post-Setup files: '{}'",
            path.display()
        ));
    }
    if !meta.is_file() {
        return Err(format!(
            "Post-Setup item must be a regular file: '{}'",
            path.display()
        ));
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let (kind, suggested_args) = match ext.as_str() {
        "msi" => (PostSetupItemKind::Installer, "/qn /norestart".to_string()),
        "exe" => (PostSetupItemKind::Installer, String::new()),
        "ps1" | "cmd" | "bat" => (PostSetupItemKind::Script, String::new()),
        _ => {
            return Err(format!(
                "Unsupported Post-Setup file '{}'. Expected .exe, .msi, .ps1, .cmd, or .bat.",
                path.display()
            ));
        }
    };

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("payload")
        .to_string();

    Ok(PostSetupFileScanResult {
        path: raw.to_string(),
        file_name,
        kind,
        extension: ext,
        size_bytes: meta.len(),
        suggested_args,
    })
}

pub fn validate_post_setup_plan(
    plan: &PostSetupBuildPlan,
    session_dir: Option<&Path>,
) -> Result<(), String> {
    for item in &plan.items {
        if !item.enabled {
            continue;
        }
        validate_args_safe(&item.args)?;
        match item.kind {
            PostSetupItemKind::Winget => {
                if item.phase != PostSetupPhase::FirstLogon {
                    return Err(
                        "Winget packages can only run during the First Logon (User) phase."
                            .to_string(),
                    );
                }
                let id = item.winget_id.as_deref().unwrap_or("");
                validate_winget_id(id)?;
            }
            PostSetupItemKind::Installer | PostSetupItemKind::Script => {
                let raw = item.source_path.as_deref().map(str::trim).unwrap_or("");
                if raw.is_empty() {
                    return Err("Post-Setup local file path must not be empty.".to_string());
                }
                let p = Path::new(raw);
                if is_unc_or_device_path(p) {
                    return Err(format!(
                        "Network/UNC or Windows device paths are not allowed for Post-Setup file: '{}'",
                        p.display()
                    ));
                }
                if let Some(sess) = session_dir
                    && is_path_within_dir(p, sess)
                {
                    return Err(format!(
                        "Post-Setup file cannot reside inside temporary workspace session_current: '{}'",
                        p.display()
                    ));
                }
                let scanned = scan_post_setup_file(p)?;
                if scanned.kind != item.kind {
                    return Err(format!(
                        "Post-Setup file '{}' extension does not match item type.",
                        p.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn sanitize_payload_filename(raw_name: &str) -> String {
    let cleaned: String = raw_name
        .trim()
        .chars()
        .map(|c| {
            if matches!(
                c,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '%' | '&' | '^' | '$' | '`'
            ) || (c as u32) <= 0x1F
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.is_empty() {
        "payload.bin".to_string()
    } else {
        cleaned
    }
}

fn payload_name_for_item(index: usize, item: &PostSetupItem) -> String {
    let base = item
        .source_path
        .as_deref()
        .and_then(|p| Path::new(p.trim()).file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("payload.bin");
    format!("{:03}_{}", index + 1, sanitize_payload_filename(base))
}

fn file_extension_lower(path_str: &str) -> String {
    Path::new(path_str.trim())
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

fn ps_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn format_winget_command(item: &PostSetupItem) -> String {
    let id = item.winget_id.as_deref().unwrap_or("").trim();
    let mut parts = vec![
        "winget".to_string(),
        "install".to_string(),
        "--id".to_string(),
        id.to_string(),
        "--exact".to_string(),
        "--silent".to_string(),
        "--accept-package-agreements".to_string(),
        "--accept-source-agreements".to_string(),
    ];
    parts.push("--source".to_string());
    match item.winget_source {
        WingetSource::Winget => parts.push("winget".to_string()),
        WingetSource::Msstore => parts.push("msstore".to_string()),
    }
    match item.winget_scope {
        WingetScope::Default => {}
        WingetScope::Machine => {
            parts.push("--scope".to_string());
            parts.push("machine".to_string());
        }
        WingetScope::User => {
            parts.push("--scope".to_string());
            parts.push("user".to_string());
        }
    }
    let extra = item.args.trim();
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    parts.join(" ")
}

pub fn generate_before_logon_cmd(plan: &PostSetupBuildPlan) -> Option<String> {
    let active: Vec<(usize, &PostSetupItem)> = plan
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.enabled && i.phase == PostSetupPhase::BeforeLogon)
        .collect();
    if active.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = vec![
        "@echo off".to_string(),
        "chcp 65001 >nul".to_string(),
        r#"set "ISOCRAFT_LOG=%SystemRoot%\Setup\ISOCraft\before_logon.log""#.to_string(),
        r#"echo [%date% %time%] Starting ISOCraft Before-Logon Post-Setup > "%ISOCRAFT_LOG%""#
            .to_string(),
    ];

    for (idx, item) in active {
        let Some(src) = item.source_path.as_deref() else {
            continue;
        };
        let payload_name = payload_name_for_item(idx, item);
        let ext = file_extension_lower(src);
        let args = item.args.trim();
        let args_suffix = if args.is_empty() {
            String::new()
        } else {
            format!(" {args}")
        };

        lines.push(format!(
            r#"echo [%date% %time%] Running {payload_name}{args_suffix} >> "%ISOCRAFT_LOG%""#
        ));

        match ext.as_str() {
            "msi" => lines.push(format!(
                r#"msiexec.exe /i "%~dp0Payload\{payload_name}"{args_suffix} >> "%ISOCRAFT_LOG%" 2>&1"#
            )),
            "exe" => lines.push(format!(
                r#"start "" /wait "%~dp0Payload\{payload_name}"{args_suffix}"#
            )),
            "ps1" => lines.push(format!(
                r#"powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Payload\{payload_name}"{args_suffix} >> "%ISOCRAFT_LOG%" 2>&1"#
            )),
            _ => lines.push(format!(
                r#"call "%~dp0Payload\{payload_name}"{args_suffix} >> "%ISOCRAFT_LOG%" 2>&1"#
            )),
        }
    }

    if plan.cleanup_temp_files {
        lines.push(r#"if exist "%~dp0Payload" rmdir /s /q "%~dp0Payload""#.to_string());
    }
    lines.push(
        r#"echo [%date% %time%] Finished ISOCraft Before-Logon Post-Setup >> "%ISOCRAFT_LOG%""#
            .to_string(),
    );

    Some(format!("{}\r\n", lines.join("\r\n")))
}

pub fn build_setup_complete_content(existing: Option<&str>) -> String {
    let call_line = r#"call "%SystemRoot%\Setup\ISOCraft\before_logon.cmd""#;
    match existing {
        Some(raw) if !raw.trim().is_empty() => {
            let clean = raw
                .trim_start_matches('\u{feff}')
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            if clean
                .to_ascii_lowercase()
                .contains(&BEFORE_LOGON_CALL_MARKER.to_ascii_lowercase())
            {
                let mut normalized = clean.replace('\n', "\r\n");
                if !normalized.ends_with("\r\n") {
                    normalized.push_str("\r\n");
                }
                return normalized;
            }
            let trimmed_end = clean.trim_end_matches('\n').replace('\n', "\r\n");
            format!("{trimmed_end}\r\n{call_line}\r\n")
        }
        _ => format!("@echo off\r\nchcp 65001 >nul\r\n{call_line}\r\n"),
    }
}

pub fn generate_first_logon_cmd() -> String {
    [
        "@echo off",
        "chcp 65001 >nul",
        r#"powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0first_logon.ps1""#,
    ]
    .join("\r\n")
        + "\r\n"
}

pub fn generate_first_logon_ps1(plan: &PostSetupBuildPlan) -> Option<String> {
    let active: Vec<(usize, &PostSetupItem)> = plan
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.enabled && i.phase == PostSetupPhase::FirstLogon)
        .collect();
    if active.is_empty() {
        return None;
    }

    let has_winget = active
        .iter()
        .any(|(_, i)| i.kind == PostSetupItemKind::Winget);
    let has_msstore = active.iter().any(|(_, i)| {
        i.kind == PostSetupItemKind::Winget && i.winget_source == WingetSource::Msstore
    });

    let mut lines: Vec<String> = vec![
        "$ErrorActionPreference = 'Continue'".to_string(),
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8".to_string(),
        "$OutputEncoding = [System.Text.Encoding]::UTF8".to_string(),
        "$logDir = Join-Path $env:LOCALAPPDATA 'ISOCraft'".to_string(),
        "New-Item -ItemType Directory -Path $logDir -Force | Out-Null".to_string(),
        "$logFile = Join-Path $logDir 'post_setup_first_logon.log'".to_string(),
        "function Write-Log([string]$msg) {".to_string(),
        "    $ts = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'".to_string(),
        "    Add-Content -LiteralPath $logFile -Value \"[$ts] $msg\" -Encoding UTF8".to_string(),
        "}".to_string(),
        "Write-Log 'Starting ISOCraft First-Logon Post-Setup'".to_string(),
    ];

    if has_winget && plan.wait_for_network {
        lines.extend([
            "for ($i = 0; $i -lt 24; $i++) {".to_string(),
            "    $netOk = [System.Net.NetworkInformation.NetworkInterface]::GetIsNetworkAvailable()"
                .to_string(),
            "    $wingetOk = [bool](Get-Command winget -ErrorAction SilentlyContinue)".to_string(),
            "    if ($netOk -and $wingetOk) { break }".to_string(),
            "    Start-Sleep -Seconds 5".to_string(),
            "}".to_string(),
        ]);
    }

    if has_msstore {
        lines.extend([
            "Write-Log 'Updating Microsoft.DesktopAppInstaller (App Installer) for msstore support...'"
                .to_string(),
            "$wingetTmp = Join-Path $env:TEMP 'ISOCraft_AppInstaller'".to_string(),
            "try {".to_string(),
            "    New-Item -ItemType Directory -Path $wingetTmp -Force | Out-Null".to_string(),
            "    $curlExe = Join-Path $env:SystemRoot 'System32\\curl.exe'".to_string(),
            "    $tarExe = Join-Path $env:SystemRoot 'System32\\tar.exe'".to_string(),
            "    $bundlePath = Join-Path $wingetTmp 'Microsoft.DesktopAppInstaller.msixbundle'"
                .to_string(),
            "    $depsZip = Join-Path $wingetTmp 'DesktopAppInstaller_Dependencies.zip'"
                .to_string(),
            "    $depsDir = Join-Path $wingetTmp 'deps'".to_string(),
            "    & $curlExe -sSL --fail --proto =https --proto-redir =https --connect-timeout 15 --max-time 180 -o $bundlePath 'https://aka.ms/getwinget'"
                .to_string(),
            "    $bundleExit = $LASTEXITCODE".to_string(),
            "    if ($bundleExit -eq 0 -and (Test-Path -LiteralPath $bundlePath) -and (Get-Item -LiteralPath $bundlePath).Length -gt 0) {"
                .to_string(),
            "        $depPaths = @()".to_string(),
            "        & $curlExe -sSL --fail --proto =https --proto-redir =https --connect-timeout 15 --max-time 180 -o $depsZip 'https://aka.ms/getwingetdependencies'"
                .to_string(),
            "        if ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $depsZip) -and (Get-Item -LiteralPath $depsZip).Length -gt 0) {"
                .to_string(),
            "            New-Item -ItemType Directory -Path $depsDir -Force | Out-Null".to_string(),
            "            & $tarExe -xf $depsZip -C $depsDir".to_string(),
            "            $x64Dir = Join-Path $depsDir 'x64'".to_string(),
            "            $searchDir = if (Test-Path -LiteralPath $x64Dir) { $x64Dir } else { $depsDir }"
                .to_string(),
            "            $depPaths = @(Get-ChildItem -LiteralPath $searchDir -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in @('.appx', '.msix') } | Select-Object -ExpandProperty FullName)"
                .to_string(),
            "        }".to_string(),
            "        if ($depPaths.Count -gt 0) {".to_string(),
            "            Add-AppxPackage -Path $bundlePath -DependencyPath $depPaths -ErrorAction Stop"
                .to_string(),
            "        } else {".to_string(),
            "            Add-AppxPackage -Path $bundlePath -ErrorAction Stop".to_string(),
            "        }".to_string(),
            "        Start-Sleep -Seconds 2".to_string(),
            "        $wgVer = (& winget --version 2>&1 | Select-Object -First 1)".to_string(),
            "        Write-Log ('AppInstaller updated: ' + $wgVer)".to_string(),
            "    } else {".to_string(),
            "        Write-Log ('Skipping AppInstaller update: download failed (exit ' + $bundleExit + ')')"
                .to_string(),
            "    }".to_string(),
            "} catch {".to_string(),
            "    Write-Log ('AppInstaller update note: ' + $_.Exception.Message)".to_string(),
            "} finally {".to_string(),
            "    if (Test-Path -LiteralPath $wingetTmp) {".to_string(),
            "        Remove-Item -LiteralPath $wingetTmp -Recurse -Force -ErrorAction SilentlyContinue"
                .to_string(),
            "    }".to_string(),
            "}".to_string(),
        ]);
    }

    for (idx, item) in active {
        match item.kind {
            PostSetupItemKind::Winget => {
                let id = item.winget_id.as_deref().unwrap_or("").trim();
                let source_flag = match item.winget_source {
                    WingetSource::Winget => " --source winget".to_string(),
                    WingetSource::Msstore => " --source msstore".to_string(),
                };
                let scope_flag = match item.winget_scope {
                    WingetScope::Default => String::new(),
                    WingetScope::Machine => " --scope machine".to_string(),
                    WingetScope::User => " --scope user".to_string(),
                };
                let extra = item.args.trim();
                let extra_flag = if extra.is_empty() {
                    String::new()
                } else {
                    format!(" {extra}")
                };
                let cmd_desc = ps_single_quote(&format_winget_command(item));
                lines.extend([
                    format!("Write-Log ('Running: ' + {cmd_desc})"),
                    "try {".to_string(),
                    format!(
                        "    $wingetOut = & winget install --id {} --exact --silent --accept-package-agreements --accept-source-agreements{source_flag}{scope_flag}{extra_flag} 2>&1",
                        ps_single_quote(id)
                    ),
                    "    $wingetExit = $LASTEXITCODE".to_string(),
                    "    $wingetOut | ForEach-Object { $_.ToString() } | Where-Object { $_.Trim() -ne '' -and $_ -notmatch '^\\s*[-\\\\|/]\\s*$' -and $_ -notmatch '[\\u2580-\\u259F]{3,}' } | Add-Content -LiteralPath $logFile -Encoding UTF8".to_string(),
                    "    Write-Log ('Winget exit code: ' + $wingetExit)".to_string(),
                    "} catch {".to_string(),
                    "    Write-Log ('Winget error: ' + $_.Exception.Message)".to_string(),
                    "}".to_string(),
                ]);
            }
            PostSetupItemKind::Installer | PostSetupItemKind::Script => {
                let Some(src) = item.source_path.as_deref() else {
                    continue;
                };
                let payload_name = payload_name_for_item(idx, item);
                let ext = file_extension_lower(src);
                let args = item.args.trim();
                let q_name = ps_single_quote(&payload_name);
                let q_args = ps_single_quote(args);

                lines.extend([
                    format!("$itemPath = Join-Path (Join-Path $PSScriptRoot 'Payload') {q_name}"),
                    format!("Write-Log ('Executing ' + {q_name})"),
                    "try {".to_string(),
                ]);

                match ext.as_str() {
                    "msi" => {
                        if args.is_empty() {
                            lines.push(
                                "    Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', ('\"' + $itemPath + '\"')) -Wait -NoNewWindow"
                                    .to_string(),
                            );
                        } else {
                            lines.push(format!(
                                "    Start-Process -FilePath 'msiexec.exe' -ArgumentList ('/i \"' + $itemPath + '\" ' + {q_args}) -Wait -NoNewWindow"
                            ));
                        }
                    }
                    "exe" => {
                        if args.is_empty() {
                            lines.push("    Start-Process -FilePath $itemPath -Wait".to_string());
                        } else {
                            lines.push(format!(
                                "    Start-Process -FilePath $itemPath -ArgumentList {q_args} -Wait"
                            ));
                        }
                    }
                    "ps1" => {
                        if args.is_empty() {
                            lines.push(
                                "    Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('\"' + $itemPath + '\"')) -Wait -NoNewWindow"
                                    .to_string(),
                            );
                        } else {
                            lines.push(format!(
                                "    Start-Process -FilePath 'powershell.exe' -ArgumentList ('-NoProfile -ExecutionPolicy Bypass -File \"' + $itemPath + '\" ' + {q_args}) -Wait -NoNewWindow"
                            ));
                        }
                    }
                    _ => {
                        if args.is_empty() {
                            lines.push(
                                "    Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', ('\"' + $itemPath + '\"')) -Wait -NoNewWindow"
                                    .to_string(),
                            );
                        } else {
                            lines.push(format!(
                                "    Start-Process -FilePath 'cmd.exe' -ArgumentList ('/c \"\"' + $itemPath + '\" ' + {q_args} + '\"') -Wait -NoNewWindow"
                            ));
                        }
                    }
                }

                lines.extend([
                    "} catch {".to_string(),
                    "    Write-Log ('Execution error: ' + $_.Exception.Message)".to_string(),
                    "}".to_string(),
                ]);
            }
        }
    }

    if plan.cleanup_temp_files {
        lines.extend([
            "$payloadDir = Join-Path $PSScriptRoot 'Payload'".to_string(),
            "if (Test-Path -LiteralPath $payloadDir) {".to_string(),
            "    Remove-Item -LiteralPath $payloadDir -Recurse -Force -ErrorAction SilentlyContinue"
                .to_string(),
            "}".to_string(),
        ]);
    }

    lines.push("Write-Log 'Finished ISOCraft First-Logon Post-Setup'".to_string());

    // Windows PowerShell 5.1 requires UTF-8 BOM + CRLF for non-ASCII characters
    Some(format!("\u{feff}{}\r\n", lines.join("\r\n")))
}

fn find_max_order_in_first_logon(fragment: &str) -> u32 {
    let mut reader = Reader::from_str(fragment);
    let mut in_order = false;
    let mut max_order = 0u32;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_string();
                let local = name.rsplit(':').next().unwrap_or(&name);
                in_order = local.eq_ignore_ascii_case("Order");
            }
            Ok(Event::Text(t)) if in_order => {
                if let Ok(val) = t.as_ref().trim().parse::<u32>() {
                    max_order = max_order.max(val);
                }
            }
            Ok(Event::End(_)) => {
                in_order = false;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    max_order
}

fn ensure_wcm_namespace(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_string();
                let local = name.rsplit(':').next().unwrap_or(&name);
                if local.eq_ignore_ascii_case("unattend") {
                    let has_wcm = e
                        .attributes()
                        .flatten()
                        .any(|a| a.key.as_ref().eq_ignore_ascii_case("xmlns:wcm"));
                    if has_wcm {
                        return xml.to_string();
                    }
                    let pos_after = reader.buffer_position() as usize;
                    let slice = &xml[pos_before..pos_after];
                    if let Some(insert_rel) = slice.rfind("/>").or_else(|| slice.rfind('>')) {
                        let insert_at = pos_before + insert_rel;
                        let mut out = String::with_capacity(xml.len() + 64);
                        out.push_str(&xml[..insert_at]);
                        out.push_str(
                            r#" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State""#,
                        );
                        out.push_str(&xml[insert_at..]);
                        return out;
                    }
                }
                break;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    xml.to_string()
}

pub fn merge_first_logon_into_autounattend(existing_xml: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let Some(raw_bytes) = existing_xml else {
        let generated = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
<unattend xmlns=\"urn:schemas-microsoft-com:unattend\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\r\n\
  <settings pass=\"oobeSystem\">\r\n\
    <component name=\"Microsoft-Windows-Shell-Setup\" processorArchitecture=\"amd64\" publicKeyToken=\"31bf3856ad364e35\" language=\"neutral\" versionScope=\"nonSxS\">\r\n\
      <FirstLogonCommands>\r\n\
        <SynchronousCommand wcm:action=\"add\">\r\n\
          <Order>1</Order>\r\n\
          <CommandLine>{RUNONCE_COMMAND}</CommandLine>\r\n\
          <Description>ISOCraft Post-Setup (First Logon)</Description>\r\n\
        </SynchronousCommand>\r\n\
      </FirstLogonCommands>\r\n\
    </component>\r\n\
  </settings>\r\n\
</unattend>\r\n"
        );
        return Ok(generated.into_bytes());
    };

    let validation = validate_autounattend_bytes(raw_bytes);
    if !validation.valid {
        return Err(format!(
            "Invalid autounattend.xml: {}",
            validation
                .error
                .unwrap_or_else(|| "unknown XML error".to_string())
        ));
    }

    let decoded = decode_text_bytes(raw_bytes);
    let content = ensure_wcm_namespace(&decoded);

    #[derive(Clone, Copy)]
    struct TagSpan {
        start_open: usize,
        end_open: usize,
        start_close: usize,
        end_close: usize,
        is_empty: bool,
    }

    struct StackEntry {
        local_lower: String,
        start_open: usize,
        end_open: usize,
        is_oobe_settings: bool,
        is_shell_setup: bool,
    }

    let mut stack: Vec<StackEntry> = Vec::new();
    let mut unattend_span: Option<TagSpan> = None;
    let mut oobe_span: Option<TagSpan> = None;
    let mut shell_setup_span: Option<TagSpan> = None;
    let mut first_logon_span: Option<TagSpan> = None;

    let mut reader = Reader::from_str(&content);
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let pos_after = reader.buffer_position() as usize;
                let name = e.name().as_ref().to_string();
                let local_lower = name
                    .rsplit(':')
                    .next()
                    .unwrap_or(&name)
                    .to_ascii_lowercase();

                let mut is_oobe_settings = false;
                let mut is_shell_setup = false;
                if local_lower == "settings" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().eq_ignore_ascii_case("pass")
                            && attr.value.trim().eq_ignore_ascii_case("oobeSystem")
                        {
                            is_oobe_settings = true;
                        }
                    }
                } else if local_lower == "component"
                    && stack.last().map(|s| s.is_oobe_settings).unwrap_or(false)
                {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().eq_ignore_ascii_case("name")
                            && attr
                                .value
                                .trim()
                                .eq_ignore_ascii_case("Microsoft-Windows-Shell-Setup")
                        {
                            is_shell_setup = true;
                        }
                    }
                }

                stack.push(StackEntry {
                    local_lower,
                    start_open: pos_before,
                    end_open: pos_after,
                    is_oobe_settings,
                    is_shell_setup,
                });
            }
            Ok(Event::Empty(e)) => {
                let pos_after = reader.buffer_position() as usize;
                let name = e.name().as_ref().to_string();
                let local_lower = name
                    .rsplit(':')
                    .next()
                    .unwrap_or(&name)
                    .to_ascii_lowercase();

                let span = TagSpan {
                    start_open: pos_before,
                    end_open: pos_after,
                    start_close: pos_before,
                    end_close: pos_after,
                    is_empty: true,
                };
                if local_lower == "unattend" && unattend_span.is_none() {
                    unattend_span = Some(span);
                } else if local_lower == "settings"
                    && oobe_span.is_none()
                    && e.attributes().flatten().any(|a| {
                        a.key.as_ref().eq_ignore_ascii_case("pass")
                            && a.value.trim().eq_ignore_ascii_case("oobeSystem")
                    })
                {
                    oobe_span = Some(span);
                } else if local_lower == "component"
                    && shell_setup_span.is_none()
                    && stack.last().map(|s| s.is_oobe_settings).unwrap_or(false)
                    && e.attributes().flatten().any(|a| {
                        a.key.as_ref().eq_ignore_ascii_case("name")
                            && a.value
                                .trim()
                                .eq_ignore_ascii_case("Microsoft-Windows-Shell-Setup")
                    })
                {
                    shell_setup_span = Some(span);
                } else if local_lower == "firstlogoncommands"
                    && first_logon_span.is_none()
                    && stack.last().map(|s| s.is_shell_setup).unwrap_or(false)
                {
                    first_logon_span = Some(span);
                }
            }
            Ok(Event::End(_)) => {
                let pos_after = reader.buffer_position() as usize;
                if let Some(entry) = stack.pop() {
                    let span = TagSpan {
                        start_open: entry.start_open,
                        end_open: entry.end_open,
                        start_close: pos_before,
                        end_close: pos_after,
                        is_empty: false,
                    };
                    if entry.local_lower == "unattend" && unattend_span.is_none() {
                        unattend_span = Some(span);
                    } else if entry.is_oobe_settings && oobe_span.is_none() {
                        oobe_span = Some(span);
                    } else if entry.is_shell_setup && shell_setup_span.is_none() {
                        shell_setup_span = Some(span);
                    } else if entry.local_lower == "firstlogoncommands"
                        && first_logon_span.is_none()
                        && stack.last().map(|s| s.is_shell_setup).unwrap_or(false)
                    {
                        first_logon_span = Some(span);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Invalid autounattend.xml: {e}")),
            _ => {}
        }
    }

    let make_sync_cmd = |order: u32| -> String {
        format!(
            "        <SynchronousCommand wcm:action=\"add\">\r\n\
          <Order>{order}</Order>\r\n\
          <CommandLine>{RUNONCE_COMMAND}</CommandLine>\r\n\
          <Description>ISOCraft Post-Setup (First Logon)</Description>\r\n\
        </SynchronousCommand>"
        )
    };

    let merged = if let Some(span) = first_logon_span {
        if span.is_empty {
            let open_tag = content[span.start_open..span.end_open]
                .trim_end_matches("/>")
                .to_string()
                + ">";
            let inner = make_sync_cmd(1);
            format!(
                "{}{open_tag}\r\n{inner}\r\n      </FirstLogonCommands>{}",
                &content[..span.start_open],
                &content[span.end_close..]
            )
        } else {
            let existing_inner = &content[span.end_open..span.start_close];
            let next_order = find_max_order_in_first_logon(existing_inner) + 1;
            let insert_block = format!("\r\n{}\r\n      ", make_sync_cmd(next_order));
            format!(
                "{}{insert_block}{}",
                &content[..span.start_close],
                &content[span.start_close..]
            )
        }
    } else if let Some(span) = shell_setup_span {
        let block = format!(
            "\r\n      <FirstLogonCommands>\r\n{}\r\n      </FirstLogonCommands>\r\n    ",
            make_sync_cmd(1)
        );
        if span.is_empty {
            let open_tag = content[span.start_open..span.end_open]
                .trim_end_matches("/>")
                .to_string()
                + ">";
            format!(
                "{}{open_tag}{block}</component>{}",
                &content[..span.start_open],
                &content[span.end_close..]
            )
        } else {
            format!(
                "{}{block}{}",
                &content[..span.start_close],
                &content[span.start_close..]
            )
        }
    } else if let Some(span) = oobe_span {
        let block = format!(
            "\r\n    <component name=\"Microsoft-Windows-Shell-Setup\" processorArchitecture=\"amd64\" publicKeyToken=\"31bf3856ad364e35\" language=\"neutral\" versionScope=\"nonSxS\">\r\n\
      <FirstLogonCommands>\r\n{}\r\n      </FirstLogonCommands>\r\n\
    </component>\r\n  ",
            make_sync_cmd(1)
        );
        if span.is_empty {
            let open_tag = content[span.start_open..span.end_open]
                .trim_end_matches("/>")
                .to_string()
                + ">";
            format!(
                "{}{open_tag}{block}</settings>{}",
                &content[..span.start_open],
                &content[span.end_close..]
            )
        } else {
            format!(
                "{}{block}{}",
                &content[..span.start_close],
                &content[span.start_close..]
            )
        }
    } else if let Some(span) = unattend_span {
        let block = format!(
            "\r\n  <settings pass=\"oobeSystem\">\r\n\
    <component name=\"Microsoft-Windows-Shell-Setup\" processorArchitecture=\"amd64\" publicKeyToken=\"31bf3856ad364e35\" language=\"neutral\" versionScope=\"nonSxS\">\r\n\
      <FirstLogonCommands>\r\n{}\r\n      </FirstLogonCommands>\r\n\
    </component>\r\n\
  </settings>\r\n",
            make_sync_cmd(1)
        );
        if span.is_empty {
            let open_tag = content[span.start_open..span.end_open]
                .trim_end_matches("/>")
                .to_string()
                + ">";
            format!(
                "{}{open_tag}{block}</unattend>{}",
                &content[..span.start_open],
                &content[span.end_close..]
            )
        } else {
            format!(
                "{}{block}{}",
                &content[..span.start_close],
                &content[span.start_close..]
            )
        }
    } else {
        return Err("Expected root XML element <unattend>.".to_string());
    };

    let merged_bytes = merged.into_bytes();
    let recheck = validate_autounattend_bytes(&merged_bytes);
    if !recheck.valid {
        return Err(format!(
            "Generated autounattend.xml failed validation: {}",
            recheck.error.unwrap_or_default()
        ));
    }
    Ok(merged_bytes)
}

pub fn preview_post_setup_scripts(
    plan: &PostSetupBuildPlan,
    autounattend_path: Option<&Path>,
    target: &str,
) -> Result<PostSetupPreview, String> {
    let before_logon_cmd = generate_before_logon_cmd(plan);
    let setup_complete_snippet = before_logon_cmd
        .as_ref()
        .map(|_| build_setup_complete_content(None));

    let first_logon_ps1 = generate_first_logon_ps1(plan);
    let first_logon_cmd = first_logon_ps1.as_ref().map(|_| generate_first_logon_cmd());

    let has_first = first_logon_ps1.is_some();
    let run_once_reg_entry = if has_first && plan.first_logon_mode == FirstLogonMode::RunOnce {
        Some(format!(
            "[HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce] (Users\\Default\\NTUSER.DAT)\r\n\"{RUNONCE_VALUE_NAME}\"=\"{}\"",
            RUNONCE_COMMAND.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    } else {
        None
    };

    let autounattend_snippet =
        if has_first && plan.first_logon_mode == FirstLogonMode::Autounattend && target == "iso" {
            let existing = if let Some(xml_p) = autounattend_path {
                let (v, b) = validate_autounattend_with_bytes(xml_p);
                if !v.valid {
                    return Err(format!(
                        "Invalid autounattend.xml: {}",
                        v.error.unwrap_or_else(|| "unknown XML error".to_string())
                    ));
                }
                b
            } else {
                None
            };
            let merged = merge_first_logon_into_autounattend(existing.as_deref())?;
            Some(String::from_utf8_lossy(&merged).to_string())
        } else {
            None
        };

    Ok(PostSetupPreview {
        setup_complete_snippet,
        before_logon_cmd,
        first_logon_cmd,
        first_logon_ps1,
        run_once_reg_entry,
        autounattend_snippet,
    })
}

fn ensure_clean_dir_no_reparse(mount_dir: &Path, rel_dir: &Path) -> Result<PathBuf, String> {
    let mut cur = mount_dir.to_path_buf();
    for comp in rel_dir.components() {
        cur.push(comp);
        if let Ok(meta) = fs::symlink_metadata(&cur) {
            if is_symlink_or_reparse(&meta) {
                return Err(format!(
                    "Refusing to write Post-Setup files through symlink or junction: '{}'",
                    cur.display()
                ));
            }
            if !meta.is_dir() {
                return Err(format!(
                    "Post-Setup target path collides with existing file: '{}'",
                    cur.display()
                ));
            }
        } else {
            fs::create_dir(&cur).map_err(|e| {
                format!(
                    "Failed creating Post-Setup directory '{}': {e}",
                    cur.display()
                )
            })?;
        }
    }
    Ok(cur)
}

pub fn apply_post_setup_to_mount_cancellable<F, C>(
    mount_dir: &Path,
    workspace_dir: &Path,
    plan: &PostSetupBuildPlan,
    is_cancelled: C,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    if !plan.has_enabled_items() {
        return Ok(());
    }
    validate_post_setup_plan(plan, None)?;

    let mut copied_before = 0usize;
    let mut copied_first = 0usize;

    let before_payload_dir = if plan.has_enabled_before_logon() {
        Some(ensure_clean_dir_no_reparse(
            mount_dir,
            Path::new(r"Windows\Setup\ISOCraft\Payload"),
        )?)
    } else {
        None
    };
    let first_payload_dir = if plan.has_enabled_first_logon() {
        Some(ensure_clean_dir_no_reparse(
            mount_dir,
            Path::new(r"ProgramData\ISOCraft\PostSetup\Payload"),
        )?)
    } else {
        None
    };

    for (idx, item) in plan.items.iter().enumerate() {
        if !item.enabled {
            continue;
        }
        if is_cancelled() {
            return Err("Operation cancelled by user.".to_string());
        }
        if matches!(
            item.kind,
            PostSetupItemKind::Installer | PostSetupItemKind::Script
        ) && let Some(src_raw) = item.source_path.as_deref()
        {
            let src = Path::new(src_raw.trim());
            let payload_name = payload_name_for_item(idx, item);
            let dest = match item.phase {
                PostSetupPhase::BeforeLogon => {
                    copied_before += 1;
                    before_payload_dir
                        .as_ref()
                        .expect("before payload dir created")
                        .join(&payload_name)
                }
                PostSetupPhase::FirstLogon => {
                    copied_first += 1;
                    first_payload_dir
                        .as_ref()
                        .expect("first payload dir created")
                        .join(&payload_name)
                }
            };
            copy_file_cancellable(src, &dest, &is_cancelled)?;
            on_log(&format!(
                "Staged Post-Setup payload ({:?}): {} -> {}",
                item.phase,
                src.display(),
                dest.display()
            ));
        }
    }

    if let Some(before_cmd) = generate_before_logon_cmd(plan) {
        let isocraft_setup_dir =
            ensure_clean_dir_no_reparse(mount_dir, Path::new(r"Windows\Setup\ISOCraft"))?;
        let before_path = isocraft_setup_dir.join("before_logon.cmd");
        fs::write(&before_path, before_cmd.as_bytes())
            .map_err(|e| format!("Failed writing '{}': {e}", before_path.display()))?;

        let scripts_dir =
            ensure_clean_dir_no_reparse(mount_dir, Path::new(r"Windows\Setup\Scripts"))?;
        let setup_complete_path = scripts_dir.join("SetupComplete.cmd");
        let existing_text = if setup_complete_path.is_file() {
            let bytes = read_file_bounded(&setup_complete_path, MAX_AUTOUNATTEND_BYTES, true)?;
            Some(decode_text_bytes(&bytes))
        } else {
            None
        };
        let final_setup_complete = build_setup_complete_content(existing_text.as_deref());
        fs::write(&setup_complete_path, final_setup_complete.as_bytes())
            .map_err(|e| format!("Failed writing '{}': {e}", setup_complete_path.display()))?;
        on_log(&format!(
            "Configured Windows\\Setup\\Scripts\\SetupComplete.cmd (payload files: {copied_before})."
        ));
    }

    if let Some(first_ps1) = generate_first_logon_ps1(plan) {
        let first_dir =
            ensure_clean_dir_no_reparse(mount_dir, Path::new(r"ProgramData\ISOCraft\PostSetup"))?;
        let ps1_path = first_dir.join("first_logon.ps1");
        let cmd_path = first_dir.join("first_logon.cmd");

        fs::write(&ps1_path, first_ps1.as_bytes())
            .map_err(|e| format!("Failed writing '{}': {e}", ps1_path.display()))?;
        fs::write(&cmd_path, generate_first_logon_cmd().as_bytes())
            .map_err(|e| format!("Failed writing '{}': {e}", cmd_path.display()))?;

        if plan.first_logon_mode == FirstLogonMode::RunOnce {
            let run_once_op = ParsedRegOp {
                line: 1,
                kind: ParsedOpKind::SetValue,
                original_key:
                    r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\RunOnce"
                        .to_string(),
                hive: OfflineHive::DefaultUser,
                subkey: r"Software\Microsoft\Windows\CurrentVersion\RunOnce".to_string(),
                uses_current_control_set: false,
                value_name: Some(RUNONCE_VALUE_NAME.to_string()),
                value_data: Some(RegValueData::Sz(RUNONCE_COMMAND.to_string())),
            };
            apply_reg_ops_to_mount_cancellable(
                mount_dir,
                workspace_dir,
                &[run_once_op],
                &is_cancelled,
                &mut on_log,
            )?;
            on_log(&format!(
                "Registered First-Logon RunOnce in Default User NTUSER.DAT (payload files: {copied_first})."
            ));
        } else {
            on_log(&format!(
                "Staged First-Logon scripts for autounattend.xml FirstLogonCommands (payload files: {copied_first})."
            ));
        }
    }

    Ok(())
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
    fn validates_winget_ids_and_safe_args() {
        assert!(validate_winget_id("7zip.7zip").is_ok());
        assert!(validate_winget_id("Notepad++.Notepad++").is_ok());
        assert!(validate_winget_id("9WZDNCRFJ3TJ").is_ok());
        assert!(validate_winget_id("").is_err());
        assert!(validate_winget_id("-bad").is_err());
        assert!(validate_winget_id("foo; calc.exe").is_err());
        assert!(validate_winget_id("foo & calc").is_err());

        assert!(validate_args_safe("/qn /norestart").is_ok());
        assert!(validate_args_safe("/S\r\ncalc.exe").is_err());
        assert!(validate_args_safe("a\0b").is_err());
    }

    #[test]
    fn generates_crlf_cmd_without_bom_and_ps1_with_utf8_bom() {
        let dir = make_temp_dir("isocraft_ps_gen");
        let msi = dir.join("setup.msi");
        let exe = dir.join("установщик.exe");
        fs::write(&msi, b"msi").unwrap();
        fs::write(&exe, b"exe").unwrap();

        let plan = PostSetupBuildPlan {
            items: vec![
                PostSetupItem {
                    id: "1".into(),
                    kind: PostSetupItemKind::Installer,
                    phase: PostSetupPhase::BeforeLogon,
                    enabled: true,
                    source_path: Some(msi.to_string_lossy().to_string()),
                    args: "/qn /norestart".into(),
                    winget_id: None,
                    display_name: None,
                    winget_source: WingetSource::Winget,
                    winget_scope: WingetScope::Default,
                },
                PostSetupItem {
                    id: "2".into(),
                    kind: PostSetupItemKind::Installer,
                    phase: PostSetupPhase::FirstLogon,
                    enabled: true,
                    source_path: Some(exe.to_string_lossy().to_string()),
                    args: "/S".into(),
                    winget_id: None,
                    display_name: None,
                    winget_source: WingetSource::Winget,
                    winget_scope: WingetScope::Default,
                },
                PostSetupItem {
                    id: "3".into(),
                    kind: PostSetupItemKind::Winget,
                    phase: PostSetupPhase::FirstLogon,
                    enabled: true,
                    source_path: None,
                    args: String::new(),
                    winget_id: Some("7zip.7zip".into()),
                    display_name: Some("7-Zip".into()),
                    winget_source: WingetSource::Winget,
                    winget_scope: WingetScope::Machine,
                },
            ],
            first_logon_mode: FirstLogonMode::RunOnce,
            cleanup_temp_files: true,
            wait_for_network: true,
        };

        let before_cmd = generate_before_logon_cmd(&plan).unwrap();
        assert!(!before_cmd.starts_with('\u{feff}'));
        assert!(before_cmd.contains("chcp 65001 >nul\r\n"));
        assert!(
            before_cmd.contains(r#"msiexec.exe /i "%~dp0Payload\001_setup.msi" /qn /norestart"#)
        );
        assert!(before_cmd.contains(r#"rmdir /s /q "%~dp0Payload""#));

        let first_ps1 = generate_first_logon_ps1(&plan).unwrap();
        assert!(first_ps1.starts_with('\u{feff}'));
        assert!(first_ps1.contains("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8"));
        assert!(first_ps1.contains("002_установщик.exe"));
        assert!(first_ps1.contains("winget install --id '7zip.7zip' --exact --silent"));
        assert!(first_ps1.contains("--source winget"));
        assert!(first_ps1.contains("--scope machine"));
        assert!(!first_ps1.contains("aka.ms/getwinget"));
        assert!(!first_ps1.contains("*>> $logFile"));
        assert!(first_ps1.contains("Winget exit code:"));
        assert!(first_ps1.contains("Remove-Item -LiteralPath $payloadDir"));
        assert!(format_winget_command(&plan.items[2]).contains("--source winget"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn emits_appinstaller_update_only_when_msstore_item_enabled() {
        let disabled_plan = PostSetupBuildPlan {
            items: vec![PostSetupItem {
                id: "1".into(),
                kind: PostSetupItemKind::Winget,
                phase: PostSetupPhase::FirstLogon,
                enabled: false,
                source_path: None,
                args: String::new(),
                winget_id: Some("9NKSQGP7F2NH".into()),
                display_name: Some("WhatsApp".into()),
                winget_source: WingetSource::Msstore,
                winget_scope: WingetScope::Default,
            }],
            first_logon_mode: FirstLogonMode::RunOnce,
            cleanup_temp_files: true,
            wait_for_network: true,
        };
        assert!(generate_first_logon_ps1(&disabled_plan).is_none());

        let enabled_plan = PostSetupBuildPlan {
            items: vec![PostSetupItem {
                id: "1".into(),
                kind: PostSetupItemKind::Winget,
                phase: PostSetupPhase::FirstLogon,
                enabled: true,
                source_path: None,
                args: String::new(),
                winget_id: Some("9NKSQGP7F2NH".into()),
                display_name: Some("WhatsApp".into()),
                winget_source: WingetSource::Msstore,
                winget_scope: WingetScope::Default,
            }],
            first_logon_mode: FirstLogonMode::RunOnce,
            cleanup_temp_files: true,
            wait_for_network: true,
        };
        let ps1 = generate_first_logon_ps1(&enabled_plan).unwrap();
        let update_pos = ps1
            .find("https://aka.ms/getwinget")
            .expect("expected aka.ms/getwinget download");
        let install_pos = ps1
            .find("winget install --id '9NKSQGP7F2NH'")
            .expect("expected msstore winget install");
        assert!(update_pos < install_pos);
        assert!(ps1.contains("https://aka.ms/getwingetdependencies"));
        assert!(ps1.contains("Add-AppxPackage"));
        assert!(ps1.contains("-DependencyPath"));
        assert!(ps1.contains("--source msstore"));
    }

    #[test]
    fn appends_setup_complete_idempotently() {
        let fresh = build_setup_complete_content(None);
        assert!(fresh.contains("before_logon.cmd"));
        assert!(!fresh.starts_with('\u{feff}'));

        let custom = "@echo off\r\necho Custom user step\r\n";
        let appended = build_setup_complete_content(Some(custom));
        assert!(appended.contains("echo Custom user step"));
        assert!(appended.contains("before_logon.cmd"));

        let again = build_setup_complete_content(Some(&appended));
        assert_eq!(again.matches("before_logon.cmd").count(), 1);
    }

    #[test]
    fn merges_first_logon_commands_into_autounattend_variants() {
        // 1. No existing XML -> creates minimal valid unattend
        let fresh = merge_first_logon_into_autounattend(None).unwrap();
        let fresh_val = validate_autounattend_bytes(&fresh);
        assert!(fresh_val.valid);
        assert_eq!(fresh_val.passes, vec!["oobeSystem"]);
        assert!(String::from_utf8_lossy(&fresh).contains("<Order>1</Order>"));

        // 2. Existing XML without oobeSystem
        let no_oobe = br#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="windowsPE"></settings>
</unattend>"#;
        let merged2 = merge_first_logon_into_autounattend(Some(no_oobe)).unwrap();
        let val2 = validate_autounattend_bytes(&merged2);
        assert!(val2.valid);
        assert!(val2.passes.contains(&"oobeSystem".to_string()));
        assert!(String::from_utf8_lossy(&merged2).contains("xmlns:wcm="));

        // 3. Existing FirstLogonCommands with Order 2 -> adds Order 3
        let with_cmd = br#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <FirstLogonCommands>
        <SynchronousCommand wcm:action="add">
          <Order>2</Order>
          <CommandLine>cmd /c echo hi</CommandLine>
        </SynchronousCommand>
      </FirstLogonCommands>
    </component>
  </settings>
</unattend>"#;
        let merged3 = merge_first_logon_into_autounattend(Some(with_cmd)).unwrap();
        let val3 = validate_autounattend_bytes(&merged3);
        assert!(val3.valid);
        let txt3 = String::from_utf8_lossy(&merged3);
        assert!(txt3.contains("<Order>3</Order>"));
        assert!(txt3.contains("first_logon.cmd"));
    }

    #[test]
    fn stages_payload_and_scripts_in_mount_without_overwriting_user_setup_complete() {
        let dir = make_temp_dir("isocraft_ps_stage");
        let mount = dir.join("mount");
        let ws = dir.join("work");
        fs::create_dir_all(mount.join(r"Windows\Setup\Scripts")).unwrap();
        fs::create_dir_all(&ws).unwrap();
        fs::write(
            mount.join(r"Windows\Setup\Scripts\SetupComplete.cmd"),
            "@echo off\r\necho User Script\r\n",
        )
        .unwrap();

        let bat = dir.join("hello.cmd");
        fs::write(&bat, "@echo off\r\necho Hello\r\n").unwrap();

        let plan = PostSetupBuildPlan {
            items: vec![PostSetupItem {
                id: "1".into(),
                kind: PostSetupItemKind::Script,
                phase: PostSetupPhase::BeforeLogon,
                enabled: true,
                source_path: Some(bat.to_string_lossy().to_string()),
                args: String::new(),
                winget_id: None,
                display_name: None,
                winget_source: WingetSource::Winget,
                winget_scope: WingetScope::Default,
            }],
            first_logon_mode: FirstLogonMode::RunOnce,
            cleanup_temp_files: true,
            wait_for_network: true,
        };

        apply_post_setup_to_mount_cancellable(&mount, &ws, &plan, || false, |_| {}).unwrap();

        let sc =
            fs::read_to_string(mount.join(r"Windows\Setup\Scripts\SetupComplete.cmd")).unwrap();
        assert!(sc.contains("echo User Script"));
        assert!(sc.contains("before_logon.cmd"));
        assert!(
            mount
                .join(r"Windows\Setup\ISOCraft\before_logon.cmd")
                .is_file()
        );
        assert!(
            mount
                .join(r"Windows\Setup\ISOCraft\Payload\001_hello.cmd")
                .is_file()
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
