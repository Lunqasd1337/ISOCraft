use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceStartType {
    Automatic,
    DelayedAuto,
    Manual,
    Disabled,
}

impl ServiceStartType {
    /// Maps Windows SCM `Start` and `DelayedAutostart` DWORDs to `ServiceStartType`.
    /// Returns `None` for kernel/boot start values (`0`, `1`) or unknown codes.
    pub fn from_registry_values(start: u32, delayed_autostart: Option<u32>) -> Option<Self> {
        match start {
            2 => {
                if delayed_autostart == Some(1) {
                    Some(ServiceStartType::DelayedAuto)
                } else {
                    Some(ServiceStartType::Automatic)
                }
            }
            3 => Some(ServiceStartType::Manual),
            4 => Some(ServiceStartType::Disabled),
            _ => None,
        }
    }

    /// Returns `(Start DWORD, whether DelayedAutostart should be set to 1)`.
    pub fn to_registry_values(self) -> (u32, bool) {
        match self {
            ServiceStartType::Automatic => (2, false),
            ServiceStartType::DelayedAuto => (2, true),
            ServiceStartType::Manual => (3, false),
            ServiceStartType::Disabled => (4, false),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ServiceStartType::Automatic => "Automatic",
            ServiceStartType::DelayedAuto => "DelayedAuto",
            ServiceStartType::Manual => "Manual",
            ServiceStartType::Disabled => "Disabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceRiskLevel {
    Safe,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceCategory {
    Core,
    Security,
    Network,
    Updates,
    Telemetry,
    Devices,
    Gaming,
    Enterprise,
    Other,
}

#[derive(Debug, Clone)]
pub struct RawOfflineService {
    pub name: String,
    pub service_type: u32,
    pub start: u32,
    pub delayed_autostart: Option<u32>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub group: Option<String>,
    pub depends_on: Vec<String>,
    pub has_trigger_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsServiceInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub start_type: ServiceStartType,
    pub service_type: u32,
    pub is_per_user_template: bool,
    pub has_trigger_start: bool,
    pub group: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub required_by: Vec<String>,
    pub risk_level: ServiceRiskLevel,
    pub category: ServiceCategory,
    pub is_locked_critical: bool,
    pub protection_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStartChange {
    pub name: String,
    pub start_type: ServiceStartType,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServicesBuildPlan {
    pub changes: Vec<ServiceStartChange>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceApplyReport {
    pub applied: usize,
    pub skipped_missing: usize,
    pub skipped_non_win32: usize,
}

/// Returns true for Win32 services and per-user service templates, excluding
/// kernel/file-system drivers (`0x01`, `0x02`, `0x08`) and runtime per-user instances (`0x80`).
pub fn is_manageable_win32_service_type(service_type: u32) -> bool {
    (service_type & 0x30) != 0 && (service_type & 0x80) == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceClassificationRule {
    pub risk_level: ServiceRiskLevel,
    pub category: ServiceCategory,
    pub is_locked_critical: bool,
    pub protection_reason: Option<&'static str>,
}

/// Strips per-user runtime suffix (e.g. `WpnUserService_1a2b3c` -> `wpnuserservice`) if present.
fn canonical_service_lookup_key(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    if let Some((base, suffix)) = lower.rsplit_once('_')
        && !base.is_empty()
        && !suffix.is_empty()
        && suffix.len() <= 8
        && suffix.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return base.to_string();
    }
    lower
}

pub fn classify_service_base(name: &str) -> ServiceClassificationRule {
    let key = canonical_service_lookup_key(name);
    match key.as_str() {
        // --- CRITICAL (hard-blocked from Disabled) ---
        "rpcss" | "rpceptmapper" | "dcomlaunch" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Critical,
            category: ServiceCategory::Core,
            is_locked_critical: true,
            protection_reason: Some("core_rpc"),
        },
        "lsm"
        | "plugplay"
        | "power"
        | "profsvc"
        | "samss"
        | "usermanager"
        | "brokerinfrastructure"
        | "systemeventsbroker"
        | "staterepository"
        | "coremessagingregistrar"
        | "appinfo"
        | "cryptsvc"
        | "eventlog" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Critical,
            category: ServiceCategory::Core,
            is_locked_critical: true,
            protection_reason: Some("core_boot"),
        },
        "trustedinstaller" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Critical,
            category: ServiceCategory::Updates,
            is_locked_critical: true,
            protection_reason: Some("servicing"),
        },
        "nsi" | "dhcp" | "dnscache" | "bfe" | "mpssvc" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Critical,
            category: ServiceCategory::Network,
            is_locked_critical: true,
            protection_reason: Some("network_core"),
        },
        "windefend" | "securityhealthservice" | "wscsvc" | "sense" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Critical,
            category: ServiceCategory::Security,
            is_locked_critical: true,
            protection_reason: Some("security_core"),
        },

        // --- HIGH RISK (confirmation modal required) ---
        "wuauserv" | "usosvc" | "waasmedicsvc" | "bits" | "dosvc" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::High,
            category: ServiceCategory::Updates,
            is_locked_critical: false,
            protection_reason: Some("windows_update"),
        },
        "msiserver" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::High,
            category: ServiceCategory::Core,
            is_locked_critical: false,
            protection_reason: Some("servicing"),
        },
        "schedule"
        | "winmgmt"
        | "storsvc"
        | "themes"
        | "fontcache"
        | "sens"
        | "shellhwdetection"
        | "timebrokersvc"
        | "devicesflowusersvc"
        | "cbdhsvc"
        | "textinputmanagementservice"
        | "dispbrokerdesktopsvc" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::High,
            category: ServiceCategory::Core,
            is_locked_critical: false,
            protection_reason: Some("system_important"),
        },
        "gpsvc" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::High,
            category: ServiceCategory::Enterprise,
            is_locked_critical: false,
            protection_reason: Some("system_important"),
        },
        "nlasvc" | "netprofm" | "lanmanworkstation" | "lanmanserver" | "netman" | "iphlpsvc" => {
            ServiceClassificationRule {
                risk_level: ServiceRiskLevel::High,
                category: ServiceCategory::Network,
                is_locked_critical: false,
                protection_reason: Some("network_important"),
            }
        }
        "wlansvc" | "wwansvc" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::High,
            category: ServiceCategory::Network,
            is_locked_critical: false,
            protection_reason: Some("wireless"),
        },
        "audiosrv" | "audioendpointbuilder" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::High,
            category: ServiceCategory::Devices,
            is_locked_critical: false,
            protection_reason: Some("audio"),
        },
        "vaultsvc" | "keyiso" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::High,
            category: ServiceCategory::Security,
            is_locked_critical: false,
            protection_reason: Some("security_important"),
        },

        // --- SAFE TO DISABLE ---
        "diagtrack"
        | "dmwappushservice"
        | "wersvc"
        | "dps"
        | "wdiservicehost"
        | "wdisystemhost"
        | "pcasvc"
        | "wisvc"
        | "diagnosticshub.standardcollector.service" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Safe,
            category: ServiceCategory::Telemetry,
            is_locked_critical: false,
            protection_reason: None,
        },
        "xblauthmanager" | "xblgamesave" | "xboxgipsvc" | "xboxnetapisvc" => {
            ServiceClassificationRule {
                risk_level: ServiceRiskLevel::Safe,
                category: ServiceCategory::Gaming,
                is_locked_critical: false,
                protection_reason: None,
            }
        }
        "fax" | "phonesvc" | "alljoynrouter" | "ajrouter" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Safe,
            category: ServiceCategory::Devices,
            is_locked_critical: false,
            protection_reason: None,
        },
        "remoteregistry" | "workfolderssvc" | "assignedaccessmanagersvc" | "shpamsvc" => {
            ServiceClassificationRule {
                risk_level: ServiceRiskLevel::Safe,
                category: ServiceCategory::Enterprise,
                is_locked_critical: false,
                protection_reason: None,
            }
        }
        "retaildemo"
        | "mapsbroker"
        | "wmpnetworksvc"
        | "sysmain"
        | "trkwks"
        | "lfsvc"
        | "walletservice"
        | "semgrsvc"
        | "naturalauthentication" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Safe,
            category: ServiceCategory::Other,
            is_locked_critical: false,
            protection_reason: None,
        },

        // --- COMMON MEDIUM RISK WITH CATEGORY HINTS ---
        "spooler" | "bthserv" | "btagservice" | "printnotify" | "stisvc" | "wiaservc"
        | "cameraserver" | "frameserver" | "tabletinputservice" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Medium,
            category: ServiceCategory::Devices,
            is_locked_critical: false,
            protection_reason: None,
        },
        "wbiosrvc" | "bdesvc" | "scardsvr" | "scdeviceenum" | "seclogon" | "certpropsvc" => {
            ServiceClassificationRule {
                risk_level: ServiceRiskLevel::Medium,
                category: ServiceCategory::Security,
                is_locked_critical: false,
                protection_reason: None,
            }
        }
        "termservice" | "sessionenv" | "umrdpservice" | "vmms" | "vmcompute" | "hvhost"
        | "webclient" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Medium,
            category: ServiceCategory::Enterprise,
            is_locked_critical: false,
            protection_reason: None,
        },
        "w32time" | "lmhosts" | "ssdpsrv" | "upnphost" | "fdrespub" | "fdphost"
        | "sharedaccess" | "icssvc" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Medium,
            category: ServiceCategory::Network,
            is_locked_critical: false,
            protection_reason: None,
        },
        "wsearch" => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Medium,
            category: ServiceCategory::Core,
            is_locked_critical: false,
            protection_reason: None,
        },
        _ => ServiceClassificationRule {
            risk_level: ServiceRiskLevel::Medium,
            category: ServiceCategory::Other,
            is_locked_critical: false,
            protection_reason: None,
        },
    }
}

