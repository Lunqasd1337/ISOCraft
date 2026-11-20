use crate::image::decode_text_bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const MAX_PREVIEW_OPS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfflineHive {
    Software,
    System,
    Default,
    DefaultUser,
    Security,
    Sam,
}

impl OfflineHive {
    pub fn label(self) -> &'static str {
        match self {
            OfflineHive::Software => "SOFTWARE",
            OfflineHive::System => "SYSTEM",
            OfflineHive::Default => "DEFAULT",
            OfflineHive::DefaultUser => "HKCU → Default User",
            OfflineHive::Security => "SECURITY",
            OfflineHive::Sam => "SAM",
        }
    }

    pub fn temp_suffix(self) -> &'static str {
        match self {
            OfflineHive::Software => "SOFTWARE",
            OfflineHive::System => "SYSTEM",
            OfflineHive::Default => "DEFAULT",
            OfflineHive::DefaultUser => "NTUSER",
            OfflineHive::Security => "SECURITY",
            OfflineHive::Sam => "SAM",
        }
    }

    pub fn relative_hive_path(self) -> &'static str {
        match self {
            OfflineHive::Software => r"Windows\System32\config\SOFTWARE",
            OfflineHive::System => r"Windows\System32\config\SYSTEM",
            OfflineHive::Default => r"Windows\System32\config\DEFAULT",
            OfflineHive::DefaultUser => r"Users\Default\NTUSER.DAT",
            OfflineHive::Security => r"Windows\System32\config\SECURITY",
            OfflineHive::Sam => r"Windows\System32\config\SAM",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValueData {
    Sz(String),
    Dword(u32),
    Qword(u64),
    ExpandSz(Vec<u8>),
    MultiSz(Vec<u8>),
    Binary(Vec<u8>),
    None(Vec<u8>),
    Raw { reg_type: u32, bytes: Vec<u8> },
}

impl RegValueData {
    pub fn reg_type_id(&self) -> u32 {
        match self {
            RegValueData::None(_) => 0,
            RegValueData::Sz(_) => 1,
            RegValueData::ExpandSz(_) => 2,
            RegValueData::Binary(_) => 3,
            RegValueData::Dword(_) => 4,
            RegValueData::MultiSz(_) => 7,
            RegValueData::Qword(_) => 11,
            RegValueData::Raw { reg_type, .. } => *reg_type,
        }
    }

    pub fn type_name(&self) -> String {
        match self {
            RegValueData::None(_) => "REG_NONE".to_string(),
            RegValueData::Sz(_) => "REG_SZ".to_string(),
            RegValueData::ExpandSz(_) => "REG_EXPAND_SZ".to_string(),
            RegValueData::Binary(_) => "REG_BINARY".to_string(),
            RegValueData::Dword(_) => "REG_DWORD".to_string(),
            RegValueData::MultiSz(_) => "REG_MULTI_SZ".to_string(),
            RegValueData::Qword(_) => "REG_QWORD".to_string(),
            RegValueData::Raw { reg_type, .. } => format!("REG_TYPE({reg_type})"),
        }
    }

    pub fn to_win32_bytes(&self) -> Vec<u8> {
        match self {
            RegValueData::Sz(s) => {
                let mut bytes = Vec::with_capacity((s.len() + 1) * 2);
                for code_unit in s.encode_utf16().chain(std::iter::once(0)) {
                    bytes.extend_from_slice(&code_unit.to_le_bytes());
                }
                bytes
            }
            RegValueData::Dword(v) => v.to_le_bytes().to_vec(),
            RegValueData::Qword(v) => v.to_le_bytes().to_vec(),
            RegValueData::ExpandSz(b)
            | RegValueData::MultiSz(b)
            | RegValueData::Binary(b)
            | RegValueData::None(b)
            | RegValueData::Raw { bytes: b, .. } => b.clone(),
        }
    }

    pub fn preview_string(&self) -> String {
        match self {
            RegValueData::Sz(s) => format!("\"{s}\""),
            RegValueData::Dword(v) => format!("0x{v:08x} ({v})"),
            RegValueData::Qword(v) => format!("0x{v:016x} ({v})"),
            RegValueData::ExpandSz(b) => {
                let decoded = decode_utf16le_z(b);
                format!("\"{decoded}\"")
            }
            RegValueData::MultiSz(b) => {
                let parts = decode_utf16le_multi(b);
                format!("[{}]", parts.join(", "))
            }
            RegValueData::Binary(b)
            | RegValueData::None(b)
            | RegValueData::Raw { bytes: b, .. } => {
                if b.is_empty() {
                    "(empty)".to_string()
                } else {
                    let shown: Vec<String> = b
                        .iter()
                        .take(12)
                        .map(|byte| format!("{byte:02x}"))
                        .collect();
                    if b.len() > 12 {
                        format!("{} ... ({} bytes)", shown.join(" "), b.len())
                    } else {
                        shown.join(" ")
                    }
                }
            }
        }
    }
}

fn decode_utf16le_z(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn decode_utf16le_multi(bytes: &[u8]) -> Vec<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut items = Vec::new();
    let mut start = 0usize;
    for (idx, &u) in units.iter().enumerate() {
        if u == 0 {
            if idx > start {
                items.push(String::from_utf16_lossy(&units[start..idx]));
            }
            start = idx + 1;
        }
    }
    if start < units.len() {
        let tail = String::from_utf16_lossy(&units[start..]);
        if !tail.is_empty() {
            items.push(tail);
        }
    }
    items
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedOpKind {
    CreateKey,
    DeleteKey,
    SetValue,
    DeleteValue,
}

#[derive(Debug, Clone)]
pub struct ParsedRegOp {
    pub line: usize,
    pub kind: ParsedOpKind,
    pub original_key: String,
    pub hive: OfflineHive,
    pub subkey: String,
    pub uses_current_control_set: bool,
    pub value_name: Option<String>,
    pub value_data: Option<RegValueData>,
}

impl ParsedRegOp {
    pub fn resolved_subkey(&self, control_set_name: &str) -> String {
        if self.uses_current_control_set {
            if self.subkey.is_empty() {
                control_set_name.to_string()
            } else {
                format!("{control_set_name}\\{}", self.subkey)
            }
        } else {
            self.subkey.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegPreviewOperation {
    pub line: usize,
    pub kind: String,
    pub hive: OfflineHive,
    pub hive_display: String,
    pub target_path: String,
    pub value_name: Option<String>,
    pub value_type: Option<String>,
    pub value_preview: Option<String>,
    pub uses_current_control_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegFilePreview {
    pub path: String,
    pub valid: bool,
    pub total_set: usize,
    pub total_delete: usize,
    pub affected_hives: Vec<String>,
    pub operations: Vec<RegPreviewOperation>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedRegFile {
    pub operations: Vec<ParsedRegOp>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
struct MappedSection {
    original_key: String,
    hive: OfflineHive,
    subkey: String,
    uses_current_control_set: bool,
}

enum KeyMapOutcome {
    Mapped(MappedSection),
    Unsupported(String),
    Invalid(String),
}

fn split_first_component(path: &str) -> (&str, &str) {
    let trimmed = path.trim_matches('\\');
    match trimmed.find('\\') {
        Some(idx) => (&trimmed[..idx], trimmed[idx + 1..].trim_start_matches('\\')),
        None => (trimmed, ""),
    }
}

fn map_offline_key_path(raw_key: &str) -> KeyMapOutcome {
    if raw_key.contains('\0') {
        return KeyMapOutcome::Invalid(
            "Registry key path contains an embedded NUL character".to_string(),
        );
    }
    let trimmed = raw_key.trim();
    if trimmed.is_empty() {
        return KeyMapOutcome::Invalid("Empty registry key path".to_string());
    }

    let (root, after_root) = split_first_component(trimmed);
    let root_upper = root.to_ascii_uppercase();

    match root_upper.as_str() {
        "HKEY_LOCAL_MACHINE" | "HKLM" => {
            if after_root.is_empty() {
                return KeyMapOutcome::Unsupported(trimmed.to_string());
            }
            let (subhive, tail) = split_first_component(after_root);
            match subhive.to_ascii_uppercase().as_str() {
                "SOFTWARE" => KeyMapOutcome::Mapped(MappedSection {
                    original_key: trimmed.to_string(),
                    hive: OfflineHive::Software,
                    subkey: tail.to_string(),
                    uses_current_control_set: false,
                }),
                "SYSTEM" => {
                    let (first_sys, sys_tail) = split_first_component(tail);
                    if first_sys.eq_ignore_ascii_case("CurrentControlSet") {
                        KeyMapOutcome::Mapped(MappedSection {
                            original_key: trimmed.to_string(),
                            hive: OfflineHive::System,
                            subkey: sys_tail.to_string(),
                            uses_current_control_set: true,
                        })
                    } else {
                        KeyMapOutcome::Mapped(MappedSection {
                            original_key: trimmed.to_string(),
                            hive: OfflineHive::System,
                            subkey: tail.to_string(),
                            uses_current_control_set: false,
                        })
                    }
                }
                "SECURITY" => KeyMapOutcome::Mapped(MappedSection {
                    original_key: trimmed.to_string(),
                    hive: OfflineHive::Security,
                    subkey: tail.to_string(),
                    uses_current_control_set: false,
                }),
                "SAM" => KeyMapOutcome::Mapped(MappedSection {
                    original_key: trimmed.to_string(),
                    hive: OfflineHive::Sam,
                    subkey: tail.to_string(),
                    uses_current_control_set: false,
                }),
                _ => KeyMapOutcome::Unsupported(trimmed.to_string()),
            }
        }
        "HKEY_CURRENT_USER" | "HKCU" => KeyMapOutcome::Mapped(MappedSection {
            original_key: trimmed.to_string(),
            hive: OfflineHive::DefaultUser,
            subkey: after_root.to_string(),
            uses_current_control_set: false,
        }),
        "HKEY_CLASSES_ROOT" | "HKCR" => {
            let subkey = if after_root.is_empty() {
                "Classes".to_string()
            } else {
                format!("Classes\\{after_root}")
            };
            KeyMapOutcome::Mapped(MappedSection {
                original_key: trimmed.to_string(),
                hive: OfflineHive::Software,
                subkey,
                uses_current_control_set: false,
            })
        }
        "HKEY_USERS" | "HKU" => {
            if after_root.is_empty() {
                return KeyMapOutcome::Unsupported(trimmed.to_string());
            }
            let (user_root, tail) = split_first_component(after_root);
            if user_root.eq_ignore_ascii_case(".DEFAULT") {
                KeyMapOutcome::Mapped(MappedSection {
                    original_key: trimmed.to_string(),
                    hive: OfflineHive::Default,
                    subkey: tail.to_string(),
                    uses_current_control_set: false,
                })
            } else {
                KeyMapOutcome::Unsupported(trimmed.to_string())
            }
        }
        "HKEY_CURRENT_CONFIG" | "HKCC" => KeyMapOutcome::Unsupported(trimmed.to_string()),
        _ => KeyMapOutcome::Invalid(format!("Unknown registry root '{root}' in '{trimmed}'")),
    }
}

fn unescape_reg_string(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return Err("Dangling escape '\\' inside quoted string".to_string()),
            }
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn parse_quoted_string_literal(s: &str) -> Result<(String, &str), String> {
    if !s.starts_with('"') {
        return Err("Expected opening quote '\"'".to_string());
    }
    let bytes = s.as_bytes();
    let mut idx = 1usize;
    let mut escaped = false;
    while idx < bytes.len() {
        let b = bytes[idx];
        if escaped {
            escaped = false;
            idx += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            idx += 1;
            continue;
        }
        if b == b'"' {
            let inner = &s[1..idx];
            let rest = &s[idx + 1..];
            let unescaped = unescape_reg_string(inner)?;
            return Ok((unescaped, rest));
        }
        idx += 1;
    }
    Err("Unterminated quoted string literal".to_string())
}

fn parse_hex_byte_list(raw: &str) -> Result<Vec<u8>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let mut bytes = Vec::new();
    for part in trimmed.split(',') {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        let byte = u8::from_str_radix(token, 16)
            .map_err(|_| format!("Invalid hex byte '{token}' in hex list"))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn encode_ansi_or_utf8_to_utf16le(segment: &[u8], out: &mut Vec<u8>) {
    if let Ok(s) = std::str::from_utf8(segment) {
        for cu in s.encode_utf16() {
            out.extend_from_slice(&cu.to_le_bytes());
        }
    } else {
        for &b in segment {
            out.extend_from_slice(&u16::from(b).to_le_bytes());
        }
    }
}

fn regedit4_bytes_to_utf16le(bytes: &[u8], double_null: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2 + 4);
    if double_null {
        for seg in bytes.split(|&b| b == 0) {
            if seg.is_empty() {
                continue;
            }
            encode_ansi_or_utf8_to_utf16le(seg, &mut out);
            out.extend_from_slice(&[0, 0]);
        }
        if out.is_empty() {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            out.extend_from_slice(&[0, 0]);
        }
    } else {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        encode_ansi_or_utf8_to_utf16le(&bytes[..end], &mut out);
        out.extend_from_slice(&[0, 0]);
    }
    out
}

fn parse_value_rhs(rhs: &str, is_regedit4: bool) -> Result<Option<RegValueData>, String> {
    let trimmed = rhs.trim();
    if trimmed == "-" {
        return Ok(None);
    }

    if trimmed.starts_with('"') {
        let (val, rest) = parse_quoted_string_literal(trimmed)?;
        let rest_clean = rest.trim();
        if !rest_clean.is_empty() && !rest_clean.starts_with(';') {
            return Err(format!(
                "Unexpected trailing characters after string value: '{rest_clean}'"
            ));
        }
        return Ok(Some(RegValueData::Sz(val)));
    }

    let lower = trimmed.to_ascii_lowercase();
    if let Some(hex_str) = lower.strip_prefix("dword:") {
        let clean = hex_str.trim();
        let val = u32::from_str_radix(clean, 16)
            .map_err(|_| format!("Invalid DWORD hex value '{clean}'"))?;
        return Ok(Some(RegValueData::Dword(val)));
    }

    if let Some(hex_body) = lower.strip_prefix("hex:") {
        let bytes = parse_hex_byte_list(hex_body)?;
        return Ok(Some(RegValueData::Binary(bytes)));
    }

    if lower.starts_with("hex(") {
        if let Some(close_paren) = trimmed.find("):") {
            let type_str = trimmed[4..close_paren].trim();
            let reg_type = u32::from_str_radix(type_str, 16)
                .map_err(|_| format!("Invalid hex(<type>) specifier '{type_str}'"))?;
            let bytes = parse_hex_byte_list(&trimmed[close_paren + 2..])?;
            let data = match reg_type {
                0 => RegValueData::None(bytes),
                1 => RegValueData::Raw { reg_type: 1, bytes },
                2 => {
                    if is_regedit4 {
                        RegValueData::ExpandSz(regedit4_bytes_to_utf16le(&bytes, false))
                    } else {
                        if bytes.len() % 2 != 0 {
                            return Err(format!(
                                "REG_EXPAND_SZ (hex(2)) byte count must be even, got {}",
                                bytes.len()
                            ));
                        }
                        RegValueData::ExpandSz(bytes)
                    }
                }
                3 => RegValueData::Binary(bytes),
                4 => {
                    if bytes.len() != 4 {
                        return Err(format!(
                            "REG_DWORD (hex(4)) requires 4 bytes, got {}",
                            bytes.len()
                        ));
                    }
                    RegValueData::Dword(u32::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ]))
                }
                7 => {
                    if is_regedit4 {
                        RegValueData::MultiSz(regedit4_bytes_to_utf16le(&bytes, true))
                    } else {
                        if bytes.len() % 2 != 0 {
                            return Err(format!(
                                "REG_MULTI_SZ (hex(7)) byte count must be even, got {}",
                                bytes.len()
                            ));
                        }
                        RegValueData::MultiSz(bytes)
                    }
                }
                11 => {
                    if bytes.len() != 8 {
                        return Err(format!(
                            "REG_QWORD (hex(b)) requires 8 bytes, got {}",
                            bytes.len()
                        ));
                    }
                    RegValueData::Qword(u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ]))
                }
                other => RegValueData::Raw {
                    reg_type: other,
                    bytes,
                },
            };
            return Ok(Some(data));
        }
    }

    Err(format!("Unsupported registry value syntax: '{trimmed}'"))
}

pub fn parse_reg_content(content: &str) -> ParsedRegFile {
    let mut operations = Vec::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let raw_lines: Vec<&str> = content.lines().collect();
    let mut idx = 0usize;
    let mut header_seen = false;
    let mut is_regedit4 = false;
    let mut current_section: Option<MappedSection> = None;
    let mut in_unsupported_section = false;

    while idx < raw_lines.len() {
        let start_line_num = idx + 1;
        let line_trimmed = raw_lines[idx].trim();
        idx += 1;

        if line_trimmed.is_empty() || line_trimmed.starts_with(';') {
            continue;
        }

        if !header_seen {
            if line_trimmed.eq_ignore_ascii_case("Windows Registry Editor Version 5.00") {
                header_seen = true;
                is_regedit4 = false;
                continue;
            }
            if line_trimmed.eq_ignore_ascii_case("REGEDIT4") {
                header_seen = true;
                is_regedit4 = true;
                continue;
            }
            errors.push(format!(
                "Line {start_line_num}: Missing or unsupported .reg header (expected 'Windows Registry Editor Version 5.00')."
            ));
            return ParsedRegFile {
                operations,
                warnings,
                errors,
            };
        }

        // Section header [Key] or [-Key]
        if line_trimmed.starts_with('[') {
            if !line_trimmed.ends_with(']') {
                errors.push(format!(
                    "Line {start_line_num}: Unclosed registry section header '{line_trimmed}'."
                ));
                current_section = None;
                in_unsupported_section = false;
                continue;
            }

            let inner = &line_trimmed[1..line_trimmed.len() - 1];
            let (is_delete, key_str) = if let Some(rest) = inner.strip_prefix('-') {
                (true, rest)
            } else {
                (false, inner)
            };

            match map_offline_key_path(key_str) {
                KeyMapOutcome::Mapped(mapped) => {
                    in_unsupported_section = false;
                    let kind = if is_delete {
                        ParsedOpKind::DeleteKey
                    } else {
                        ParsedOpKind::CreateKey
                    };
                    operations.push(ParsedRegOp {
                        line: start_line_num,
                        kind,
                        original_key: mapped.original_key.clone(),
                        hive: mapped.hive,
                        subkey: mapped.subkey.clone(),
                        uses_current_control_set: mapped.uses_current_control_set,
                        value_name: None,
                        value_data: None,
                    });
                    current_section = if is_delete { None } else { Some(mapped) };
                }
                KeyMapOutcome::Unsupported(raw) => {
                    warnings.push(format!(
                        "Line {start_line_num}: Skipped unsupported offline root '{raw}'."
                    ));
                    current_section = None;
                    in_unsupported_section = true;
                }
                KeyMapOutcome::Invalid(err) => {
                    errors.push(format!("Line {start_line_num}: {err}"));
                    current_section = None;
                    in_unsupported_section = false;
                }
            }
            continue;
        }

        // Join backslash line continuations for value lines
        let mut logical = line_trimmed.to_string();
        while logical.ends_with('\\') && idx < raw_lines.len() {
            logical.pop();
            let cont = raw_lines[idx].trim_start();
            logical.push_str(cont.trim_end());
            idx += 1;
        }

        if in_unsupported_section {
            continue;
        }

        let Some(ref section) = current_section else {
            errors.push(format!(
                "Line {start_line_num}: Registry value defined outside of any [Key] section."
            ));
            continue;
        };

        let (value_name, rhs) = if let Some(after_at) = logical.strip_prefix('@') {
            let after_at_trimmed = after_at.trim_start();
            let Some(rhs) = after_at_trimmed.strip_prefix('=') else {
                errors.push(format!(
                    "Line {start_line_num}: Expected '=' after '@' default value."
                ));
                continue;
            };
            (String::new(), rhs)
        } else if logical.starts_with('"') {
            match parse_quoted_string_literal(&logical) {
                Ok((name, rest)) => {
                    let rest_trimmed = rest.trim_start();
                    let Some(rhs) = rest_trimmed.strip_prefix('=') else {
                        errors.push(format!(
                            "Line {start_line_num}: Expected '=' after value name \"{name}\"."
                        ));
                        continue;
                    };
                    (name, rhs)
                }
                Err(e) => {
                    errors.push(format!("Line {start_line_num}: {e}"));
                    continue;
                }
            }
        } else {
            errors.push(format!(
                "Line {start_line_num}: Invalid registry entry '{logical}'."
            ));
            continue;
        };

        if value_name.contains('\0') {
            errors.push(format!(
                "Line {start_line_num}: Registry value name contains an embedded NUL character."
            ));
            continue;
        }

        match parse_value_rhs(rhs, is_regedit4) {
            Ok(Some(data)) => {
                operations.push(ParsedRegOp {
                    line: start_line_num,
                    kind: ParsedOpKind::SetValue,
                    original_key: section.original_key.clone(),
                    hive: section.hive,
                    subkey: section.subkey.clone(),
                    uses_current_control_set: section.uses_current_control_set,
                    value_name: Some(value_name),
                    value_data: Some(data),
                });
            }
            Ok(None) => {
                operations.push(ParsedRegOp {
                    line: start_line_num,
                    kind: ParsedOpKind::DeleteValue,
                    original_key: section.original_key.clone(),
                    hive: section.hive,
                    subkey: section.subkey.clone(),
                    uses_current_control_set: section.uses_current_control_set,
                    value_name: Some(value_name),
                    value_data: None,
                });
            }
            Err(e) => {
                errors.push(format!("Line {start_line_num}: {e}"));
            }
        }
    }

    if !header_seen && errors.is_empty() {
        errors.push("Registry file is empty or has no valid header.".to_string());
    }

    ParsedRegFile {
        operations,
        warnings,
        errors,
    }
}

pub fn preview_reg_file(path: &Path) -> Result<RegFilePreview, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed reading '{}': {e}", path.display()))?;
    let content = decode_text_bytes(&bytes);
    let parsed = parse_reg_content(&content);

    let mut total_set = 0usize;
    let mut total_delete = 0usize;
    let mut hive_labels = BTreeSet::new();
    let mut preview_ops = Vec::new();
    let mut warnings = parsed.warnings;

    for op in &parsed.operations {
        match op.kind {
            ParsedOpKind::CreateKey | ParsedOpKind::SetValue => total_set += 1,
            ParsedOpKind::DeleteKey | ParsedOpKind::DeleteValue => total_delete += 1,
        }

        let hive_display = if op.uses_current_control_set {
            "SYSTEM → ControlSet001".to_string()
        } else {
            op.hive.label().to_string()
        };
        hive_labels.insert(hive_display.clone());

        if preview_ops.len() < MAX_PREVIEW_OPS {
            let kind_str = match op.kind {
                ParsedOpKind::CreateKey => "create_key",
                ParsedOpKind::DeleteKey => "delete_key",
                ParsedOpKind::SetValue => "set_value",
                ParsedOpKind::DeleteValue => "delete_value",
            };
            preview_ops.push(RegPreviewOperation {
                line: op.line,
                kind: kind_str.to_string(),
                hive: op.hive,
                hive_display,
                target_path: op.resolved_subkey("ControlSet001"),
                value_name: op.value_name.clone(),
                value_type: op.value_data.as_ref().map(|d| d.type_name()),
                value_preview: op.value_data.as_ref().map(|d| d.preview_string()),
                uses_current_control_set: op.uses_current_control_set,
            });
        }
    }

    if parsed.operations.len() > MAX_PREVIEW_OPS {
        warnings.push(format!(
            "Preview truncated to first {MAX_PREVIEW_OPS} operations (total: {}).",
            parsed.operations.len()
        ));
    }

    Ok(RegFilePreview {
        path: path.to_string_lossy().to_string(),
        valid: parsed.errors.is_empty(),
        total_set,
        total_delete,
        affected_hives: hive_labels.into_iter().collect(),
        operations: preview_ops,
        warnings,
        errors: parsed.errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf16le_bom_and_all_value_types() {
        let reg_text = "Windows Registry Editor Version 5.00\r\n\
; comment\r\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\TestApp]\r\n\
@=\"Default\\\"Val\\\\\"\r\n\
\"DwordVal\"=dword:0000002a\r\n\
\"QwordVal\"=hex(b):01,00,00,00,00,00,00,00\r\n\
\"ExpandVal\"=hex(2):25,00,53,00,\\\r\n  59,00,53,00,25,00,00,00\r\n\
\"MultiVal\"=hex(7):41,00,00,00,\\\r\n  42,00,00,00,00,00\r\n\
\"BinVal\"=hex:de,ad,be,ef\r\n\
\"NoneVal\"=hex(0):01,02\r\n\
\"RemoveMe\"=-\r\n\
@=-\r\n";

        let mut utf16_bytes = vec![0xFFu8, 0xFEu8];
        for cu in reg_text.encode_utf16() {
            utf16_bytes.extend_from_slice(&cu.to_le_bytes());
        }
        let decoded = decode_text_bytes(&utf16_bytes);
        let parsed = parse_reg_content(&decoded);

        assert!(
            parsed.errors.is_empty(),
            "unexpected errors: {:?}",
            parsed.errors
        );
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.operations.len(), 10);

        assert_eq!(parsed.operations[0].kind, ParsedOpKind::CreateKey);
        assert_eq!(parsed.operations[0].hive, OfflineHive::Software);
        assert_eq!(parsed.operations[0].subkey, "TestApp");

        assert_eq!(parsed.operations[1].value_name.as_deref(), Some(""));
        assert_eq!(
            parsed.operations[1].value_data,
            Some(RegValueData::Sz("Default\"Val\\".to_string()))
        );
        assert_eq!(
            parsed.operations[2].value_data,
            Some(RegValueData::Dword(42))
        );
        assert_eq!(
            parsed.operations[3].value_data,
            Some(RegValueData::Qword(1))
        );
        assert_eq!(
            parsed.operations[4]
                .value_data
                .as_ref()
                .unwrap()
                .preview_string(),
            "\"%SYS%\""
        );
        assert_eq!(
            parsed.operations[5]
                .value_data
                .as_ref()
                .unwrap()
                .preview_string(),
            "[A, B]"
        );
        assert_eq!(
            parsed.operations[6].value_data,
            Some(RegValueData::Binary(vec![0xde, 0xad, 0xbe, 0xef]))
        );
        assert_eq!(
            parsed.operations[7].value_data,
            Some(RegValueData::None(vec![0x01, 0x02]))
        );
        assert_eq!(parsed.operations[8].kind, ParsedOpKind::DeleteValue);
        assert_eq!(parsed.operations[8].value_name.as_deref(), Some("RemoveMe"));
        assert_eq!(parsed.operations[9].kind, ParsedOpKind::DeleteValue);
        assert_eq!(parsed.operations[9].value_name.as_deref(), Some(""));
    }

    #[test]
    fn remaps_current_control_set_hkcu_hkcr_hku_and_deletions() {
        let reg = "\
Windows Registry Editor Version 5.00
[HKLM\\SYSTEM\\CurrentControlSet\\Services\\DiagTrack]
\"Start\"=dword:00000004
[-HKEY_LOCAL_MACHINE\\SOFTWARE\\Protected\\TrustedKey]
[HKEY_CURRENT_USER\\Software\\MyPref]
\"Theme\"=dword:00000000
[HKCR\\.custom]
@=\"CustomFile\"
[HKU\\.DEFAULT\\Control Panel\\Desktop]
\"WallPaper\"=\"\"
";
        let parsed = parse_reg_content(reg);
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);

        let ccs = &parsed.operations[0];
        assert_eq!(ccs.hive, OfflineHive::System);
        assert!(ccs.uses_current_control_set);
        assert_eq!(
            ccs.resolved_subkey("ControlSet001"),
            r"ControlSet001\Services\DiagTrack"
        );
        assert_eq!(
            ccs.resolved_subkey("ControlSet002"),
            r"ControlSet002\Services\DiagTrack"
        );

        let del = &parsed.operations[2];
        assert_eq!(del.kind, ParsedOpKind::DeleteKey);
        assert_eq!(del.hive, OfflineHive::Software);
        assert_eq!(del.subkey, r"Protected\TrustedKey");

        let hkcu = &parsed.operations[3];
        assert_eq!(hkcu.hive, OfflineHive::DefaultUser);
        assert_eq!(hkcu.subkey, r"Software\MyPref");

        let hkcr = &parsed.operations[5];
        assert_eq!(hkcr.hive, OfflineHive::Software);
        assert_eq!(hkcr.subkey, r"Classes\.custom");

        let hku = &parsed.operations[7];
        assert_eq!(hku.hive, OfflineHive::Default);
        assert_eq!(hku.subkey, r"Control Panel\Desktop");
    }

    #[test]
    fn warns_and_skips_unsupported_offline_roots() {
        let reg = "\
Windows Registry Editor Version 5.00
[HKEY_LOCAL_MACHINE\\HARDWARE\\DESCRIPTION\\System]
\"Identifier\"=\"AT/AT COMPATIBLE\"
[HKEY_CURRENT_CONFIG\\System]
\"Foo\"=dword:00000001
[HKEY_USERS\\S-1-5-21-12345\\Software]
\"Bar\"=\"baz\"
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Valid]
\"Ok\"=dword:00000001
";
        let parsed = parse_reg_content(reg);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.warnings.len(), 3);
        assert_eq!(parsed.operations.len(), 2);
        assert_eq!(parsed.operations[0].hive, OfflineHive::Software);
        assert_eq!(parsed.operations[0].subkey, "Valid");
    }

    #[test]
    fn reports_syntax_errors_cleanly() {
        let bad_header = "Not a reg file\n[HKLM\\SOFTWARE\\Foo]\n";
        assert!(!parse_reg_content(bad_header).errors.is_empty());

        let bad_types = "\
Windows Registry Editor Version 5.00
[HKLM\\SOFTWARE\\Foo]
\"BadDword\"=dword:zzzz
\"OddExpand\"=hex(2):41,00,42
";
        let p = parse_reg_content(bad_types);
        assert_eq!(p.errors.len(), 2);

        let embedded_nul = "Windows Registry Editor Version 5.00\n\
            [-HKLM\\SOFTWARE\\\0\\Test]\n\
            [HKLM\\SOFTWARE\\Valid]\n\
            \"Bad\0Name\"=dword:00000001\n";
        let p_nul = parse_reg_content(embedded_nul);
        assert_eq!(p_nul.errors.len(), 2);
        assert!(!p_nul
            .operations
            .iter()
            .any(|op| op.kind == ParsedOpKind::DeleteKey));
    }

    #[test]
    fn parses_utf16be_utf8_bom_regedit4_and_cyrillic_preview() {
        let dir = std::env::temp_dir().join(format!(
            "isocraft_reg_enc_test_{}_{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();

        let utf16be_text = "Windows Registry Editor Version 5.00\r\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\ISOCraft Тест]\r\n\
\"Название\"=\"Сборка Про\"\r\n";
        let mut utf16be_bytes = vec![0xFEu8, 0xFFu8];
        for cu in utf16be_text.encode_utf16() {
            utf16be_bytes.extend_from_slice(&cu.to_be_bytes());
        }
        let be_path = dir.join("utf16be_кириллица.reg");
        fs::write(&be_path, utf16be_bytes).unwrap();
        let preview_be = preview_reg_file(&be_path).unwrap();
        assert!(preview_be.valid, "errors: {:?}", preview_be.errors);
        assert_eq!(preview_be.total_set, 2);
        assert_eq!(
            preview_be.operations[1].value_name.as_deref(),
            Some("Название")
        );

        let utf8_bom_reg4 = "\u{feff}REGEDIT4\r\n\
[HKEY_CURRENT_USER\\Software\\My App]\r\n\
\"Enabled\"=dword:00000001\r\n";
        let reg4_path = dir.join("regedit4.reg");
        fs::write(&reg4_path, utf8_bom_reg4.as_bytes()).unwrap();
        let preview_r4 = preview_reg_file(&reg4_path).unwrap();
        assert!(preview_r4.valid, "errors: {:?}", preview_r4.errors);
        assert_eq!(preview_r4.total_set, 2);
        assert_eq!(preview_r4.operations[0].hive, OfflineHive::DefaultUser);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn converts_regedit4_ansi_expand_and_multi_sz_to_utf16le() {
        let reg = "REGEDIT4\r\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\LegacyApp]\r\n\
\"ExpandAnsi\"=hex(2):25,53,59,53,25,00\r\n\
\"MultiAnsi\"=hex(7):41,00,42,00,00\r\n";

        let parsed = parse_reg_content(reg);
        assert!(parsed.errors.is_empty(), "errors: {:?}", parsed.errors);
        assert_eq!(parsed.operations.len(), 3);
        assert_eq!(
            parsed.operations[1]
                .value_data
                .as_ref()
                .unwrap()
                .preview_string(),
            "\"%SYS%\""
        );
        assert_eq!(
            parsed.operations[2]
                .value_data
                .as_ref()
                .unwrap()
                .preview_string(),
            "[A, B]"
        );
    }
}
