use crate::fs_utils::{is_symlink_or_reparse, is_windows_reserved_name};
use crate::process::{ProcessGuard, run_tracked_command};
use crate::updates::{UpdateScanInfo, inspect_update_path, is_update_file};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogUpdateEntry {
    pub update_id: String,
    pub title: String,
    pub kb: Option<String>,
    pub products: String,
    pub classification: String,
    pub last_updated: String,
    pub version: String,
    pub size_text: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDownloadProgress {
    pub update_id: String,
    pub file_name: String,
    pub file_index: usize,
    pub total_files: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDownloadResult {
    pub update_id: String,
    pub target_dir: String,
    pub downloaded_files: Vec<UpdateScanInfo>,
}

/// Maps a Windows 11 OS build number to its Microsoft Update Catalog release tag.
pub fn resolve_win11_version_tag(build_number: u32) -> &'static str {
    match build_number {
        26300.. => "26H2",
        26200..=26299 => "25H2",
        26100..=26199 => "24H2",
        22631..=26099 => "23H2",
        22621..=22630 => "22H2",
        ..=22620 => "21H2",
    }
}

/// Validates and normalizes a Microsoft Update Catalog GUID (`8-4-4-4-12`).
pub fn validate_catalog_update_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() != 36 {
        return Err(format!("Invalid catalog update ID '{raw}': expected UUID."));
    }
    for (idx, &b) in bytes.iter().enumerate() {
        let is_hyphen_pos = matches!(idx, 8 | 13 | 18 | 23);
        if is_hyphen_pos {
            if b != b'-' {
                return Err(format!("Invalid catalog update ID '{raw}': expected UUID."));
            }
        } else if !b.is_ascii_hexdigit() {
            return Err(format!("Invalid catalog update ID '{raw}': expected UUID."));
        }
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// Extracts a `KBxxxxxxx` identifier from an update title, normalized to uppercase.
pub fn extract_kb_number(title: &str) -> Option<String> {
    let bytes = title.as_bytes();
    let mut i = 0usize;
    while i + 8 <= bytes.len() {
        let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if prev_ok
            && (bytes[i] == b'K' || bytes[i] == b'k')
            && (bytes[i + 1] == b'B' || bytes[i + 1] == b'b')
        {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let digit_count = j - (i + 2);
            let next_ok = j == bytes.len() || !bytes[j].is_ascii_alphanumeric();
            if (6..=8).contains(&digit_count) && next_ok {
                let digits = &title[i + 2..j];
                return Some(format!("KB{digits}"));
            }
        }
        i += 1;
    }
    None
}

fn has_standalone_x86_token(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("x86") {
        let start = search_from + rel;
        let end = start + 3;
        let prev_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let next_is_64 = lower[end..].starts_with("_64") || lower[end..].starts_with("64");
        let next_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if prev_ok && next_ok && !next_is_64 {
            return true;
        }
        search_from = end;
    }
    false
}

/// Returns true if a Microsoft Update Catalog entry targets client Windows 11 x64.
pub fn is_compatible_win11_x64_catalog_entry(title: &str, products: &str) -> bool {
    let combined = format!("{title} {products}").to_ascii_lowercase();
    if combined.contains("arm64")
        || combined.contains("x86-based")
        || has_standalone_x86_token(&combined)
        || combined.contains("windows server")
        || combined.contains("dynamic update")
        || combined.contains("itanium")
        || combined.contains("ia64")
    {
        return false;
    }
    if combined.contains("windows 10") && !combined.contains("windows 11") {
        return false;
    }
    true
}

fn decode_html_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp_idx) = rest.find('&') {
        out.push_str(&rest[..amp_idx]);
        let after_amp = &rest[amp_idx..];
        if let Some(semi_idx) = after_amp.find(';')
            && semi_idx <= 12
        {
            let entity = &after_amp[1..semi_idx];
            let decoded = match entity {
                "amp" => Some("&".to_string()),
                "lt" => Some("<".to_string()),
                "gt" => Some(">".to_string()),
                "quot" => Some("\"".to_string()),
                "apos" | "#39" => Some("'".to_string()),
                "nbsp" | "#160" => Some(" ".to_string()),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                }
                _ if entity.starts_with('#') => entity[1..]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string()),
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push_str(&ch);
                rest = &after_amp[semi_idx + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &after_amp[1..];
    }
    out.push_str(rest);
    out
}

fn strip_html_tags_and_collapse(html: &str) -> String {
    let mut plain = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                plain.push(' ');
            }
            '>' => {
                in_tag = false;
            }
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    let decoded = decode_html_entities(&plain);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_attr_value<'a>(tag_attrs: &'a str, attr_name: &str) -> Option<&'a str> {
    let lower = tag_attrs.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0usize;
    while let Some(pos) = lower[from..].find(attr_name) {
        let start = from + pos;
        let prev_ok = start == 0 || bytes[start - 1].is_ascii_whitespace();
        let mut cursor = start + attr_name.len();
        if prev_ok {
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'=' {
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                if cursor < bytes.len() {
                    let quote = bytes[cursor];
                    if quote == b'"' || quote == b'\'' {
                        let val_start = cursor + 1;
                        if let Some(rel_end) = tag_attrs[val_start..].find(quote as char) {
                            return Some(&tag_attrs[val_start..val_start + rel_end]);
                        }
                    } else {
                        let val_start = cursor;
                        let mut val_end = val_start;
                        while val_end < bytes.len()
                            && !bytes[val_end].is_ascii_whitespace()
                            && bytes[val_end] != b'>'
                        {
                            val_end += 1;
                        }
                        return Some(&tag_attrs[val_start..val_end]);
                    }
                }
            }
        }
        from = start + attr_name.len();
    }
    None
}

