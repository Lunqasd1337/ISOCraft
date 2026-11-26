use crate::env::strip_windows_verbatim_prefix;
use crate::fs_utils::{is_symlink_or_reparse, read_file_bounded};
use crate::post_setup::{WingetSource, validate_winget_id};
use crate::process::run_tracked_command;
use crate::update_catalog::is_allowed_microsoft_host;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub const WINGET_SOURCE2_URL: &str = "https://cdn.winget.microsoft.com/cache/source2.msix";
pub const MSSTORE_SEARCH_URL: &str = "https://storeedgefd.dsx.mp.microsoft.com/v9.0/manifestSearch";
pub const WINGET_INDEX_TTL_SECS: u64 = 24 * 60 * 60;
const MAX_WINGET_MSIX_BYTES: &str = "104857600"; // 100 MiB
const MAX_MSSTORE_JSON_BYTES: &str = "10485760"; // 10 MiB
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
pub const DEFAULT_SEARCH_LIMIT: usize = 40;
pub const MAX_SEARCH_LIMIT: usize = 100;
const WINGET_INDEX_DB_FILE: &str = "winget_index.db";
const WINGET_INDEX_META_FILE: &str = "winget_index_meta.json";

static TEMP_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WingetSearchEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: Option<String>,
    pub moniker: Option<String>,
    pub source: WingetSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetSearchResponse {
    pub items: Vec<WingetSearchEntry>,
    pub source: WingetSource,
    pub index_updated_at: Option<String>,
    pub from_cache: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WingetIndexMeta {
    updated_at: String,
    size_bytes: u64,
}

struct TempPathGuard {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl TempPathGuard {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            dirs: Vec::new(),
        }
    }

    fn track_file(&mut self, path: PathBuf) {
        self.files.push(path);
    }

    fn track_dir(&mut self, path: PathBuf) {
        self.dirs.push(path);
    }
}

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        for f in &self.files {
            let _ = fs::remove_file(f);
        }
        for d in &self.dirs {
            let _ = fs::remove_dir_all(d);
        }
    }
}

fn next_nonce() -> u64 {
    TEMP_NONCE.fetch_add(1, Ordering::Relaxed)
}

fn is_valid_sqlite_file(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if is_symlink_or_reparse(&meta) || !meta.is_file() || meta.len() < SQLITE_MAGIC.len() as u64 {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0u8; 16];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    &header == SQLITE_MAGIC
}

pub fn path_to_sqlite_ro_uri(path: &Path) -> String {
    let clean = strip_windows_verbatim_prefix(path);
    let forward = clean.to_string_lossy().replace('\\', "/");
    let mut escaped = String::with_capacity(forward.len());
    for ch in forward.chars() {
        match ch {
            '%' => escaped.push_str("%25"),
            '?' => escaped.push_str("%3F"),
            '#' => escaped.push_str("%23"),
            _ => escaped.push(ch),
        }
    }
    if escaped.starts_with('/') {
        format!("file://{escaped}?mode=ro&immutable=1")
    } else {
        format!("file:///{escaped}?mode=ro&immutable=1")
    }
}