/// Recomputes `risk_level`, `category`, `is_locked_critical`, `protection_reason`,
/// and reverse dependency graph (`required_by`) across all services.
/// Automatically upgrades non-critical services depended on by a `Critical` service to `High`.
pub fn reclassify_services(services: &mut [WindowsServiceInfo]) {
    let mut lower_to_canonical: HashMap<String, String> = HashMap::with_capacity(services.len());
    for svc in services.iter() {
        lower_to_canonical.insert(svc.name.to_ascii_lowercase(), svc.name.clone());
    }

    let mut rev_map: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut deps_map: HashMap<String, Vec<String>> = HashMap::with_capacity(services.len());

    for svc in services.iter_mut() {
        let rule = classify_service_base(&svc.name);
        svc.risk_level = rule.risk_level;
        svc.category = rule.category;
        svc.is_locked_critical = rule.is_locked_critical;
        svc.protection_reason = rule.protection_reason.map(str::to_string);

        let self_low = svc.name.to_ascii_lowercase();
        let mut norm_deps: Vec<String> = Vec::new();
        let mut seen_deps: HashSet<String> = HashSet::new();
        for dep in &svc.depends_on {
            let trimmed = dep.trim();
            if trimmed.is_empty() {
                continue;
            }
            let dep_low = trimmed.to_ascii_lowercase();
            if dep_low == self_low || !seen_deps.insert(dep_low.clone()) {
                continue;
            }
            let canonical_dep = lower_to_canonical
                .get(&dep_low)
                .cloned()
                .unwrap_or_else(|| trimmed.to_string());
            norm_deps.push(canonical_dep);
            if lower_to_canonical.contains_key(&dep_low) {
                rev_map.entry(dep_low).or_default().insert(svc.name.clone());
            }
        }
        svc.depends_on = norm_deps.clone();
        deps_map.insert(self_low, norm_deps);
    }

    // BFS from every Critical service down `depends_on` edges
    let mut required_by_critical: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for svc in services.iter() {
        if svc.is_locked_critical {
            queue.push_back(svc.name.to_ascii_lowercase());
        }
    }
    while let Some(cur) = queue.pop_front() {
        if let Some(deps) = deps_map.get(&cur) {
            for dep in deps {
                let dep_low = dep.to_ascii_lowercase();
                if lower_to_canonical.contains_key(&dep_low)
                    && required_by_critical.insert(dep_low.clone())
                {
                    queue.push_back(dep_low);
                }
            }
        }
    }

    for svc in services.iter_mut() {
        let low = svc.name.to_ascii_lowercase();
        let mut req_list: Vec<String> = rev_map
            .get(&low)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        req_list.sort_by_key(|a| a.to_ascii_lowercase());
        svc.required_by = req_list;

        if !svc.is_locked_critical && required_by_critical.contains(&low) {
            if svc.risk_level < ServiceRiskLevel::High {
                svc.risk_level = ServiceRiskLevel::High;
            }
            if svc.protection_reason.is_none() {
                svc.protection_reason = Some("dependency_of_critical".to_string());
            }
        }
    }

    services.sort_by(|a, b| {
        a.display_name
            .to_ascii_lowercase()
            .cmp(&b.display_name.to_ascii_lowercase())
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndirectStringKind {
    Plain(String),
    Resource { file_path: String, resource_id: u32 },
}

/// Parses a Windows registry indirect resource string such as:
/// - `@%SystemRoot%\system32\spoolsv.exe,-1` -> `Resource`
/// - `@acpi.inf,%ACPI.SvcDesc%;Microsoft ACPI Driver` -> `Plain("Microsoft ACPI Driver")`
/// - `Plain text` -> `Plain("Plain text")`
pub fn parse_indirect_string(raw: &str) -> IndirectStringKind {
    let trimmed = raw.trim();
    let Some(without_at) = trimmed.strip_prefix('@') else {
        return IndirectStringKind::Plain(trimmed.to_string());
    };

    if let Some((_, fallback)) = without_at.split_once(';') {
        let clean = fallback.trim();
        if !clean.is_empty() {
            return IndirectStringKind::Plain(clean.to_string());
        }
    }

    if let Some((path_part, id_part)) = without_at.rsplit_once(',') {
        let id_clean = id_part.split(';').next().unwrap_or("").trim();
        if let Ok(signed_id) = id_clean.parse::<i64>() {
            let abs_id = signed_id.unsigned_abs();
            if abs_id > 0 && abs_id <= u32::MAX as u64 {
                let clean_path = path_part.trim().trim_matches('"').to_string();
                if !clean_path.is_empty() {
                    return IndirectStringKind::Resource {
                        file_path: clean_path,
                        resource_id: abs_id as u32,
                    };
                }
            }
        }
    }

    IndirectStringKind::Plain(String::new())
}

#[cfg(windows)]
mod mui_win32 {
    use super::{IndirectStringKind, parse_indirect_string};
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::fs;
    use std::path::{Component, Path, PathBuf};

    const LOAD_LIBRARY_AS_DATAFILE: u32 = 0x0000_0002;
    const LOAD_LIBRARY_AS_IMAGE_RESOURCE: u32 = 0x0000_0020;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(
            lpLibFileName: *const u16,
            hFile: *mut c_void,
            dwFlags: u32,
        ) -> *mut c_void;
        fn FreeLibrary(hLibModule: *mut c_void) -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn LoadStringW(
            hInstance: *mut c_void,
            uID: u32,
            lpBuffer: *mut u16,
            cchBufferMax: i32,
        ) -> i32;
    }

    struct OwnedHModule(*mut c_void);

    impl Drop for OwnedHModule {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = FreeLibrary(self.0);
                }
            }
        }
    }

    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn is_lang_tag_dir(name: &str) -> bool {
        let Some((lang, rest)) = name.split_once('-') else {
            return false;
        };
        (2..=3).contains(&lang.len())
            && lang.bytes().all(|b| b.is_ascii_alphabetic())
            && !rest.is_empty()
            && rest.len() <= 10
            && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }

    fn discover_preferred_langs(mount_dir: &Path, preferred_lang: Option<&str>) -> Vec<String> {
        let mut available: Vec<String> = Vec::new();
        let sys32 = mount_dir.join("Windows").join("System32");
        if let Ok(entries) = fs::read_dir(&sys32) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type()
                    && ft.is_dir()
                {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if is_lang_tag_dir(&name) {
                        available.push(name);
                    }
                }
            }
        }
        available.sort_by_key(|a| a.to_ascii_lowercase());

        let mut ordered: Vec<String> = Vec::new();

        if let Some(pref) = preferred_lang.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(found) = available.iter().find(|l| l.eq_ignore_ascii_case(pref)) {
                ordered.push(found.clone());
            }
            if let Some((short, _)) = pref.split_once('-') {
                for l in &available {
                    if l.to_ascii_lowercase()
                        .starts_with(&format!("{}-", short.to_ascii_lowercase()))
                        && !ordered.iter().any(|x| x.eq_ignore_ascii_case(l))
                    {
                        ordered.push(l.clone());
                    }
                }
            }
        }
        if let Some(found) = available.iter().find(|l| l.eq_ignore_ascii_case("en-US"))
            && !ordered.iter().any(|x| x.eq_ignore_ascii_case(found))
        {
            ordered.push(found.clone());
        }
        for l in available {
            if !ordered.iter().any(|x| x.eq_ignore_ascii_case(&l)) {
                ordered.push(l);
            }
        }
        ordered
    }

    fn resolve_image_relative_path(mount_dir: &Path, raw_win_path: &str) -> Option<PathBuf> {
        let trimmed = raw_win_path.trim().replace('/', "\\");
        if trimmed.is_empty() || trimmed.starts_with("\\\\") {
            return None;
        }

        let lower = trimmed.to_ascii_lowercase();
        let rel = if lower.starts_with("%systemroot%\\") {
            format!("Windows\\{}", &trimmed["%systemroot%\\".len()..])
        } else if lower == "%systemroot%" {
            "Windows".to_string()
        } else if lower.starts_with("%windir%\\") {
            format!("Windows\\{}", &trimmed["%windir%\\".len()..])
        } else if lower.starts_with("%systemdrive%\\") {
            trimmed["%systemdrive%\\".len()..].to_string()
        } else if lower.starts_with("c:\\") {
            trimmed[3..].to_string()
        } else if trimmed.starts_with('\\') {
            trimmed.trim_start_matches('\\').to_string()
        } else if !trimmed.contains('\\') {
            format!("Windows\\System32\\{trimmed}")
        } else {
            trimmed
        };

        // Ignore unresolved environment variables like %ProgramFiles% safely
        if rel.contains('%') || rel.contains(':') {
            return None;
        }

        let mut stack: Vec<String> = Vec::new();
        for comp in Path::new(&rel).components() {
            match comp {
                Component::Normal(seg) => stack.push(seg.to_string_lossy().to_string()),
                Component::CurDir => {}
                Component::ParentDir => {
                    stack.pop()?;
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        if stack.is_empty() {
            return None;
        }
        let mut out = mount_dir.to_path_buf();
        for seg in stack {
            out.push(seg);
        }
        Some(out)
    }

    pub struct MuiResolver {
        mount_dir: PathBuf,
        preferred_langs: Vec<String>,
        modules: HashMap<PathBuf, Option<OwnedHModule>>,
    }

    impl MuiResolver {
        pub fn new(mount_dir: &Path, preferred_lang: Option<&str>) -> Self {
            let preferred_langs = discover_preferred_langs(mount_dir, preferred_lang);
            Self {
                mount_dir: mount_dir.to_path_buf(),
                preferred_langs,
                modules: HashMap::new(),
            }
        }

        fn load_string_from_file(&mut self, file_path: &Path, resource_id: u32) -> Option<String> {
            if !self.modules.contains_key(file_path) {
                let loaded = if file_path.is_file() {
                    let wide = to_wide_null(&file_path.to_string_lossy());
                    let handle = unsafe {
                        LoadLibraryExW(
                            wide.as_ptr(),
                            std::ptr::null_mut(),
                            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
                        )
                    };
                    if handle.is_null() {
                        None
                    } else {
                        Some(OwnedHModule(handle))
                    }
                } else {
                    None
                };
                self.modules.insert(file_path.to_path_buf(), loaded);
            }

            let handle = self.modules.get(file_path)?.as_ref()?.0;
            let mut buf = vec![0u16; 2048];
            let len =
                unsafe { LoadStringW(handle, resource_id, buf.as_mut_ptr(), buf.len() as i32) };
            if len <= 0 {
                return None;
            }
            if len as usize + 1 >= buf.len() {
                buf.resize(8192, 0);
                let len2 =
                    unsafe { LoadStringW(handle, resource_id, buf.as_mut_ptr(), buf.len() as i32) };
                if len2 <= 0 {
                    return None;
                }
                return Some(String::from_utf16_lossy(&buf[..len2 as usize]));
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }

        pub fn resolve(&mut self, raw: &str) -> Option<String> {
            match parse_indirect_string(raw) {
                IndirectStringKind::Plain(s) => {
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                }
                IndirectStringKind::Resource {
                    file_path,
                    resource_id,
                } => {
                    let target = resolve_image_relative_path(&self.mount_dir, &file_path)?;
                    if let (Some(parent), Some(file_name)) =
                        (target.parent(), target.file_name().and_then(|f| f.to_str()))
                    {
                        let mui_name = format!("{file_name}.mui");
                        let langs = self.preferred_langs.clone();
                        for lang in langs {
                            let mui_path = parent.join(&lang).join(&mui_name);
                            if let Some(text) = self.load_string_from_file(&mui_path, resource_id)
                                && !text.trim().is_empty()
                            {
                                return Some(text.trim().to_string());
                            }
                        }
                    }
                    self.load_string_from_file(&target, resource_id)
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod mui_win32 {
    use super::{IndirectStringKind, parse_indirect_string};
    use std::path::Path;

    pub struct MuiResolver;

    impl MuiResolver {
        pub fn new(_mount_dir: &Path, _preferred_lang: Option<&str>) -> Self {
            Self
        }

        pub fn resolve(&mut self, raw: &str) -> Option<String> {
            match parse_indirect_string(raw) {
                IndirectStringKind::Plain(s) if !s.is_empty() => Some(s),
                _ => None,
            }
        }
    }
}

/// Converts raw offline registry service records into classified, MUI-resolved `WindowsServiceInfo` items.
pub fn build_services_from_raw(
    mount_dir: &Path,
    raw_services: Vec<RawOfflineService>,
    preferred_lang: Option<&str>,
) -> Vec<WindowsServiceInfo> {
    let mut resolver = mui_win32::MuiResolver::new(mount_dir, preferred_lang);
    let mut out = Vec::with_capacity(raw_services.len());

    for raw in raw_services {
        if !is_manageable_win32_service_type(raw.service_type) {
            continue;
        }
        let Some(start_type) =
            ServiceStartType::from_registry_values(raw.start, raw.delayed_autostart)
        else {
            continue;
        };

        let display_name = raw
            .display_name
            .as_deref()
            .and_then(|s| resolver.resolve(s))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| raw.name.clone());

        let description = raw
            .description
            .as_deref()
            .and_then(|s| resolver.resolve(s))
            .unwrap_or_default();

        let rule = classify_service_base(&raw.name);

        out.push(WindowsServiceInfo {
            name: raw.name,
            display_name,
            description,
            start_type,
            service_type: raw.service_type,
            is_per_user_template: (raw.service_type & 0x40) != 0,
            has_trigger_start: raw.has_trigger_start,
            group: raw.group.unwrap_or_default(),
            depends_on: raw.depends_on,
            required_by: Vec::new(),
            risk_level: rule.risk_level,
            category: rule.category,
            is_locked_critical: rule.is_locked_critical,
            protection_reason: rule.protection_reason.map(str::to_string),
        });
    }

    drop(resolver);
    reclassify_services(&mut out);
    out
}

/// Validates that a Windows service key name is a safe single registry subkey segment.
pub fn is_valid_service_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed == "." || trimmed == ".." {
        return false;
    }
    if name != trimmed {
        return false;
    }
    !trimmed
        .chars()
        .any(|c| c == '\\' || c == '/' || c == '\0' || c.is_control())
}

pub fn validate_services_plan(plan: &ServicesBuildPlan) -> Result<(), String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for item in &plan.changes {
        if !is_valid_service_name(&item.name) {
            return Err(format!("Invalid service name '{}'.", item.name));
        }
        let low = item.name.to_ascii_lowercase();
        if seen.contains_key(&low) {
            return Err(format!(
                "Duplicate service entry '{}' in services plan.",
                item.name
            ));
        }
        seen.insert(low, item.name.clone());

        if item.start_type == ServiceStartType::Disabled {
            let rule = classify_service_base(&item.name);
            if rule.is_locked_critical {
                return Err(format!(
                    "Critical system service '{}' cannot be disabled.",
                    item.name
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn filters_win32_services_and_excludes_drivers_and_instances() {
        assert!(is_manageable_win32_service_type(0x10)); // SERVICE_WIN32_OWN_PROCESS
        assert!(is_manageable_win32_service_type(0x20)); // SERVICE_WIN32_SHARE_PROCESS
        assert!(is_manageable_win32_service_type(0x50)); // UserOwnProcess template
        assert!(is_manageable_win32_service_type(0x60)); // UserShareProcess template
        assert!(is_manageable_win32_service_type(0x110)); // Interactive own process

        assert!(!is_manageable_win32_service_type(0x01)); // Kernel driver
        assert!(!is_manageable_win32_service_type(0x02)); // FS driver
        assert!(!is_manageable_win32_service_type(0x08)); // Recognizer driver
        assert!(!is_manageable_win32_service_type(0xD0)); // Per-user runtime instance (0x50 | 0x80)
        assert!(!is_manageable_win32_service_type(0xE0)); // Per-user runtime instance (0x60 | 0x80)
        assert!(!is_manageable_win32_service_type(0x00));
    }

    #[test]
    fn maps_start_and_delayed_autostart_roundtrip() {
        assert_eq!(
            ServiceStartType::from_registry_values(2, None),
            Some(ServiceStartType::Automatic)
        );
        assert_eq!(
            ServiceStartType::from_registry_values(2, Some(0)),
            Some(ServiceStartType::Automatic)
        );
        assert_eq!(
            ServiceStartType::from_registry_values(2, Some(1)),
            Some(ServiceStartType::DelayedAuto)
        );
        // Stale DelayedAutostart=1 on Manual/Disabled must still report Manual/Disabled
        assert_eq!(
            ServiceStartType::from_registry_values(3, Some(1)),
            Some(ServiceStartType::Manual)
        );
        assert_eq!(
            ServiceStartType::from_registry_values(4, Some(1)),
            Some(ServiceStartType::Disabled)
        );
        assert_eq!(ServiceStartType::from_registry_values(0, None), None);
        assert_eq!(ServiceStartType::from_registry_values(1, None), None);

        assert_eq!(ServiceStartType::Automatic.to_registry_values(), (2, false));
        assert_eq!(
            ServiceStartType::DelayedAuto.to_registry_values(),
            (2, true)
        );
        assert_eq!(ServiceStartType::Manual.to_registry_values(), (3, false));
        assert_eq!(ServiceStartType::Disabled.to_registry_values(), (4, false));
    }

    #[test]
    fn parses_indirect_and_inf_fallback_strings() {
        assert_eq!(
            parse_indirect_string("Plain Display Name"),
            IndirectStringKind::Plain("Plain Display Name".to_string())
        );
        assert_eq!(
            parse_indirect_string("@1394.inf,%PCI\\CC_0C0010.DeviceDesc%;1394 OHCI Controller"),
            IndirectStringKind::Plain("1394 OHCI Controller".to_string())
        );
        assert_eq!(
            parse_indirect_string(r"@%SystemRoot%\system32\AarSvc.dll,-100"),
            IndirectStringKind::Resource {
                file_path: r"%SystemRoot%\system32\AarSvc.dll".to_string(),
                resource_id: 100
            }
        );
        assert_eq!(
            parse_indirect_string(r#"@"%SystemRoot%\system32\spoolsv.exe",-2"#),
            IndirectStringKind::Resource {
                file_path: r"%SystemRoot%\system32\spoolsv.exe".to_string(),
                resource_id: 2
            }
        );
    }

    #[test]
    fn builds_reverse_dependencies_and_upgrades_critical_dependency_risk() {
        let raw = vec![
            RawOfflineService {
                name: "RpcSs".to_string(),
                service_type: 0x20,
                start: 2,
                delayed_autostart: None,
                display_name: Some("Remote Procedure Call".to_string()),
                description: None,
                group: None,
                depends_on: vec!["DcomLaunch".to_string(), "CustomHelperSvc".to_string()],
                has_trigger_start: false,
            },
            RawOfflineService {
                name: "DcomLaunch".to_string(),
                service_type: 0x20,
                start: 2,
                delayed_autostart: None,
                display_name: Some("DCOM Server Process Launcher".to_string()),
                description: None,
                group: None,
                depends_on: vec![],
                has_trigger_start: false,
            },
            RawOfflineService {
                name: "CustomHelperSvc".to_string(),
                service_type: 0x10,
                start: 3,
                delayed_autostart: None,
                display_name: Some("Custom Helper".to_string()),
                description: None,
                group: None,
                depends_on: vec![],
                has_trigger_start: true,
            },
            RawOfflineService {
                name: "DiagTrack".to_string(),
                service_type: 0x10,
                start: 2,
                delayed_autostart: None,
                display_name: Some("@%SystemRoot%\\system32\\diagtrack.dll,-3001".to_string()),
                description: None,
                group: None,
                depends_on: vec![],
                has_trigger_start: false,
            },
            RawOfflineService {
                name: "disk".to_string(),
                service_type: 0x01, // driver, must be filtered out
                start: 0,
                delayed_autostart: None,
                display_name: Some("Disk Driver".to_string()),
                description: None,
                group: None,
                depends_on: vec![],
                has_trigger_start: false,
            },
        ];

        let services = build_services_from_raw(&PathBuf::from(r"C:\nonexistent"), raw, None);
        assert_eq!(services.len(), 4);

        let helper = services
            .iter()
            .find(|s| s.name == "CustomHelperSvc")
            .unwrap();
        assert_eq!(helper.required_by, vec!["RpcSs".to_string()]);
        assert_eq!(helper.risk_level, ServiceRiskLevel::High);
        assert_eq!(
            helper.protection_reason.as_deref(),
            Some("dependency_of_critical")
        );
        assert!(helper.has_trigger_start);

        // Unresolved indirect DisplayName falls back cleanly to service key name
        let diag = services.iter().find(|s| s.name == "DiagTrack").unwrap();
        assert_eq!(diag.display_name, "DiagTrack");
        assert_eq!(diag.risk_level, ServiceRiskLevel::Safe);
    }

    #[test]
    fn validates_services_plan_strictly() {
        let valid = ServicesBuildPlan {
            changes: vec![
                ServiceStartChange {
                    name: "DiagTrack".to_string(),
                    start_type: ServiceStartType::Disabled,
                },
                ServiceStartChange {
                    name: "RpcSs".to_string(),
                    start_type: ServiceStartType::Automatic,
                },
            ],
        };
        assert!(validate_services_plan(&valid).is_ok());

        let locked_err = validate_services_plan(&ServicesBuildPlan {
            changes: vec![ServiceStartChange {
                name: "rpcss".to_string(),
                start_type: ServiceStartType::Disabled,
            }],
        })
        .unwrap_err();
        assert!(locked_err.contains("cannot be disabled"));

        let dup_err = validate_services_plan(&ServicesBuildPlan {
            changes: vec![
                ServiceStartChange {
                    name: "DiagTrack".to_string(),
                    start_type: ServiceStartType::Disabled,
                },
                ServiceStartChange {
                    name: "diagtrack".to_string(),
                    start_type: ServiceStartType::Manual,
                },
            ],
        })
        .unwrap_err();
        assert!(dup_err.contains("Duplicate service entry"));

        for bad in ["", "..", "a\\b", "a/b", " spaced "] {
            let err = validate_services_plan(&ServicesBuildPlan {
                changes: vec![ServiceStartChange {
                    name: bad.to_string(),
                    start_type: ServiceStartType::Manual,
                }],
            })
            .unwrap_err();
            assert!(err.contains("Invalid service name"));
        }
    }
}