fn find_tag_end(html: &str, from: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = from;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
        } else if b == b'"' || b == b'\'' {
            quote = Some(b);
        } else if b == b'>' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn extract_span_by_id_suffix<'a>(cell_html: &'a str, suffix_lower: &str) -> Option<&'a str> {
    let lower = cell_html.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(rel) = lower[cursor..].find("<span") {
        let tag_start = cursor + rel;
        let after_name = tag_start + 5;
        let next_byte = lower.as_bytes().get(after_name).copied().unwrap_or(0);
        if !next_byte.is_ascii_whitespace() && next_byte != b'>' {
            cursor = after_name;
            continue;
        }
        let Some(tag_end) = find_tag_end(cell_html, after_name) else {
            break;
        };
        let attrs = &cell_html[after_name..tag_end];
        if let Some(id_val) = extract_attr_value(attrs, "id")
            && id_val.trim().to_ascii_lowercase().ends_with(suffix_lower)
        {
            let inner_start = tag_end + 1;
            if let Some(close_rel) = lower[inner_start..].find("</span>") {
                return Some(&cell_html[inner_start..inner_start + close_rel]);
            }
        }
        cursor = tag_end + 1;
    }
    None
}

fn parse_human_size_bytes(size_text: &str) -> u64 {
    let normalized = size_text.replace(',', "").to_ascii_uppercase();
    let mut parts = normalized.split_whitespace();
    let Some(num_str) = parts.next() else {
        return 0;
    };
    let Ok(val) = num_str.parse::<f64>() else {
        return 0;
    };
    if !val.is_finite() || val <= 0.0 {
        return 0;
    }
    let unit = parts.next().unwrap_or("B");
    let mult = match unit {
        "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
        "MB" | "MIB" => 1024.0 * 1024.0,
        "KB" | "KIB" => 1024.0,
        "B" => 1.0,
        _ => return 0,
    };
    (val * mult) as u64
}

fn extract_td_cells<'a>(row_html: &'a str, guid_lower: &str) -> [Option<&'a str>; 7] {
    let lower = row_html.to_ascii_lowercase();
    let mut named: [Option<&'a str>; 7] = [None; 7];
    let mut positional: Vec<&'a str> = Vec::new();
    let mut cursor = 0usize;

    while let Some(rel) = lower[cursor..].find("<td") {
        let tag_start = cursor + rel;
        let after_name = tag_start + 3;
        let next_byte = lower.as_bytes().get(after_name).copied().unwrap_or(0);
        if !next_byte.is_ascii_whitespace() && next_byte != b'>' {
            cursor = after_name;
            continue;
        }
        let Some(tag_end) = find_tag_end(row_html, after_name) else {
            break;
        };
        let inner_start = tag_end + 1;
        let Some(close_rel) = lower[inner_start..].find("</td>") else {
            cursor = inner_start;
            continue;
        };
        let cell_inner = &row_html[inner_start..inner_start + close_rel];
        positional.push(cell_inner);

        let attrs = &row_html[after_name..tag_end];
        if let Some(id_val) = extract_attr_value(attrs, "id") {
            let id_low = id_val.trim().to_ascii_lowercase();
            for (col, slot) in named.iter_mut().enumerate().skip(1) {
                let marker = format!("{guid_lower}_c{col}_r");
                if id_low.starts_with(&marker) {
                    *slot = Some(cell_inner);
                    break;
                }
            }
        }

        cursor = inner_start + close_rel + 5;
    }

    for (col, slot) in named.iter_mut().enumerate().skip(1) {
        if slot.is_none()
            && let Some(&pos_cell) = positional.get(col)
        {
            *slot = Some(pos_cell);
        }
    }

    named
}

/// Parses rows from Microsoft Update Catalog `Search.aspx` HTML.
pub fn parse_catalog_search_html(html: &str, only_win11_x64: bool) -> Vec<CatalogUpdateEntry> {
    let lower = html.to_ascii_lowercase();
    let mut entries = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut cursor = 0usize;

    while let Some(rel) = lower[cursor..].find("<tr") {
        let tag_start = cursor + rel;
        let after_name = tag_start + 3;
        let next_byte = lower.as_bytes().get(after_name).copied().unwrap_or(0);
        if !next_byte.is_ascii_whitespace() && next_byte != b'>' {
            cursor = after_name;
            continue;
        }
        let Some(tag_end) = find_tag_end(html, after_name) else {
            break;
        };
        let attrs = &html[after_name..tag_end];
        let guid_opt = extract_attr_value(attrs, "id").and_then(|id_val| {
            let trimmed = id_val.trim();
            let r_pos = trimmed.find("_R").or_else(|| trimmed.find("_r"))?;
            validate_catalog_update_id(&trimmed[..r_pos]).ok()
        });

        let Some(guid) = guid_opt else {
            // Advance past '>' only so outer layout <tr> tags do not swallow inner result rows.
            cursor = tag_end + 1;
            continue;
        };

        let inner_start = tag_end + 1;
        let Some(tr_end_rel) = lower[inner_start..].find("</tr>") else {
            cursor = inner_start;
            continue;
        };
        let row_html = &html[inner_start..inner_start + tr_end_rel];
        cursor = inner_start + tr_end_rel + 5;

        if !seen_ids.insert(guid.clone()) {
            continue;
        }

        let cells = extract_td_cells(row_html, &guid);
        let title = cells[1]
            .map(strip_html_tags_and_collapse)
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }

        let products = cells[2]
            .map(strip_html_tags_and_collapse)
            .unwrap_or_default();
        if only_win11_x64 && !is_compatible_win11_x64_catalog_entry(&title, &products) {
            continue;
        }

        let classification = cells[3]
            .map(strip_html_tags_and_collapse)
            .unwrap_or_default();
        let last_updated = cells[4]
            .map(strip_html_tags_and_collapse)
            .unwrap_or_default();
        let version = cells[5]
            .map(strip_html_tags_and_collapse)
            .unwrap_or_default();

        let size_cell = cells[6].unwrap_or("");
        let size_text = extract_span_by_id_suffix(size_cell, "_size")
            .map(strip_html_tags_and_collapse)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| strip_html_tags_and_collapse(size_cell));
        let orig_bytes = extract_span_by_id_suffix(size_cell, "_originalsize")
            .map(strip_html_tags_and_collapse)
            .and_then(|s| s.replace(',', "").parse::<u64>().ok())
            .unwrap_or(0);
        let size_bytes = if orig_bytes > 0 {
            orig_bytes
        } else {
            parse_human_size_bytes(&size_text)
        };

        let kb = extract_kb_number(&title);
        entries.push(CatalogUpdateEntry {
            update_id: guid,
            title,
            kb,
            products,
            classification,
            last_updated,
            version,
            size_text,
            size_bytes,
        });
    }

    entries
}