pub fn escape_like_pattern(token: &str) -> String {
    let mut out = String::with_capacity(token.len() + 4);
    for ch in token.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn normalize_moniker(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn compute_match_rank(
    id_lower: &str,
    name_lower: &str,
    moniker_lower: &str,
    query_lower: &str,
    tokens: &[String],
) -> Option<u8> {
    if !tokens.iter().all(|t| {
        id_lower.contains(t.as_str())
            || name_lower.contains(t.as_str())
            || (!moniker_lower.is_empty() && moniker_lower.contains(t.as_str()))
    }) {
        return None;
    }

    if id_lower == query_lower
        || name_lower == query_lower
        || (!moniker_lower.is_empty() && moniker_lower == query_lower)
    {
        Some(0)
    } else if id_lower.starts_with(query_lower)
        || name_lower.starts_with(query_lower)
        || (!moniker_lower.is_empty() && moniker_lower.starts_with(query_lower))
    {
        Some(1)
    } else if name_lower.contains(query_lower)
        || (!moniker_lower.is_empty() && moniker_lower.contains(query_lower))
    {
        Some(2)
    } else {
        Some(3)
    }
}

#[cfg(windows)]
mod winsqlite {
    use super::*;
    use core::ffi::{c_char, c_int, c_void};
    use std::ffi::CString;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    const SQLITE_OK: c_int = 0;
    const SQLITE_ROW: c_int = 100;
    const SQLITE_DONE: c_int = 101;
    const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
    #[cfg(test)]
    const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
    #[cfg(test)]
    const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
    const SQLITE_OPEN_URI: c_int = 0x0000_0040;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(lp_lib_file_name: *const u16) -> *mut c_void;
        fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const c_char) -> *mut c_void;
    }

    type FnOpenV2 =
        unsafe extern "system" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
    type FnCloseV2 = unsafe extern "system" fn(*mut c_void) -> c_int;
    type FnPrepareV2 = unsafe extern "system" fn(
        *mut c_void,
        *const c_char,
        c_int,
        *mut *mut c_void,
        *mut *const c_char,
    ) -> c_int;
    type FnBindText =
        unsafe extern "system" fn(*mut c_void, c_int, *const c_char, c_int, *const c_void) -> c_int;
    type FnBindInt64 = unsafe extern "system" fn(*mut c_void, c_int, i64) -> c_int;
    type FnStep = unsafe extern "system" fn(*mut c_void) -> c_int;
    type FnColumnText = unsafe extern "system" fn(*mut c_void, c_int) -> *const u8;
    type FnColumnBytes = unsafe extern "system" fn(*mut c_void, c_int) -> c_int;
    type FnFinalize = unsafe extern "system" fn(*mut c_void) -> c_int;
    type FnErrmsg = unsafe extern "system" fn(*mut c_void) -> *const c_char;
    #[cfg(test)]
    type FnExec = unsafe extern "system" fn(
        *mut c_void,
        *const c_char,
        *const c_void,
        *mut c_void,
        *mut *mut c_char,
    ) -> c_int;
    #[cfg(test)]
    type FnFree = unsafe extern "system" fn(*mut c_void);

    struct WinsqliteApi {
        open_v2: FnOpenV2,
        close_v2: FnCloseV2,
        prepare_v2: FnPrepareV2,
        bind_text: FnBindText,
        bind_int64: FnBindInt64,
        step: FnStep,
        column_text: FnColumnText,
        column_bytes: FnColumnBytes,
        finalize: FnFinalize,
        errmsg: FnErrmsg,
        #[cfg(test)]
        exec: FnExec,
        #[cfg(test)]
        free: FnFree,
    }

    unsafe impl Send for WinsqliteApi {}
    unsafe impl Sync for WinsqliteApi {}

    static SQLITE_API: OnceLock<Result<WinsqliteApi, String>> = OnceLock::new();

    unsafe fn get_proc(module: *mut c_void, name: &core::ffi::CStr) -> Result<*mut c_void, String> {
        let ptr = unsafe { GetProcAddress(module, name.as_ptr()) };
        if ptr.is_null() {
            return Err(format!(
                "Missing symbol '{}' in winsqlite3.dll.",
                name.to_string_lossy()
            ));
        }
        Ok(ptr)
    }

    fn load_api(dll_path: &Path) -> Result<&'static WinsqliteApi, String> {
        let res = SQLITE_API.get_or_init(|| {
            let clean = strip_windows_verbatim_prefix(dll_path);
            let wide: Vec<u16> = clean
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let module = unsafe { LoadLibraryW(wide.as_ptr()) };
            if module.is_null() {
                return Err(format!(
                    "Failed loading winsqlite3.dll from '{}'.",
                    clean.display()
                ));
            }

            unsafe {
                Ok(WinsqliteApi {
                    open_v2: std::mem::transmute::<*mut c_void, FnOpenV2>(get_proc(
                        module,
                        c"sqlite3_open_v2",
                    )?),
                    close_v2: std::mem::transmute::<*mut c_void, FnCloseV2>(get_proc(
                        module,
                        c"sqlite3_close_v2",
                    )?),
                    prepare_v2: std::mem::transmute::<*mut c_void, FnPrepareV2>(get_proc(
                        module,
                        c"sqlite3_prepare_v2",
                    )?),
                    bind_text: std::mem::transmute::<*mut c_void, FnBindText>(get_proc(
                        module,
                        c"sqlite3_bind_text",
                    )?),
                    bind_int64: std::mem::transmute::<*mut c_void, FnBindInt64>(get_proc(
                        module,
                        c"sqlite3_bind_int64",
                    )?),
                    step: std::mem::transmute::<*mut c_void, FnStep>(get_proc(
                        module,
                        c"sqlite3_step",
                    )?),
                    column_text: std::mem::transmute::<*mut c_void, FnColumnText>(get_proc(
                        module,
                        c"sqlite3_column_text",
                    )?),
                    column_bytes: std::mem::transmute::<*mut c_void, FnColumnBytes>(get_proc(
                        module,
                        c"sqlite3_column_bytes",
                    )?),
                    finalize: std::mem::transmute::<*mut c_void, FnFinalize>(get_proc(
                        module,
                        c"sqlite3_finalize",
                    )?),
                    errmsg: std::mem::transmute::<*mut c_void, FnErrmsg>(get_proc(
                        module,
                        c"sqlite3_errmsg",
                    )?),
                    #[cfg(test)]
                    exec: std::mem::transmute::<*mut c_void, FnExec>(get_proc(
                        module,
                        c"sqlite3_exec",
                    )?),
                    #[cfg(test)]
                    free: std::mem::transmute::<*mut c_void, FnFree>(get_proc(
                        module,
                        c"sqlite3_free",
                    )?),
                })
            }
        });
        res.as_ref().map_err(|e| e.clone())
    }

    pub struct SqliteDb {
        api: &'static WinsqliteApi,
        handle: *mut c_void,
    }

    impl Drop for SqliteDb {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    let _ = (self.api.close_v2)(self.handle);
                }
            }
        }
    }

    pub struct SqliteStmt<'a> {
        db: &'a SqliteDb,
        handle: *mut c_void,
    }

    impl Drop for SqliteStmt<'_> {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    let _ = (self.db.api.finalize)(self.handle);
                }
            }
        }
    }

    impl SqliteDb {
        pub fn open_readonly(db_path: &Path, dll_path: &Path) -> Result<Self, String> {
            let api = load_api(dll_path)?;
            let uri = path_to_sqlite_ro_uri(db_path);
            let c_uri =
                CString::new(uri).map_err(|_| "Database URI contains NUL byte.".to_string())?;
            let mut handle: *mut c_void = std::ptr::null_mut();
            let rc = unsafe {
                (api.open_v2)(
                    c_uri.as_ptr(),
                    &mut handle,
                    SQLITE_OPEN_READONLY | SQLITE_OPEN_URI,
                    std::ptr::null(),
                )
            };
            if rc != SQLITE_OK || handle.is_null() {
                let msg = if handle.is_null() {
                    format!("sqlite3_open_v2 failed with code {rc}")
                } else {
                    let err_ptr = unsafe { (api.errmsg)(handle) };
                    let text = if err_ptr.is_null() {
                        format!("SQLite error {rc}")
                    } else {
                        unsafe { std::ffi::CStr::from_ptr(err_ptr) }
                            .to_string_lossy()
                            .into_owned()
                    };
                    unsafe {
                        let _ = (api.close_v2)(handle);
                    }
                    text
                };
                return Err(format!("Failed opening Winget index database: {msg}"));
            }
            Ok(Self { api, handle })
        }

        #[cfg(test)]
        pub fn open_readwrite_create(db_path: &Path, dll_path: &Path) -> Result<Self, String> {
            let api = load_api(dll_path)?;
            let clean = strip_windows_verbatim_prefix(db_path);
            let c_path = CString::new(clean.to_string_lossy().as_bytes())
                .map_err(|_| "Database path contains NUL byte.".to_string())?;
            let mut handle: *mut c_void = std::ptr::null_mut();
            let rc = unsafe {
                (api.open_v2)(
                    c_path.as_ptr(),
                    &mut handle,
                    SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                    std::ptr::null(),
                )
            };
            if rc != SQLITE_OK || handle.is_null() {
                if !handle.is_null() {
                    unsafe {
                        let _ = (api.close_v2)(handle);
                    }
                }
                return Err(format!("sqlite3_open_v2 (rwc) failed with code {rc}"));
            }
            Ok(Self { api, handle })
        }

        #[cfg(test)]
        pub fn exec_batch(&self, sql: &str) -> Result<(), String> {
            let c_sql = CString::new(sql).map_err(|_| "SQL contains NUL byte.".to_string())?;
            let mut err_ptr: *mut c_char = std::ptr::null_mut();
            let rc = unsafe {
                (self.api.exec)(
                    self.handle,
                    c_sql.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    &mut err_ptr,
                )
            };
            if rc != SQLITE_OK {
                let msg = if err_ptr.is_null() {
                    format!("sqlite3_exec failed with code {rc}")
                } else {
                    let s = unsafe { std::ffi::CStr::from_ptr(err_ptr) }
                        .to_string_lossy()
                        .into_owned();
                    unsafe {
                        (self.api.free)(err_ptr.cast());
                    }
                    s
                };
                return Err(msg);
            }
            Ok(())
        }

        fn last_error(&self) -> String {
            let err_ptr = unsafe { (self.api.errmsg)(self.handle) };
            if err_ptr.is_null() {
                "Unknown SQLite error".to_string()
            } else {
                unsafe { std::ffi::CStr::from_ptr(err_ptr) }
                    .to_string_lossy()
                    .into_owned()
            }
        }

        pub fn prepare(&self, sql: &str) -> Result<SqliteStmt<'_>, String> {
            let c_sql = CString::new(sql).map_err(|_| "SQL contains NUL byte.".to_string())?;
            let len = c_int::try_from(c_sql.as_bytes().len())
                .map_err(|_| "SQL statement is too long.".to_string())?;
            let mut stmt_handle: *mut c_void = std::ptr::null_mut();
            let rc = unsafe {
                (self.api.prepare_v2)(
                    self.handle,
                    c_sql.as_ptr(),
                    len,
                    &mut stmt_handle,
                    std::ptr::null_mut(),
                )
            };
            if rc != SQLITE_OK || stmt_handle.is_null() {
                return Err(format!("SQLite prepare failed: {}", self.last_error()));
            }
            Ok(SqliteStmt {
                db: self,
                handle: stmt_handle,
            })
        }
    }

    impl SqliteStmt<'_> {
        pub fn bind_text(&self, index: i32, value: &str) -> Result<(), String> {
            let len = c_int::try_from(value.len())
                .map_err(|_| "Bound string is too long.".to_string())?;
            let transient = std::ptr::without_provenance::<c_void>(usize::MAX);
            let ptr = if value.is_empty() {
                c"".as_ptr()
            } else {
                value.as_ptr().cast::<c_char>()
            };
            let rc = unsafe { (self.db.api.bind_text)(self.handle, index, ptr, len, transient) };
            if rc != SQLITE_OK {
                return Err(format!("SQLite bind_text failed: {}", self.db.last_error()));
            }
            Ok(())
        }

        pub fn bind_i64(&self, index: i32, value: i64) -> Result<(), String> {
            let rc = unsafe { (self.db.api.bind_int64)(self.handle, index, value) };
            if rc != SQLITE_OK {
                return Err(format!("SQLite bind_i64 failed: {}", self.db.last_error()));
            }
            Ok(())
        }

        pub fn step_row(&self) -> Result<bool, String> {
            let rc = unsafe { (self.db.api.step)(self.handle) };
            match rc {
                SQLITE_ROW => Ok(true),
                SQLITE_DONE => Ok(false),
                _ => Err(format!("SQLite step failed: {}", self.db.last_error())),
            }
        }

        pub fn column_string(&self, col: i32) -> Option<String> {
            let ptr = unsafe { (self.db.api.column_text)(self.handle, col) };
            if ptr.is_null() {
                return None;
            }
            let bytes_len = unsafe { (self.db.api.column_bytes)(self.handle, col) };
            if bytes_len <= 0 {
                return Some(String::new());
            }
            let slice = unsafe { std::slice::from_raw_parts(ptr, bytes_len as usize) };
            Some(String::from_utf8_lossy(slice).into_owned())
        }
    }
}

#[cfg(windows)]
pub fn query_winget_index_db(
    db_path: &Path,
    winsqlite_path: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<WingetSearchEntry>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if !is_valid_sqlite_file(db_path) {
        return Err("Winget index database file is missing or invalid.".to_string());
    }

    let capped_limit = limit.clamp(1, MAX_SEARCH_LIMIT);
    let db = winsqlite::SqliteDb::open_readonly(db_path, winsqlite_path)?;

    let mut tables = HashSet::new();
    {
        let stmt = db.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
        while stmt.step_row()? {
            if let Some(name) = stmt.column_string(0) {
                tables.insert(name.to_ascii_lowercase());
            }
        }
    }

    if tables.contains("packages") {
        query_schema_v2_packages(&db, trimmed, capped_limit)
    } else if tables.contains("manifest") && tables.contains("ids") && tables.contains("names") {
        query_schema_v1_manifest(&db, &tables, trimmed, capped_limit)
    } else {
        Err("Unsupported Winget index database schema.".to_string())
    }
}

#[cfg(not(windows))]
pub fn query_winget_index_db(
    _db_path: &Path,
    _winsqlite_path: &Path,
    _query: &str,
    _limit: usize,
) -> Result<Vec<WingetSearchEntry>, String> {
    Err("winsqlite3.dll is only available on Windows.".to_string())
}