fn is_allowed_microsoft_host(url: &str) -> bool {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    let Some(after_scheme) = lower.strip_prefix("https://") else {
        return false;
    };
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return false;
    }
    let host = if let Some((h, port_str)) = authority.split_once(':') {
        if port_str.is_empty() || !port_str.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let Ok(port) = port_str.parse::<u16>() else {
            return false;
        };
        if port != 443 {
            return false;
        }
        h
    } else {
        authority
    };
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return false;
    }
    host == "microsoft.com"
        || host.ends_with(".microsoft.com")
        || host == "windowsupdate.com"
        || host.ends_with(".windowsupdate.com")
}

pub fn extract_redirect_location(headers: &str) -> Option<String> {
    let mut last_location: Option<String> = None;
    for line in headers.lines() {
        let trimmed = line.trim();
        if trimmed
            .get(..9)
            .is_some_and(|p| p.eq_ignore_ascii_case("location:"))
        {
            let val = trimmed[9..].trim();
            if !val.is_empty() {
                last_location = Some(val.to_string());
            }
        }
    }
    last_location
}

pub fn resolve_redirect_url(current_url: &str, location: &str) -> Option<String> {
    let loc = location.trim();
    if loc.is_empty() || loc.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    if loc.len() >= 8 && loc[..8].eq_ignore_ascii_case("https://") {
        return Some(loc.to_string());
    }
    if loc.len() >= 7 && loc[..7].eq_ignore_ascii_case("http://") {
        return Some(format!("https://{}", &loc[7..]));
    }
    let after_scheme = current_url.strip_prefix("https://")?;
    let authority = after_scheme.split(['/', '?', '#']).next()?;
    if let Some(rest) = loc.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if loc.starts_with('/') {
        return Some(format!("https://{authority}{loc}"));
    }
    let path_end = current_url.find(['?', '#']).unwrap_or(current_url.len());
    let base_no_query = &current_url[..path_end];
    let dir_end = base_no_query.rfind('/').unwrap_or(base_no_query.len());
    Some(format!("{}/{loc}", &base_no_query[..dir_end]))
}

fn percent_decode_utf8(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(val) = u8::from_str_radix(hex, 16)
        {
            out.push(val);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Validates a download URL filename and returns a sanitized Windows basename ending in `.msu` or `.cab`.
pub fn sanitize_download_filename(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    let scheme_pos = trimmed
        .find("://")
        .ok_or_else(|| format!("Invalid download URL '{trimmed}': missing scheme."))?;
    let after_scheme = &trimmed[scheme_pos + 3..];
    let slash_pos = after_scheme
        .find('/')
        .ok_or_else(|| format!("Invalid download URL '{trimmed}': missing path."))?;
    let path_and_query = &after_scheme[slash_pos + 1..];
    let path_only = path_and_query.split(['?', '#']).next().unwrap_or("");
    let raw_name = path_only.rsplit('/').next().unwrap_or("").trim();

    let decoded = percent_decode_utf8(raw_name);
    if decoded.is_empty() || decoded == "." || decoded == ".." {
        return Err(format!("Download URL has invalid filename: '{trimmed}'."));
    }

    let mut clean: String = decoded
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || (c as u32) < 0x20
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    clean = clean.trim_matches([' ', '.']).to_string();

    if clean.is_empty() || clean.contains("..") || is_windows_reserved_name(&clean) {
        return Err(format!(
            "Download URL produced unsafe filename: '{trimmed}'."
        ));
    }
    if is_update_file(Path::new(&clean)).is_none() {
        return Err(format!(
            "Unsupported update download filename '{clean}': expected .msu or .cab."
        ));
    }
    Ok(clean)
}

/// Extracts ordered, deduplicated `.msu`/`.cab` download URLs from `DownloadDialog.aspx` response HTML.
pub fn parse_download_dialog_html(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    let mut search_from = 0usize;

    while let Some(rel) = lower[search_from..].find(".url") {
        let marker_start = search_from + rel;
        let mut cursor = marker_start + 4;
        search_from = cursor;

        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'=' {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            continue;
        }
        let quote = bytes[cursor];
        if quote != b'\'' && quote != b'"' {
            continue;
        }
        let val_start = cursor + 1;
        let Some(rel_end) = html[val_start..].find(quote as char) else {
            continue;
        };
        let raw_val = &html[val_start..val_start + rel_end];
        if raw_val.contains('\n') || raw_val.contains('\r') {
            continue;
        }
        let raw_trimmed = raw_val.replace(r"\/", "/");
        let trimmed = raw_trimmed.trim();
        let candidate = if trimmed
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        {
            format!("https://{}", &trimmed[7..])
        } else {
            trimmed.to_string()
        };
        if !is_allowed_microsoft_host(&candidate) {
            continue;
        }
        if sanitize_download_filename(&candidate).is_err() {
            continue;
        }
        if seen.insert(candidate.to_ascii_lowercase()) {
            urls.push(candidate);
        }
    }

    urls
}

fn encode_url_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn parse_content_length_from_bytes(bytes: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(bytes);
    let mut found: Option<u64> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed
            .get(..15)
            .is_some_and(|p| p.eq_ignore_ascii_case("content-length:"))
            && let Ok(len) = trimmed[15..].trim().parse::<u64>()
            && len > 0
        {
            found = Some(len);
        }
    }
    found
}

fn split_body_and_effective_url(stdout: &str) -> (&str, Option<&str>) {
    let trimmed = stdout.trim_end_matches(['\r', '\n']);
    match trimmed.rfind('\n') {
        Some(idx) => (&trimmed[..idx], Some(trimmed[idx + 1..].trim())),
        None => (trimmed, None),
    }
}

const CATALOG_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const MAX_CATALOG_HTML_BYTES: &str = "10485760"; // 10 MiB
const MAX_UPDATE_FILE_BYTES: &str = "4294967296"; // 4 GiB

/// Queries Microsoft Update Catalog `Search.aspx` using `curl.exe`.
pub async fn search_microsoft_catalog(
    curl_path: &Path,
    query: &str,
    only_win11_x64: bool,
    guard: Option<&ProcessGuard>,
) -> Result<Vec<CatalogUpdateEntry>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Search query must not be empty.".to_string());
    }

    let url = format!(
        "https://www.catalog.update.microsoft.com/Search.aspx?q={}",
        encode_url_component(trimmed)
    );
    let args = vec![
        "-sS".to_string(),
        "-L".to_string(),
        "--max-redirs".to_string(),
        "5".to_string(),
        "--max-filesize".to_string(),
        MAX_CATALOG_HTML_BYTES.to_string(),
        "--proto".to_string(),
        "=https".to_string(),
        "--proto-redir".to_string(),
        "=https".to_string(),
        "--fail".to_string(),
        "--max-time".to_string(),
        "30".to_string(),
        "-A".to_string(),
        CATALOG_USER_AGENT.to_string(),
        "-w".to_string(),
        "\n%{url_effective}".to_string(),
        url,
    ];

    let out = run_tracked_command(&curl_path.to_string_lossy(), &args, None, guard, |_| {}).await?;
    if out.exit_code != 0 {
        return Err(format!(
            "Microsoft Update Catalog search failed (exit {}): {}",
            out.exit_code,
            out.stderr.trim()
        ));
    }

    let (body, eff_opt) = split_body_and_effective_url(&out.stdout);
    if let Some(eff) = eff_opt
        && !is_allowed_microsoft_host(eff)
    {
        return Err(format!(
            "Blocked download redirect to untrusted host: '{eff}'."
        ));
    }

    Ok(parse_catalog_search_html(body, only_win11_x64))
}

/// Resolves direct `.msu`/`.cab` download URLs for a catalog update GUID via `DownloadDialog.aspx`.
pub async fn resolve_catalog_download_urls(
    curl_path: &Path,
    update_id: &str,
    guard: Option<&ProcessGuard>,
) -> Result<Vec<String>, String> {
    let guid = validate_catalog_update_id(update_id)?;
    let form_body = format!(
        "updateIDs=[{{\"size\":0,\"languages\":\"\",\"uidInfo\":\"{guid}\",\"updateID\":\"{guid}\"}}]"
    );
    let args = vec![
        "-sS".to_string(),
        "-L".to_string(),
        "--max-redirs".to_string(),
        "5".to_string(),
        "--max-filesize".to_string(),
        MAX_CATALOG_HTML_BYTES.to_string(),
        "--proto".to_string(),
        "=https".to_string(),
        "--proto-redir".to_string(),
        "=https".to_string(),
        "--fail".to_string(),
        "--max-time".to_string(),
        "30".to_string(),
        "-A".to_string(),
        CATALOG_USER_AGENT.to_string(),
        "-w".to_string(),
        "\n%{url_effective}".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        "--data-urlencode".to_string(),
        form_body,
        "https://www.catalog.update.microsoft.com/DownloadDialog.aspx".to_string(),
    ];

    let out = run_tracked_command(&curl_path.to_string_lossy(), &args, None, guard, |_| {}).await?;
    if out.exit_code != 0 {
        return Err(format!(
            "Failed resolving catalog download links for {guid} (exit {}): {}",
            out.exit_code,
            out.stderr.trim()
        ));
    }

    let (body, eff_opt) = split_body_and_effective_url(&out.stdout);
    if let Some(eff) = eff_opt
        && !is_allowed_microsoft_host(eff)
    {
        return Err(format!(
            "Blocked download redirect to untrusted host: '{eff}'."
        ));
    }

    let urls = parse_download_dialog_html(body);
    if urls.is_empty() {
        return Err(format!(
            "No downloadable .msu or .cab links were returned by Microsoft Update Catalog for {guid}."
        ));
    }
    Ok(urls)
}