#[cfg(windows)]
fn query_schema_v2_packages(
    db: &winsqlite::SqliteDb,
    trimmed_query: &str,
    capped_limit: usize,
) -> Result<Vec<WingetSearchEntry>, String> {
    let mut cols: Vec<String> = Vec::new();
    {
        let stmt = db.prepare("PRAGMA table_info(packages)")?;
        while stmt.step_row()? {
            if let Some(col_name) = stmt.column_string(1) {
                cols.push(col_name);
            }
        }
    }

    let find_col = |candidates: &[&'static str]| -> Option<&'static str> {
        candidates
            .iter()
            .copied()
            .find(|&cand| cols.iter().any(|c| c.eq_ignore_ascii_case(cand)))
    };

    let id_col = find_col(&["id", "package_id"]).ok_or_else(|| {
        "Unsupported Winget index schema: missing id column in packages.".to_string()
    })?;
    let name_col = find_col(&["name"]).ok_or_else(|| {
        "Unsupported Winget index schema: missing name column in packages.".to_string()
    })?;
    let moniker_expr = find_col(&["moniker"])
        .map(|c| format!("COALESCE({c}, '')"))
        .unwrap_or_else(|| "''".to_string());
    let version_expr = find_col(&["latest_version", "version"])
        .map(|c| format!("COALESCE({c}, '')"))
        .unwrap_or_else(|| "''".to_string());

    if trimmed_query.is_ascii() {
        let tokens: Vec<String> = trimmed_query
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .take(6)
            .map(ToString::to_string)
            .collect();
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut where_clauses = Vec::with_capacity(tokens.len());
        let mut like_patterns = Vec::with_capacity(tokens.len());
        for token in &tokens {
            like_patterns.push(format!("%{}%", escape_like_pattern(token)));
            where_clauses.push(format!(
                "({id_col} LIKE ? ESCAPE '\\' OR {name_col} LIKE ? ESCAPE '\\' OR {moniker_expr} LIKE ? ESCAPE '\\')"
            ));
        }

        let full_escaped = escape_like_pattern(trimmed_query);
        let prefix_pat = format!("{full_escaped}%");
        let substr_pat = format!("%{full_escaped}%");
        let fetch_limit =
            i64::try_from((capped_limit * 2).min(MAX_SEARCH_LIMIT * 2)).unwrap_or(200);

        let sql = format!(
            "SELECT {id_col}, {name_col}, {moniker_expr}, {version_expr} \
             FROM packages \
             WHERE {} \
             ORDER BY CASE \
                 WHEN LOWER({id_col}) = LOWER(?) OR LOWER({name_col}) = LOWER(?) OR LOWER({moniker_expr}) = LOWER(?) THEN 0 \
                 WHEN {id_col} LIKE ? ESCAPE '\\' OR {name_col} LIKE ? ESCAPE '\\' OR {moniker_expr} LIKE ? ESCAPE '\\' THEN 1 \
                 WHEN {name_col} LIKE ? ESCAPE '\\' OR {moniker_expr} LIKE ? ESCAPE '\\' THEN 2 \
                 ELSE 3 \
             END, LENGTH({name_col}) ASC, {name_col} ASC \
             LIMIT ?",
            where_clauses.join(" AND ")
        );

        let stmt = db.prepare(&sql)?;
        let mut bind_idx = 1i32;
        for pat in &like_patterns {
            stmt.bind_text(bind_idx, pat)?;
            stmt.bind_text(bind_idx + 1, pat)?;
            stmt.bind_text(bind_idx + 2, pat)?;
            bind_idx += 3;
        }
        // Rank 0: exact match
        stmt.bind_text(bind_idx, trimmed_query)?;
        stmt.bind_text(bind_idx + 1, trimmed_query)?;
        stmt.bind_text(bind_idx + 2, trimmed_query)?;
        bind_idx += 3;
        // Rank 1: prefix match
        stmt.bind_text(bind_idx, &prefix_pat)?;
        stmt.bind_text(bind_idx + 1, &prefix_pat)?;
        stmt.bind_text(bind_idx + 2, &prefix_pat)?;
        bind_idx += 3;
        // Rank 2: name or moniker substring match
        stmt.bind_text(bind_idx, &substr_pat)?;
        stmt.bind_text(bind_idx + 1, &substr_pat)?;
        bind_idx += 2;
        stmt.bind_i64(bind_idx, fetch_limit)?;

        let mut results = Vec::with_capacity(capped_limit);
        let mut seen = HashSet::new();
        while stmt.step_row()? {
            let id = stmt.column_string(0).unwrap_or_default().trim().to_string();
            let name = stmt.column_string(1).unwrap_or_default().trim().to_string();
            let moniker = normalize_moniker(stmt.column_string(2));
            let version = stmt.column_string(3).unwrap_or_default().trim().to_string();

            if name.is_empty() || validate_winget_id(&id).is_err() {
                continue;
            }
            if !seen.insert(id.to_ascii_lowercase()) {
                continue;
            }
            results.push(WingetSearchEntry {
                id,
                name,
                version,
                publisher: None,
                moniker,
                source: WingetSource::Winget,
            });
            if results.len() >= capped_limit {
                break;
            }
        }
        Ok(results)
    } else {
        let sql =
            format!("SELECT {id_col}, {name_col}, {moniker_expr}, {version_expr} FROM packages");
        let stmt = db.prepare(&sql)?;
        collect_and_rank_rows_in_rust(&stmt, trimmed_query, capped_limit)
    }
}

#[cfg(windows)]
fn query_schema_v1_manifest(
    db: &winsqlite::SqliteDb,
    tables: &HashSet<String>,
    trimmed_query: &str,
    capped_limit: usize,
) -> Result<Vec<WingetSearchEntry>, String> {
    let has_versions = tables.contains("versions");
    let has_monikers = tables.contains("monikers");

    let moniker_select = if has_monikers {
        "COALESCE(MAX(monikers.moniker), '')"
    } else {
        "''"
    };
    let version_select = if has_versions {
        "COALESCE(MAX(versions.version), '')"
    } else {
        "''"
    };
    let moniker_join = if has_monikers {
        " LEFT JOIN monikers ON monikers.rowid = manifest.moniker"
    } else {
        ""
    };
    let version_join = if has_versions {
        " LEFT JOIN versions ON versions.rowid = manifest.version"
    } else {
        ""
    };

    let sql = format!(
        "SELECT ids.id, MAX(names.name), {moniker_select}, {version_select} \
         FROM manifest \
         JOIN ids ON ids.rowid = manifest.id \
         JOIN names ON names.rowid = manifest.name\
         {moniker_join}{version_join} \
         GROUP BY ids.id"
    );

    let stmt = db.prepare(&sql)?;
    collect_and_rank_rows_in_rust(&stmt, trimmed_query, capped_limit)
}

#[cfg(windows)]
fn collect_and_rank_rows_in_rust(
    stmt: &winsqlite::SqliteStmt<'_>,
    trimmed_query: &str,
    capped_limit: usize,
) -> Result<Vec<WingetSearchEntry>, String> {
    let query_lower = trimmed_query.to_lowercase();
    let tokens: Vec<String> = query_lower
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .take(6)
        .map(ToString::to_string)
        .collect();
    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let mut ranked: Vec<(u8, usize, String, WingetSearchEntry)> = Vec::new();
    let mut seen = HashSet::new();

    while stmt.step_row()? {
        let id = stmt.column_string(0).unwrap_or_default().trim().to_string();
        let name = stmt.column_string(1).unwrap_or_default().trim().to_string();
        let moniker = normalize_moniker(stmt.column_string(2));
        let version = stmt.column_string(3).unwrap_or_default().trim().to_string();

        if name.is_empty() || validate_winget_id(&id).is_err() {
            continue;
        }
        let id_lower = id.to_ascii_lowercase();
        let name_lower = name.to_lowercase();
        let moniker_lower = moniker
            .as_deref()
            .map(str::to_lowercase)
            .unwrap_or_default();

        let Some(rank) = compute_match_rank(
            &id_lower,
            &name_lower,
            &moniker_lower,
            &query_lower,
            &tokens,
        ) else {
            continue;
        };
        if !seen.insert(id_lower) {
            continue;
        }

        ranked.push((
            rank,
            name.chars().count(),
            name_lower,
            WingetSearchEntry {
                id,
                name,
                version,
                publisher: None,
                moniker,
                source: WingetSource::Winget,
            },
        ));
    }

    ranked.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    Ok(ranked
        .into_iter()
        .take(capped_limit)
        .map(|(_, _, _, entry)| entry)
        .collect())
}

pub fn read_cached_winget_index_meta(workspace_dir: &Path) -> Option<String> {
    let cache_dir = workspace_dir.join("cache");
    let db_path = cache_dir.join(WINGET_INDEX_DB_FILE);
    let meta_path = cache_dir.join(WINGET_INDEX_META_FILE);

    if !is_valid_sqlite_file(&db_path) {
        return None;
    }
    if let Ok(meta_stat) = fs::symlink_metadata(&meta_path)
        && !is_symlink_or_reparse(&meta_stat)
        && meta_stat.is_file()
        && let Ok(raw) = fs::read_to_string(&meta_path)
        && let Ok(parsed) = serde_json::from_str::<WingetIndexMeta>(&raw)
        && !parsed.updated_at.trim().is_empty()
    {
        return Some(parsed.updated_at);
    }

    fs::metadata(&db_path)
        .and_then(|m| m.modified())
        .ok()
        .map(|ts| chrono::DateTime::<chrono::Utc>::from(ts).to_rfc3339())
}

fn is_cache_fresh(db_path: &Path) -> bool {
    if !is_valid_sqlite_file(db_path) {
        return false;
    }
    let Ok(meta) = fs::metadata(db_path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    match SystemTime::now().duration_since(modified) {
        Ok(age) => age < Duration::from_secs(WINGET_INDEX_TTL_SECS),
        Err(_) => false,
    }
}

fn validate_safe_archive_entry(entry: &str) -> Option<&str> {
    let trimmed = entry.trim();
    if trimmed.is_empty() || trimmed.starts_with(['/', '\\']) || trimmed.contains(':') {
        return None;
    }
    let normalized = trimmed.replace('\\', "/");
    if normalized.split('/').any(|seg| seg == "..") {
        return None;
    }
    if normalized.eq_ignore_ascii_case("index.db")
        || normalized.to_ascii_lowercase().ends_with("/index.db")
    {
        Some(trimmed)
    } else {
        None
    }
}

async fn download_and_extract_winget_index<F>(
    curl_path: &Path,
    tar_path: &Path,
    cache_dir: &Path,
    db_path: &Path,
    meta_path: &Path,
    on_log: &mut F,
) -> Result<(PathBuf, Option<String>), String>
where
    F: FnMut(String),
{
    if let Ok(cache_meta) = fs::symlink_metadata(cache_dir) {
        if is_symlink_or_reparse(&cache_meta) {
            return Err(format!(
                "Unsafe cache directory '{}': symbolic links and reparse points are not allowed.",
                cache_dir.display()
            ));
        }
        if !cache_meta.is_dir() {
            return Err(format!(
                "Cache path '{}' exists and is not a directory.",
                cache_dir.display()
            ));
        }
    } else {
        fs::create_dir_all(cache_dir)
            .map_err(|e| format!("Failed creating cache directory: {e}"))?;
    }

    if let Ok(db_meta) = fs::symlink_metadata(db_path)
        && is_symlink_or_reparse(&db_meta)
    {
        return Err(format!(
            "Unsafe Winget index path '{}': symbolic links and reparse points are not allowed.",
            db_path.display()
        ));
    }
    if let Ok(meta_stat) = fs::symlink_metadata(meta_path)
        && is_symlink_or_reparse(&meta_stat)
    {
        return Err(format!(
            "Unsafe Winget metadata path '{}': symbolic links and reparse points are not allowed.",
            meta_path.display()
        ));
    }

    let pid = std::process::id();
    let nonce = next_nonce();
    let tmp_msix = cache_dir.join(format!("winget_source2_{pid}_{nonce}.msix.tmp"));
    let tmp_extract_dir = cache_dir.join(format!("winget_extract_{pid}_{nonce}"));
    fs::create_dir_all(&tmp_extract_dir)
        .map_err(|e| format!("Failed creating temporary extraction folder: {e}"))?;

    let mut cleanup = TempPathGuard::new();
    cleanup.track_file(tmp_msix.clone());
    cleanup.track_dir(tmp_extract_dir.clone());

    on_log("Downloading Winget package index (source2.msix)...".to_string());

    let curl_args = vec![
        "-sS".to_string(),
        "-L".to_string(),
        "--max-redirs".to_string(),
        "5".to_string(),
        "--max-filesize".to_string(),
        MAX_WINGET_MSIX_BYTES.to_string(),
        "--proto".to_string(),
        "=https".to_string(),
        "--proto-redir".to_string(),
        "=https".to_string(),
        "--fail".to_string(),
        "--connect-timeout".to_string(),
        "15".to_string(),
        "--max-time".to_string(),
        "90".to_string(),
        "-w".to_string(),
        "%{url_effective}".to_string(),
        "-o".to_string(),
        tmp_msix.to_string_lossy().to_string(),
        WINGET_SOURCE2_URL.to_string(),
    ];

    let curl_out =
        run_tracked_command(&curl_path.to_string_lossy(), &curl_args, None, None, |_| {}).await?;
    if curl_out.exit_code != 0 {
        return Err(format!(
            "Failed downloading Winget index (exit {}): {}",
            curl_out.exit_code,
            curl_out.stderr.trim()
        ));
    }
    let eff_url = curl_out.stdout.trim();
    if !is_allowed_microsoft_host(eff_url) {
        return Err(format!(
            "Blocked download redirect to untrusted host: '{eff_url}'."
        ));
    }

    let tar_str = tar_path.to_string_lossy().to_string();
    let tmp_msix_str = tmp_msix.to_string_lossy().to_string();
    let tmp_extract_str = tmp_extract_dir.to_string_lossy().to_string();

    let direct_args = vec![
        "-xf".to_string(),
        tmp_msix_str.clone(),
        "-C".to_string(),
        tmp_extract_str.clone(),
        "Public/index.db".to_string(),
    ];
    let _ = run_tracked_command(&tar_str, &direct_args, None, None, |_| {}).await?;

    let mut extracted_db = tmp_extract_dir.join("Public").join("index.db");
    if !extracted_db.is_file() {
        let list_args = vec!["-tf".to_string(), tmp_msix_str.clone()];
        let list_out = run_tracked_command(&tar_str, &list_args, None, None, |_| {}).await?;
        if list_out.exit_code != 0 {
            return Err(format!(
                "Failed inspecting Winget index archive (exit {}): {}",
                list_out.exit_code,
                list_out.stderr.trim()
            ));
        }

        let matched_entry = list_out
            .stdout
            .lines()
            .find_map(validate_safe_archive_entry)
            .ok_or_else(|| "Winget source2.msix archive does not contain index.db.".to_string())?
            .to_string();

        let extract_args = vec![
            "-xf".to_string(),
            tmp_msix_str,
            "-C".to_string(),
            tmp_extract_str,
            matched_entry.clone(),
        ];
        let ext_out = run_tracked_command(&tar_str, &extract_args, None, None, |_| {}).await?;
        if ext_out.exit_code != 0 {
            return Err(format!(
                "Failed extracting index.db from Winget archive (exit {}): {}",
                ext_out.exit_code,
                ext_out.stderr.trim()
            ));
        }

        let mut candidate = tmp_extract_dir.clone();
        for seg in matched_entry.split(['/', '\\']) {
            if !seg.is_empty() {
                candidate.push(seg);
            }
        }
        extracted_db = candidate;
    }

    if !is_valid_sqlite_file(&extracted_db) {
        return Err("Extracted Winget index.db is missing or invalid SQLite file.".to_string());
    }

    let db_size = fs::metadata(&extracted_db).map(|m| m.len()).unwrap_or(0);
    if let Ok(existing_meta) = fs::symlink_metadata(db_path) {
        if is_symlink_or_reparse(&existing_meta) {
            return Err(format!(
                "Refusing to overwrite symlink or reparse point at '{}'.",
                db_path.display()
            ));
        }
        let _ = fs::remove_file(db_path);
    }

    if fs::rename(&extracted_db, db_path).is_err() {
        fs::copy(&extracted_db, db_path)
            .map_err(|e| format!("Failed saving Winget index database: {e}"))?;
        let _ = fs::remove_file(&extracted_db);
    }

    let updated_at = chrono::Utc::now().to_rfc3339();
    let meta = WingetIndexMeta {
        updated_at: updated_at.clone(),
        size_bytes: db_size,
    };
    if let Ok(json_text) = serde_json::to_string_pretty(&meta) {
        let _ = fs::write(meta_path, json_text);
    }

    on_log(format!(
        "Winget package index updated ({:.1} MiB).",
        (db_size as f64) / (1024.0 * 1024.0)
    ));

    Ok((db_path.to_path_buf(), Some(updated_at)))
}

async fn ensure_winget_index_db<F>(
    curl_path: &Path,
    tar_path: &Path,
    workspace_dir: &Path,
    force_refresh: bool,
    on_log: &mut F,
) -> Result<(PathBuf, Option<String>, bool), String>
where
    F: FnMut(String),
{
    let cache_dir = workspace_dir.join("cache");
    let db_path = cache_dir.join(WINGET_INDEX_DB_FILE);
    let meta_path = cache_dir.join(WINGET_INDEX_META_FILE);

    if let Ok(db_meta) = fs::symlink_metadata(&db_path)
        && is_symlink_or_reparse(&db_meta)
    {
        return Err(format!(
            "Unsafe Winget index path '{}': symbolic links and reparse points are not allowed.",
            db_path.display()
        ));
    }

    if !force_refresh && is_cache_fresh(&db_path) {
        let updated_at = read_cached_winget_index_meta(workspace_dir);
        return Ok((db_path, updated_at, true));
    }

    match download_and_extract_winget_index(
        curl_path, tar_path, &cache_dir, &db_path, &meta_path, on_log,
    )
    .await
    {
        Ok((path, updated_at)) => Ok((path, updated_at, false)),
        Err(err) => {
            if !force_refresh && is_valid_sqlite_file(&db_path) {
                on_log(format!(
                    "Winget index refresh failed ({err}); falling back to cached index."
                ));
                let updated_at = read_cached_winget_index_meta(workspace_dir);
                Ok((db_path, updated_at, true))
            } else {
                Err(err)
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct MsstoreResponsePayload {
    #[serde(rename = "Data", default)]
    data: Vec<MsstorePackageRecord>,
}

#[derive(Debug, Deserialize)]
struct MsstorePackageRecord {
    #[serde(rename = "PackageIdentifier", default)]
    package_identifier: String,
    #[serde(rename = "PackageName", default)]
    package_name: String,
    #[serde(rename = "Publisher", default)]
    publisher: Option<String>,
    #[serde(rename = "Versions", default)]
    versions: Vec<MsstoreVersionRecord>,
}

#[derive(Debug, Deserialize)]
struct MsstoreVersionRecord {
    #[serde(rename = "PackageVersion", default)]
    package_version: Option<String>,
    #[serde(rename = "Version", default)]
    version: Option<String>,
}

pub fn parse_msstore_search_json(
    raw: &str,
    limit: usize,
) -> Result<Vec<WingetSearchEntry>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let capped_limit = limit.clamp(1, MAX_SEARCH_LIMIT);
    let payload: MsstoreResponsePayload = serde_json::from_str(trimmed)
        .map_err(|e| format!("Failed parsing Microsoft Store search response: {e}"))?;

    let mut entries = Vec::with_capacity(payload.data.len().min(capped_limit));
    let mut seen = HashSet::new();

    for rec in payload.data {
        let id = rec.package_identifier.trim().to_string();
        let name = rec.package_name.trim().to_string();
        if id.is_empty() || name.is_empty() || validate_winget_id(&id).is_err() {
            continue;
        }
        if !seen.insert(id.to_ascii_lowercase()) {
            continue;
        }

        let publisher = rec.publisher.and_then(|p| {
            let clean = p.trim().to_string();
            if clean.is_empty() { None } else { Some(clean) }
        });

        let version = rec
            .versions
            .into_iter()
            .find_map(|v| {
                v.package_version
                    .or(v.version)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_default();

        entries.push(WingetSearchEntry {
            id,
            name,
            version,
            publisher,
            moniker: None,
            source: WingetSource::Msstore,
        });

        if entries.len() >= capped_limit {
            break;
        }
    }

    Ok(entries)
}

pub async fn search_msstore_packages(
    curl_path: &Path,
    query: &str,
    limit: usize,
) -> Result<WingetSearchResponse, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(WingetSearchResponse {
            items: Vec::new(),
            source: WingetSource::Msstore,
            index_updated_at: None,
            from_cache: false,
        });
    }

    let capped_limit = limit.clamp(1, MAX_SEARCH_LIMIT);
    let body = serde_json::json!({
        "MaximumResults": capped_limit,
        "Query": {
            "KeyWord": trimmed,
            "MatchType": "Substring"
        }
    });
    let body_bytes = serde_json::to_vec(&body)
        .map_err(|e| format!("Failed serializing Microsoft Store query: {e}"))?;

    let pid = std::process::id();
    let nonce = next_nonce();
    let tmp_json = std::env::temp_dir().join(format!("isocraft_msstore_query_{pid}_{nonce}.json"));
    let tmp_resp_json =
        std::env::temp_dir().join(format!("isocraft_msstore_resp_{pid}_{nonce}.json"));
    let mut cleanup = TempPathGuard::new();
    cleanup.track_file(tmp_json.clone());
    cleanup.track_file(tmp_resp_json.clone());
    fs::write(&tmp_json, &body_bytes)
        .map_err(|e| format!("Failed writing Microsoft Store query payload: {e}"))?;

    let args = vec![
        "-sS".to_string(),
        "-L".to_string(),
        "--max-redirs".to_string(),
        "5".to_string(),
        "--max-filesize".to_string(),
        MAX_MSSTORE_JSON_BYTES.to_string(),
        "--proto".to_string(),
        "=https".to_string(),
        "--proto-redir".to_string(),
        "=https".to_string(),
        "--fail".to_string(),
        "--connect-timeout".to_string(),
        "15".to_string(),
        "--max-time".to_string(),
        "25".to_string(),
        "-H".to_string(),
        "Content-Type: application/json".to_string(),
        "-H".to_string(),
        "Accept: application/json".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        "--data-binary".to_string(),
        format!("@{}", tmp_json.to_string_lossy()),
        "-o".to_string(),
        tmp_resp_json.to_string_lossy().to_string(),
        "-w".to_string(),
        "%{url_effective}".to_string(),
        MSSTORE_SEARCH_URL.to_string(),
    ];

    let out = run_tracked_command(&curl_path.to_string_lossy(), &args, None, None, |_| {}).await?;
    if out.exit_code != 0 {
        return Err(format!(
            "Microsoft Store search failed (exit {}): {}",
            out.exit_code,
            out.stderr.trim()
        ));
    }

    let eff_url = out.stdout.trim();
    if !is_allowed_microsoft_host(eff_url) {
        return Err(format!(
            "Blocked download redirect to untrusted host: '{eff_url}'."
        ));
    }

    let resp_body = if tmp_resp_json.is_file() {
        let bytes = read_file_bounded(&tmp_resp_json, 10 * 1024 * 1024, true)?;
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        String::new()
    };

    let items = parse_msstore_search_json(&resp_body, capped_limit)?;
    Ok(WingetSearchResponse {
        items,
        source: WingetSource::Msstore,
        index_updated_at: None,
        from_cache: false,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn search_winget_catalog<F>(
    curl_path: &Path,
    tar_path: &Path,
    winsqlite_path: &Path,
    workspace_dir: &Path,
    query: &str,
    source: WingetSource,
    force_refresh: bool,
    limit: usize,
    index_lock: &Arc<tokio::sync::RwLock<()>>,
    mut on_log: F,
) -> Result<WingetSearchResponse, String>
where
    F: FnMut(String),
{
    let capped_limit = limit.clamp(1, MAX_SEARCH_LIMIT);

    match source {
        WingetSource::Msstore => search_msstore_packages(curl_path, query, capped_limit).await,
        WingetSource::Winget => {
            let trimmed = query.trim().to_string();
            if trimmed.is_empty() && !force_refresh {
                let _read_guard = index_lock.read().await;
                return Ok(WingetSearchResponse {
                    items: Vec::new(),
                    source: WingetSource::Winget,
                    index_updated_at: read_cached_winget_index_meta(workspace_dir),
                    from_cache: true,
                });
            }

            if !force_refresh {
                let read_guard = Arc::clone(index_lock).read_owned().await;
                let db_path = workspace_dir.join("cache").join(WINGET_INDEX_DB_FILE);
                let safe_non_symlink = fs::symlink_metadata(&db_path)
                    .map(|m| !is_symlink_or_reparse(&m))
                    .unwrap_or(false);
                if safe_non_symlink && is_cache_fresh(&db_path) {
                    let index_updated_at = read_cached_winget_index_meta(workspace_dir);
                    let winsqlite_owned = winsqlite_path.to_path_buf();
                    let items = tokio::task::spawn_blocking(move || {
                        let _hold = read_guard;
                        query_winget_index_db(&db_path, &winsqlite_owned, &trimmed, capped_limit)
                    })
                    .await
                    .map_err(|e| format!("Winget index search task failed: {e}"))??;

                    return Ok(WingetSearchResponse {
                        items,
                        source: WingetSource::Winget,
                        index_updated_at,
                        from_cache: true,
                    });
                }
            }

            let (db_path, index_updated_at, from_cache) = {
                let _write_guard = index_lock.write().await;
                ensure_winget_index_db(
                    curl_path,
                    tar_path,
                    workspace_dir,
                    force_refresh,
                    &mut on_log,
                )
                .await?
            };

            if trimmed.is_empty() {
                return Ok(WingetSearchResponse {
                    items: Vec::new(),
                    source: WingetSource::Winget,
                    index_updated_at,
                    from_cache,
                });
            }

            let read_guard = Arc::clone(index_lock).read_owned().await;
            let winsqlite_owned = winsqlite_path.to_path_buf();
            let items = tokio::task::spawn_blocking(move || {
                let _hold = read_guard;
                query_winget_index_db(&db_path, &winsqlite_owned, &trimmed, capped_limit)
            })
            .await
            .map_err(|e| format!("Winget index search task failed: {e}"))??;

            Ok(WingetSearchResponse {
                items,
                source: WingetSource::Winget,
                index_updated_at,
                from_cache,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_microsoft_hosts_for_winget_and_msstore() {
        assert!(is_allowed_microsoft_host(WINGET_SOURCE2_URL));
        assert!(is_allowed_microsoft_host(MSSTORE_SEARCH_URL));
        assert!(!is_allowed_microsoft_host(""));
        assert!(!is_allowed_microsoft_host(
            "https://evil.example.com/source2.msix"
        ));
    }

    #[test]
    fn escapes_like_patterns_properly() {
        assert_eq!(escape_like_pattern(r"100%_a\b"), r"100\%\_a\\b");
    }

    #[test]
    fn builds_sqlite_readonly_uri_safely() {
        let uri = path_to_sqlite_ro_uri(Path::new(r"\\?\C:\Work\100%#?dir\index.db"));
        assert_eq!(
            uri,
            "file:///C:/Work/100%25%23%3Fdir/index.db?mode=ro&immutable=1"
        );
    }

    #[test]
    fn parses_msstore_json_and_handles_empty_204() {
        assert!(parse_msstore_search_json("", 25).unwrap().is_empty());
        assert!(parse_msstore_search_json("   ", 25).unwrap().is_empty());

        let json = r#"{
            "Data": [
                {
                    "PackageIdentifier": "9NKSQGP7F2NH",
                    "PackageName": "WhatsApp",
                    "Publisher": "WhatsApp LLC",
                    "Versions": [{"PackageVersion": "2.24.1.0"}]
                },
                {
                    "PackageIdentifier": "9nksqgp7f2nh",
                    "PackageName": "WhatsApp Duplicate",
                    "Publisher": "WhatsApp LLC",
                    "Versions": [{"Version": "2.24.0.0"}]
                },
                {
                    "PackageIdentifier": "-InvalidId",
                    "PackageName": "Bad Package"
                },
                {
                    "PackageIdentifier": "9WZDNCRFJ3TJ",
                    "PackageName": "Netflix",
                    "Publisher": "Netflix, Inc.",
                    "Versions": [{"Version": "6.99.5.0"}]
                }
            ]
        }"#;

        let parsed = parse_msstore_search_json(json, 10).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "9NKSQGP7F2NH");
        assert_eq!(parsed[0].name, "WhatsApp");
        assert_eq!(parsed[0].publisher.as_deref(), Some("WhatsApp LLC"));
        assert_eq!(parsed[0].version, "2.24.1.0");
        assert_eq!(parsed[0].source, WingetSource::Msstore);
        assert_eq!(parsed[1].id, "9WZDNCRFJ3TJ");
        assert_eq!(parsed[1].version, "6.99.5.0");

        let limited = parse_msstore_search_json(json, 1).unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn rejects_unsafe_archive_entries() {
        assert_eq!(
            validate_safe_archive_entry("Public/index.db"),
            Some("Public/index.db")
        );
        assert_eq!(validate_safe_archive_entry("index.db"), Some("index.db"));
        assert_eq!(validate_safe_archive_entry("../Public/index.db"), None);
        assert_eq!(validate_safe_archive_entry("/Public/index.db"), None);
        assert_eq!(validate_safe_archive_entry(r"C:\Public\index.db"), None);
        assert_eq!(validate_safe_archive_entry("Public/other.txt"), None);
    }

    #[cfg(windows)]
    #[test]
    fn queries_schema_v2_and_v1_via_winsqlite3_including_cyrillic_paths() {
        let winsqlite_tool = crate::env::find_winsqlite3();
        let dll_path = PathBuf::from(
            winsqlite_tool
                .path
                .expect("System winsqlite3.dll must exist on Windows"),
        );

        let dir = std::env::temp_dir().join(format!(
            "isocraft_winget_тест_{}_{}",
            std::process::id(),
            next_nonce()
        ));
        fs::create_dir_all(&dir).unwrap();

        // 1. Non-SQLite file must fail cleanly
        let bad_db = dir.join("bad.db");
        fs::write(&bad_db, b"not a sqlite database").unwrap();
        assert!(query_winget_index_db(&bad_db, &dll_path, "vlc", 20).is_err());

        // 2. Schema 2.0 (packages table)
        let db_v2 = dir.join("index_v2.db");
        {
            let db = winsqlite::SqliteDb::open_readwrite_create(&db_v2, &dll_path).unwrap();
            db.exec_batch(
                "CREATE TABLE packages (id TEXT, name TEXT, moniker TEXT, latest_version TEXT);
                 INSERT INTO packages VALUES ('7zip.7zip', '7-Zip', '7zip', '24.09');
                 INSERT INTO packages VALUES ('VideoLAN.VLC', 'VLC media player', 'vlc', '3.0.21');
                 INSERT INTO packages VALUES ('Acme.VlcHelper', 'VLC Helper Tool', 'None', '1.0');
                 INSERT INTO packages VALUES ('Microsoft.VisualStudioCode', 'Microsoft Visual Studio Code', 'vscode', '1.96.0');
                 INSERT INTO packages VALUES ('Notepad++.Notepad++', 'Notepad++', 'notepad++', '8.7.5');
                 INSERT INTO packages VALUES ('Vendor.CyrillicApp', 'Мой Редактор Текста', NULL, '2.0');
                 INSERT INTO packages VALUES ('Vendor.Percent100', '100% Cleaner', NULL, '1.2');
                 INSERT INTO packages VALUES ('-Bad.Id', 'Should Be Filtered', NULL, '1.0');",
            )
            .unwrap();
        }

        // Exact/moniker match ranks VideoLAN.VLC ahead of Acme.VlcHelper
        let vlc_res = query_winget_index_db(&db_v2, &dll_path, "vlc", 20).unwrap();
        assert_eq!(vlc_res.len(), 2);
        assert_eq!(vlc_res[0].id, "VideoLAN.VLC");
        assert_eq!(vlc_res[0].moniker.as_deref(), Some("vlc"));
        assert_eq!(vlc_res[1].id, "Acme.VlcHelper");
        assert_eq!(vlc_res[1].moniker, None); // "None" normalized to None

        // Multi-token search
        let code_res = query_winget_index_db(&db_v2, &dll_path, "visual code", 20).unwrap();
        assert_eq!(code_res.len(), 1);
        assert_eq!(code_res[0].id, "Microsoft.VisualStudioCode");

        // Escaped LIKE wildcard %
        let pct_res = query_winget_index_db(&db_v2, &dll_path, "100%", 20).unwrap();
        assert_eq!(pct_res.len(), 1);
        assert_eq!(pct_res[0].id, "Vendor.Percent100");

        // Cyrillic case-insensitive search
        let cyr_res = query_winget_index_db(&db_v2, &dll_path, "редактор", 20).unwrap();
        assert_eq!(cyr_res.len(), 1);
        assert_eq!(cyr_res[0].id, "Vendor.CyrillicApp");

        // Invalid IDs filtered
        let bad_res = query_winget_index_db(&db_v2, &dll_path, "Filtered", 20).unwrap();
        assert!(bad_res.is_empty());

        // 3. Legacy Schema 1.x (manifest + ids + names + versions + monikers)
        let db_v1 = dir.join("index_v1.db");
        {
            let db = winsqlite::SqliteDb::open_readwrite_create(&db_v1, &dll_path).unwrap();
            db.exec_batch(
                "CREATE TABLE ids (id TEXT);
                 CREATE TABLE names (name TEXT);
                 CREATE TABLE versions (version TEXT);
                 CREATE TABLE monikers (moniker TEXT);
                 CREATE TABLE manifest (id INTEGER, name INTEGER, version INTEGER, moniker INTEGER);
                 INSERT INTO ids (rowid, id) VALUES (1, 'Mozilla.Firefox');
                 INSERT INTO names (rowid, name) VALUES (1, 'Mozilla Firefox');
                 INSERT INTO versions (rowid, version) VALUES (1, '133.0');
                 INSERT INTO monikers (rowid, moniker) VALUES (1, 'firefox');
                 INSERT INTO manifest (id, name, version, moniker) VALUES (1, 1, 1, 1);",
            )
            .unwrap();
        }

        let ff_res = query_winget_index_db(&db_v1, &dll_path, "firefox", 20).unwrap();
        assert_eq!(ff_res.len(), 1);
        assert_eq!(ff_res[0].id, "Mozilla.Firefox");
        assert_eq!(ff_res[0].name, "Mozilla Firefox");
        assert_eq!(ff_res[0].version, "133.0");
        assert_eq!(ff_res[0].moniker.as_deref(), Some("firefox"));

        let _ = fs::remove_dir_all(&dir);
    }
}