async fn resolve_safe_download_url(
    curl_path: &Path,
    initial_url: &str,
    guard: Option<&ProcessGuard>,
) -> Result<String, String> {
    if !is_allowed_microsoft_host(initial_url) {
        return Err(format!(
            "Blocked download redirect to untrusted host: '{initial_url}'."
        ));
    }
    let mut current = initial_url.to_string();
    for _hop in 0..5 {
        let args = vec![
            "-sS".to_string(),
            "-I".to_string(),
            "--proto".to_string(),
            "=https".to_string(),
            "--connect-timeout".to_string(),
            "30".to_string(),
            "--max-time".to_string(),
            "30".to_string(),
            "-A".to_string(),
            CATALOG_USER_AGENT.to_string(),
            "-w".to_string(),
            "\n%{http_code}".to_string(),
            current.clone(),
        ];
        let out =
            run_tracked_command(&curl_path.to_string_lossy(), &args, None, guard, |_| {}).await?;
        if out.exit_code != 0 {
            // If HEAD is rejected or fails transiently without redirect, fall back to direct GET without -L
            return Ok(current);
        }
        let (headers, code_opt) = split_body_and_effective_url(&out.stdout);
        let status = code_opt.and_then(|c| c.parse::<u16>().ok()).unwrap_or(200);
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let loc = extract_redirect_location(headers).ok_or_else(|| {
                format!("HTTP {status} redirect missing Location header from '{current}'.")
            })?;
            let next = resolve_redirect_url(&current, &loc)
                .ok_or_else(|| format!("Blocked download redirect to untrusted host: '{loc}'."))?;
            if !is_allowed_microsoft_host(&next) {
                return Err(format!(
                    "Blocked download redirect to untrusted host: '{next}'."
                ));
            }
            current = next;
            continue;
        }
        return Ok(current);
    }
    Err(format!(
        "Too many redirects resolving download URL '{initial_url}'."
    ))
}

/// Downloads all `.msu`/`.cab` files belonging to a catalog update into `<workspace_dir>\downloads\updates`.
pub async fn download_catalog_update<FLog, FProg>(
    curl_path: &Path,
    update_id: &str,
    expected_size_bytes: u64,
    workspace_dir: &Path,
    guard: Option<&ProcessGuard>,
    mut on_log: FLog,
    mut on_progress: FProg,
) -> Result<CatalogDownloadResult, String>
where
    FLog: FnMut(&str) + Send,
    FProg: FnMut(CatalogDownloadProgress) + Send,
{
    let guid = validate_catalog_update_id(update_id)?;
    let downloads_root = workspace_dir.join("downloads");
    if let Ok(meta) = fs::symlink_metadata(&downloads_root)
        && (is_symlink_or_reparse(&meta) || !meta.is_dir())
    {
        return Err(format!(
            "Unsafe downloads directory '{}': symbolic links and reparse points are not allowed.",
            downloads_root.display()
        ));
    }
    let target_dir = downloads_root.join("updates");
    fs::create_dir_all(&target_dir).map_err(|e| {
        format!(
            "Failed creating updates download folder '{}': {e}",
            target_dir.display()
        )
    })?;
    if let Ok(meta) = fs::symlink_metadata(&target_dir)
        && (is_symlink_or_reparse(&meta) || !meta.is_dir())
    {
        return Err(format!(
            "Unsafe updates download folder '{}': symbolic links and reparse points are not allowed.",
            target_dir.display()
        ));
    }

    on_log(&format!(
        "Resolving Microsoft Update Catalog download URLs for {guid}..."
    ));
    let urls = resolve_catalog_download_urls(curl_path, &guid, guard).await?;
    let total_files = urls.len();
    on_log(&format!(
        "Resolved {total_files} file(s) for {guid}; target folder: {}",
        target_dir.display()
    ));

    let mut downloaded_files = Vec::with_capacity(total_files);

    for (idx, url) in urls.iter().enumerate() {
        if let Some(g) = guard
            && g.is_cancel_requested()
        {
            return Err("Operation cancelled by user.".to_string());
        }

        let file_name = sanitize_download_filename(url)?;
        let file_index = idx + 1;
        let final_path = target_dir.join(&file_name);
        let part_path = target_dir.join(format!("{file_name}.part"));
        let hdr_path = target_dir.join(format!("{file_name}.hdr"));

        for candidate in [&final_path, &part_path, &hdr_path] {
            if let Ok(meta) = fs::symlink_metadata(candidate)
                && is_symlink_or_reparse(&meta)
            {
                return Err(format!(
                    "Refusing to overwrite symlink or reparse point at '{}'.",
                    candidate.display()
                ));
            }
        }

        let _ = fs::remove_file(&part_path);
        let _ = fs::remove_file(&hdr_path);

        let safe_url = resolve_safe_download_url(curl_path, url, guard).await?;

        on_log(&format!(
            "Downloading [{file_index}/{total_files}] {file_name}..."
        ));

        let args = vec![
            "-sS".to_string(),
            "--max-filesize".to_string(),
            MAX_UPDATE_FILE_BYTES.to_string(),
            "--proto".to_string(),
            "=https".to_string(),
            "--fail".to_string(),
            "--connect-timeout".to_string(),
            "30".to_string(),
            "--speed-limit".to_string(),
            "1024".to_string(),
            "--speed-time".to_string(),
            "60".to_string(),
            "-A".to_string(),
            CATALOG_USER_AGENT.to_string(),
            "-w".to_string(),
            "\n%{url_effective}".to_string(),
            "-D".to_string(),
            hdr_path.to_string_lossy().to_string(),
            "-o".to_string(),
            part_path.to_string_lossy().to_string(),
            safe_url.clone(),
        ];

        let curl_str = curl_path.to_string_lossy().to_string();
        let mut cmd_fut =
            std::pin::pin!(run_tracked_command(&curl_str, &args, None, guard, |_| {},));
        let mut interval = tokio::time::interval(Duration::from_millis(180));

        let cmd_res = loop {
            tokio::select! {
                res = &mut cmd_fut => break res,
                _ = interval.tick() => {
                    let downloaded = tokio::fs::metadata(&part_path).await.map(|m| m.len()).unwrap_or(0);
                    let hdr_len = tokio::fs::read(&hdr_path)
                        .await
                        .ok()
                        .and_then(|b| parse_content_length_from_bytes(&b));
                    let total_bytes = hdr_len.unwrap_or(
                        if total_files == 1 {
                            expected_size_bytes
                        } else {
                            0
                        },
                    );
                    let percent = if total_bytes > 0 {
                        ((downloaded as f64 / total_bytes as f64) * 100.0).clamp(0.0, 99.9) as f32
                    } else {
                        0.0
                    };
                    on_progress(CatalogDownloadProgress {
                        update_id: guid.clone(),
                        file_name: file_name.clone(),
                        file_index,
                        total_files,
                        downloaded_bytes: downloaded,
                        total_bytes,
                        percent,
                    });
                }
            }
        };

        let out = match cmd_res {
            Ok(o) => o,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                let _ = tokio::fs::remove_file(&hdr_path).await;
                return Err(e);
            }
        };

        if let Some(g) = guard
            && g.is_cancel_requested()
        {
            let _ = tokio::fs::remove_file(&part_path).await;
            let _ = tokio::fs::remove_file(&hdr_path).await;
            return Err("Operation cancelled by user.".to_string());
        }

        if out.exit_code != 0 {
            let _ = tokio::fs::remove_file(&part_path).await;
            let _ = tokio::fs::remove_file(&hdr_path).await;
            return Err(format!(
                "Failed downloading '{file_name}' (exit {}): {}",
                out.exit_code,
                out.stderr.trim()
            ));
        }

        let effective_url = out
            .stdout
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.trim())
            .unwrap_or(safe_url.as_str());
        if !is_allowed_microsoft_host(effective_url) {
            let _ = tokio::fs::remove_file(&part_path).await;
            let _ = tokio::fs::remove_file(&hdr_path).await;
            return Err(format!(
                "Blocked download redirect to untrusted host: '{effective_url}'."
            ));
        }

        let _ = tokio::fs::remove_file(&hdr_path).await;

        #[cfg(all(windows, not(test)))]
        {
            let part_verify = part_path.clone();
            let sig_ok = tokio::task::spawn_blocking(move || {
                crate::env::verify_microsoft_authenticode(&part_verify)
            })
            .await
            .unwrap_or(false);
            if !sig_ok {
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(format!(
                    "Downloaded update '{file_name}' failed Microsoft Authenticode signature verification."
                ));
            }
        }

        if final_path.exists() {
            let _ = tokio::fs::remove_file(&final_path).await;
        }
        tokio::fs::rename(&part_path, &final_path)
            .await
            .map_err(|e| {
                let p_clone = part_path.clone();
                let _ = std::fs::remove_file(p_clone);
                format!(
                    "Failed saving downloaded update '{}': {e}",
                    final_path.display()
                )
            })?;

        let scan = inspect_update_path(&final_path)?;
        on_progress(CatalogDownloadProgress {
            update_id: guid.clone(),
            file_name: file_name.clone(),
            file_index,
            total_files,
            downloaded_bytes: scan.total_bytes,
            total_bytes: scan.total_bytes,
            percent: 100.0,
        });
        on_log(&format!(
            "Downloaded [{file_index}/{total_files}] {} ({} bytes).",
            final_path.display(),
            scan.total_bytes
        ));
        downloaded_files.push(scan);
    }

    Ok(CatalogDownloadResult {
        update_id: guid,
        target_dir: target_dir.to_string_lossy().to_string(),
        downloaded_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_catalog_update_uuid_and_kb() {
        assert_eq!(
            validate_catalog_update_id(" D3B07384-D113-4E96-A7C8-5A2D1B2C3D4E ").unwrap(),
            "d3b07384-d113-4e96-a7c8-5a2d1b2c3d4e"
        );
        assert!(validate_catalog_update_id("not-a-uuid").is_err());
        assert!(validate_catalog_update_id("d3b07384d1134e96a7c85a2d1b2c3d4e").is_err());
        assert!(validate_catalog_update_id("g3b07384-d113-4e96-a7c8-5a2d1b2c3d4e").is_err());

        assert_eq!(
            extract_kb_number("2025-10 Cumulative Update for Windows 11 (kb5044284)"),
            Some("KB5044284".to_string())
        );
        assert_eq!(extract_kb_number("Update without KB code"), None);
    }

    #[test]
    fn parses_catalog_search_html_and_filters_incompatible() {
        let html = r#"
        <html><body><table><tr><td>
          <table id="ctl00_catalogBody_updateMatches">
            <tr id="headerRow"><th>Title</th></tr>
            <tr id="11111111-2222-3333-4444-555555555555_R0">
              <td id="11111111-2222-3333-4444-555555555555_C0_R0"><input type="checkbox"/></td>
              <td id="11111111-2222-3333-4444-555555555555_C1_R0">
                <a href="javascript:void(0);">2025-10 Cumulative Update for Windows 11 Version 24H2 for x64-based Systems &amp; Fixes (KB5044284)</a>
              </td>
              <td id="11111111-2222-3333-4444-555555555555_C2_R0">Windows 11</td>
              <td id="11111111-2222-3333-4444-555555555555_C3_R0">Security Updates</td>
              <td id="11111111-2222-3333-4444-555555555555_C4_R0">10/8/2024</td>
              <td id="11111111-2222-3333-4444-555555555555_C5_R0">n/a</td>
              <td id="11111111-2222-3333-4444-555555555555_C6_R0">
                <span id="11111111-2222-3333-4444-555555555555_size">654.2 MB</span>
                <span id="11111111-2222-3333-4444-555555555555_originalSize" style="display:none">685975552</span>
              </td>
            </tr>
            <tr id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_R1">
              <td id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_C0_R1"></td>
              <td id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_C1_R1"><a>2025-10 Cumulative Update for Windows 11 Version 24H2 for ARM64-based Systems (KB5044284)</a></td>
              <td id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_C2_R1">Windows 11</td>
              <td id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_C3_R1">Security Updates</td>
              <td id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_C4_R1">10/8/2024</td>
              <td id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_C5_R1">n/a</td>
              <td id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_C6_R1"><span id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee_size">512 MB</span></td>
            </tr>
            <tr id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_R2">
              <td id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_C0_R2"></td>
              <td id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_C1_R2"><a>2025-10 Cumulative Update for Windows Server 2025 for x64-based Systems (KB5044284)</a></td>
              <td id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_C2_R2">Windows Server 2025</td>
              <td id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_C3_R2">Security Updates</td>
              <td id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_C4_R2">10/8/2024</td>
              <td id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_C5_R2">n/a</td>
              <td id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_C6_R2"><span id="bbbbbbbb-cccc-dddd-eeee-ffffffffffff_size">700 MB</span></td>
            </tr>
            <tr id="cccccccc-dddd-eeee-ffff-000000000000_R3">
              <td id="cccccccc-dddd-eeee-ffff-000000000000_C0_R3"></td>
              <td id="cccccccc-dddd-eeee-ffff-000000000000_C1_R3"><a>2025-10 Cumulative Update for Windows 10 Version 22H2 for x86-based Systems (KB5044273)</a></td>
              <td id="cccccccc-dddd-eeee-ffff-000000000000_C2_R3">Windows 10</td>
              <td id="cccccccc-dddd-eeee-ffff-000000000000_C3_R3">Security Updates</td>
              <td id="cccccccc-dddd-eeee-ffff-000000000000_C4_R3">10/8/2024</td>
              <td id="cccccccc-dddd-eeee-ffff-000000000000_C5_R3">n/a</td>
              <td id="cccccccc-dddd-eeee-ffff-000000000000_C6_R3"><span id="cccccccc-dddd-eeee-ffff-000000000000_size">400 MB</span></td>
            </tr>
            <tr id="dddddddd-eeee-ffff-0000-111111111111_R4">
              <td id="dddddddd-eeee-ffff-0000-111111111111_C0_R4"></td>
              <td id="dddddddd-eeee-ffff-0000-111111111111_C1_R4"><a>2026-11 Safe OS Dynamic Update for Windows 11, version 26H2 for x64-based Systems (KB5133853)</a></td>
              <td id="dddddddd-eeee-ffff-0000-111111111111_C2_R4">Windows Safe OS Dynamic Update, Windows 10 and later Dynamic Update</td>
              <td id="dddddddd-eeee-ffff-0000-111111111111_C3_R4">Critical Updates</td>
              <td id="dddddddd-eeee-ffff-0000-111111111111_C4_R4">11/19/2026</td>
              <td id="dddddddd-eeee-ffff-0000-111111111111_C5_R4">n/a</td>
              <td id="dddddddd-eeee-ffff-0000-111111111111_C6_R4"><span id="dddddddd-eeee-ffff-0000-111111111111_size">247.3 MB</span></td>
            </tr>
          </table>
        </td></tr></table></body></html>
        "#;

        let filtered = parse_catalog_search_html(html, true);
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].update_id,
            "11111111-2222-3333-4444-555555555555"
        );
        assert!(filtered[0].title.contains("& Fixes"));
        assert_eq!(filtered[0].kb.as_deref(), Some("KB5044284"));
        assert_eq!(filtered[0].products, "Windows 11");
        assert_eq!(filtered[0].classification, "Security Updates");
        assert_eq!(filtered[0].last_updated, "10/8/2024");
        assert_eq!(filtered[0].size_text, "654.2 MB");
        assert_eq!(filtered[0].size_bytes, 685_975_552);

        let unfiltered = parse_catalog_search_html(html, false);
        assert_eq!(unfiltered.len(), 5);
    }

    #[test]
    fn parses_download_dialog_with_checkpoint_and_lcu_chain() {
        let dialog_html = r#"
        <script>
          downloadInformation[0] = new Object();
          downloadInformation[0].files = new Array();
          downloadInformation[0].files[0] = new Object();
          downloadInformation[0].files[0].url = 'https://catalog.s.download.windowsupdate.com/c/msdownload/update/software/secu/2024/09/windows11.0-kb5043080-x64_ckpt.msu';
          downloadInformation[0].files[1] = new Object();
          downloadInformation[0].files[1].url = "https://catalog.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/abc/windows11.0-kb5044284-x64_lcu.msu?P1=123";
          downloadInformation[0].files[2] = new Object();
          downloadInformation[0].files[2].url = 'https://catalog.s.download.windowsupdate.com/c/msdownload/update/software/secu/2024/09/windows11.0-kb5043080-x64_ckpt.msu';
          downloadInformation[0].files[3] = new Object();
          downloadInformation[0].files[3].url = 'https://evil.example.com/fake.msu';
          downloadInformation[0].files[4] = new Object();
          downloadInformation[0].files[4].url = 'https://download.windowsupdate.com/bad/payload.exe';
        </script>
        "#;

        let urls = parse_download_dialog_html(dialog_html);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].ends_with("kb5043080-x64_ckpt.msu"));
        assert!(urls[1].contains("kb5044284-x64_lcu.msu"));
    }

    #[test]
    fn sanitizes_download_filenames_and_rejects_bad_extensions() {
        assert_eq!(
            sanitize_download_filename(
                "https://catalog.s.download.windowsupdate.com/msdownload/windows11.0-kb5044284%20x64.msu?token=123"
            )
            .unwrap(),
            "windows11.0-kb5044284 x64.msu"
        );
        assert_eq!(
            sanitize_download_filename("https://download.windowsupdate.com/d/pkg:1.cab").unwrap(),
            "pkg_1.cab"
        );
        assert!(
            sanitize_download_filename("https://download.windowsupdate.com/d/setup.exe").is_err()
        );
        assert!(sanitize_download_filename("https://download.windowsupdate.com/").is_err());
    }

    #[test]
    fn resolves_win11_version_tags_including_26h2() {
        assert_eq!(resolve_win11_version_tag(22000), "21H2");
        assert_eq!(resolve_win11_version_tag(22621), "22H2");
        assert_eq!(resolve_win11_version_tag(22631), "23H2");
        assert_eq!(resolve_win11_version_tag(26100), "24H2");
        assert_eq!(resolve_win11_version_tag(26200), "25H2");
        assert_eq!(resolve_win11_version_tag(26300), "26H2");
        assert_eq!(resolve_win11_version_tag(26800), "26H2");
    }

    #[test]
    fn enforces_https_and_rejects_userinfo_bypass_in_microsoft_hosts() {
        assert!(is_allowed_microsoft_host(
            "https://catalog.s.download.windowsupdate.com/c/msdownload/kb.msu"
        ));
        assert!(is_allowed_microsoft_host(
            "https://download.microsoft.com:443/update.cab"
        ));

        // Userinfo @ bypasses & plain HTTP must be rejected
        assert!(!is_allowed_microsoft_host(
            "https://download.microsoft.com:443@evil.com/update.msu"
        ));
        assert!(!is_allowed_microsoft_host(
            "https://download.microsoft.com@evil.com/update.msu"
        ));
        assert!(!is_allowed_microsoft_host(
            "http://download.windowsupdate.com/update.msu"
        ));
        assert!(!is_allowed_microsoft_host(
            "https://evil.com/microsoft.com/update.msu"
        ));

        // parse_download_dialog_html upgrades http:// to https:// and drops @ bypasses
        let html = r#"
        <script>
          f[0].url = 'http://catalog.s.download.windowsupdate.com/c/kb5044284.msu';
          f[1].url = 'https://download.microsoft.com:443@evil.com/fake.msu';
        </script>
        "#;
        let urls = parse_download_dialog_html(html);
        assert_eq!(
            urls,
            vec!["https://catalog.s.download.windowsupdate.com/c/kb5044284.msu"]
        );
        assert!(
            sanitize_download_filename("https://download.windowsupdate.com/d/CON.msu").is_err()
        );
        assert!(!is_allowed_microsoft_host(
            "https://download.sfx.ms/fake.msu"
        ));
        assert!(!is_allowed_microsoft_host(
            "https://az123.vo.msecnd.net/fake.msu"
        ));
        assert!(!is_allowed_microsoft_host(
            "https://download.microsoft.com:8443/fake.msu"
        ));
        assert!(!is_allowed_microsoft_host(
            "https://download.microsoft.com:99999/fake.msu"
        ));
    }

    #[test]
    fn extracts_and_resolves_redirect_locations_safely() {
        let headers = "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: https://catalog.sf.dl.delivery.mp.microsoft.com/update.msu\r\n\r\n";
        assert_eq!(
            extract_redirect_location(headers).as_deref(),
            Some("https://catalog.sf.dl.delivery.mp.microsoft.com/update.msu")
        );

        assert_eq!(
            resolve_redirect_url(
                "https://www.catalog.update.microsoft.com/DownloadDialog.aspx",
                "/files/kb123.msu"
            )
            .as_deref(),
            Some("https://www.catalog.update.microsoft.com/files/kb123.msu")
        );
        assert_eq!(
            resolve_redirect_url(
                "https://www.catalog.update.microsoft.com/DownloadDialog.aspx",
                "//download.windowsupdate.com/kb123.msu"
            )
            .as_deref(),
            Some("https://download.windowsupdate.com/kb123.msu")
        );
        assert!(
            resolve_redirect_url(
                "https://www.catalog.update.microsoft.com/DownloadDialog.aspx",
                "https://evil.com/\nfoo"
            )
            .is_none()
        );
    }
}
