use crate::apps::StubOption;
use crate::files::FilesBuildPlan;
use crate::registry::RegistryBuildPlan;
use crate::updates::UpdatesBuildPlan;
use crate::wim::InstallOutputFormat;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub const PRESET_SCHEMA_VERSION: &str = "1.0";

fn default_true() -> bool {
    true
}

fn default_target_arch() -> String {
    "x64".to_string()
}

fn default_target() -> String {
    "iso".to_string()
}

fn default_install_format() -> InstallOutputFormat {
    InstallOutputFormat::Wim
}

fn default_split_size() -> u32 {
    3800
}

fn default_volume_label() -> String {
    "WIN11_CUSTOM".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetMeta {
    pub name: String,
    pub description: String,
    pub created_at: String,
    #[serde(default = "default_target_arch")]
    pub target_arch: String,
    pub target_edition_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PresetRemoveAppx {
    Detailed {
        #[serde(rename = "packageFamilyName")]
        package_family_name: String,
        #[serde(rename = "displayNameHint", default)]
        display_name_hint: Option<String>,
    },
    FamilyOnly(String),
}

impl PresetRemoveAppx {
    pub fn package_family_name(&self) -> &str {
        match self {
            PresetRemoveAppx::Detailed {
                package_family_name,
                ..
            } => package_family_name.as_str(),
            PresetRemoveAppx::FamilyOnly(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetAddAppx {
    pub package_path: String,
    pub dependency_paths: Vec<String>,
    pub license_path: Option<String>,
    #[serde(default = "default_true")]
    pub skip_license: bool,
    #[serde(default = "default_true")]
    pub region_all: bool,
    pub stub_option: StubOption,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetApps {
    pub remove_packages: Vec<PresetRemoveAppx>,
    pub add_packages: Vec<PresetAddAppx>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetFeatureEnable {
    pub name: String,
    #[serde(default = "default_true")]
    pub enable_all: bool,
    pub custom_sxs_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetCapabilityAdd {
    pub name: String,
    #[serde(default = "default_true")]
    pub limit_access: bool,
    pub custom_sxs_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetFeatures {
    pub enable: Vec<PresetFeatureEnable>,
    pub disable: Vec<String>,
    pub capabilities_add: Vec<PresetCapabilityAdd>,
    pub capabilities_remove: Vec<String>,
    pub default_sxs_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetDriverItem {
    pub path: String,
    pub is_directory: Option<bool>,
    #[serde(default = "default_true")]
    pub recurse: bool,
    pub force_unsigned: bool,
    #[serde(default = "default_true")]
    pub inject_to_install_wim: bool,
    pub inject_to_boot_wim: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetDrivers {
    pub items: Vec<PresetDriverItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PresetOutput {
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(default = "default_install_format")]
    pub install_format: InstallOutputFormat,
    #[serde(default = "default_split_size")]
    pub split_size_mb: u32,
    pub autounattend_path: Option<String>,
    pub add_generic_ei_cfg: bool,
    #[serde(default = "default_true")]
    pub uefi_no_prompt: bool,
    #[serde(default = "default_volume_label")]
    pub volume_label: String,
}

impl Default for PresetOutput {
    fn default() -> Self {
        Self {
            target: default_target(),
            install_format: default_install_format(),
            split_size_mb: default_split_size(),
            autounattend_path: None,
            add_generic_ei_cfg: false,
            uefi_no_prompt: true,
            volume_label: default_volume_label(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StudioPreset {
    pub schema_version: String,
    pub meta: PresetMeta,
    pub apps: PresetApps,
    pub features: PresetFeatures,
    pub updates: UpdatesBuildPlan,
    pub drivers: PresetDrivers,
    pub registry: RegistryBuildPlan,
    pub files: FilesBuildPlan,
    pub output: PresetOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MissingPathKind {
    AppxPackage,
    AppxDependency,
    AppxLicense,
    FeaturesSxs,
    UpdateItem,
    DriverItem,
    RegFile,
    CustomFile,
    AutounattendXml,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingPresetPath {
    pub id: String,
    pub kind: MissingPathKind,
    pub expected_directory: bool,
    pub path: String,
    pub parent_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetImportResult {
    pub preset: StudioPreset,
    pub missing_paths: Vec<MissingPresetPath>,
}

fn path_exists_matching(raw_path: &str, expect_dir: bool) -> bool {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let p = Path::new(trimmed);
    if expect_dir {
        p.is_dir()
    } else {
        p.is_file()
    }
}

pub fn find_missing_preset_paths(preset: &StudioPreset) -> Vec<MissingPresetPath> {
    let mut missing = Vec::new();
    let mut next_id = 0usize;

    let mut push_missing =
        |kind: MissingPathKind, expected_directory: bool, path: &str, parent_path: Option<&str>| {
            missing.push(MissingPresetPath {
                id: format!("missing-{next_id}"),
                kind,
                expected_directory,
                path: path.to_string(),
                parent_path: parent_path.map(str::to_string),
            });
            next_id += 1;
        };

    for pkg in &preset.apps.add_packages {
        if !path_exists_matching(&pkg.package_path, false) {
            push_missing(MissingPathKind::AppxPackage, false, &pkg.package_path, None);
        }
        for dep in &pkg.dependency_paths {
            if !path_exists_matching(dep, false) {
                push_missing(
                    MissingPathKind::AppxDependency,
                    false,
                    dep,
                    Some(&pkg.package_path),
                );
            }
        }
        if let Some(lic) = pkg
            .license_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !path_exists_matching(lic, false) {
                push_missing(
                    MissingPathKind::AppxLicense,
                    false,
                    lic,
                    Some(&pkg.package_path),
                );
            }
        }
    }

    let mut checked_sxs: HashSet<String> = HashSet::new();
    let mut check_sxs = |raw: Option<&str>| {
        let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return;
        };
        let key = trimmed.to_ascii_lowercase();
        if !checked_sxs.insert(key) {
            return;
        }
        if !path_exists_matching(trimmed, true) {
            push_missing(MissingPathKind::FeaturesSxs, true, trimmed, None);
        }
    };

    check_sxs(preset.features.default_sxs_path.as_deref());
    for feat in &preset.features.enable {
        check_sxs(feat.custom_sxs_path.as_deref());
    }
    for cap in &preset.features.capabilities_add {
        check_sxs(cap.custom_sxs_path.as_deref());
    }

    for upd in &preset.updates.items {
        if !path_exists_matching(&upd.path, upd.is_directory) {
            push_missing(
                MissingPathKind::UpdateItem,
                upd.is_directory,
                &upd.path,
                None,
            );
        }
    }

    for drv in &preset.drivers.items {
        let expect_dir = drv
            .is_directory
            .unwrap_or_else(|| !drv.path.trim().to_ascii_lowercase().ends_with(".inf"));
        if !path_exists_matching(&drv.path, expect_dir) {
            push_missing(MissingPathKind::DriverItem, expect_dir, &drv.path, None);
        }
    }

    for reg in &preset.registry.reg_files {
        if !path_exists_matching(&reg.path, false) {
            push_missing(MissingPathKind::RegFile, false, &reg.path, None);
        }
    }

    for file_item in &preset.files.items {
        if !path_exists_matching(&file_item.source_path, file_item.is_directory) {
            push_missing(
                MissingPathKind::CustomFile,
                file_item.is_directory,
                &file_item.source_path,
                None,
            );
        }
    }

    if let Some(xml) = preset
        .output
        .autounattend_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !path_exists_matching(xml, false) {
            push_missing(MissingPathKind::AutounattendXml, false, xml, None);
        }
    }

    missing
}

pub fn export_preset_to_file(dest_path: &Path, mut preset: StudioPreset) -> Result<String, String> {
    preset.schema_version = PRESET_SCHEMA_VERSION.to_string();
    if preset.meta.created_at.trim().is_empty() {
        preset.meta.created_at = Utc::now().to_rfc3339();
    }
    if preset.meta.target_arch.trim().is_empty() {
        preset.meta.target_arch = default_target_arch();
    }

    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                format!("Failed creating preset folder '{}': {e}", parent.display())
            })?;
        }
    }

    let json = serde_json::to_string_pretty(&preset)
        .map_err(|e| format!("Failed serializing JSON preset: {e}"))?;
    fs::write(dest_path, json)
        .map_err(|e| format!("Failed writing preset '{}': {e}", dest_path.display()))?;

    Ok(dest_path.to_string_lossy().to_string())
}

pub fn import_preset_from_file(src_path: &Path) -> Result<PresetImportResult, String> {
    let raw = fs::read_to_string(src_path)
        .map_err(|e| format!("Failed reading preset '{}': {e}", src_path.display()))?;
    let clean = raw.trim_start_matches('\u{feff}').trim();

    let preset: StudioPreset = serde_json::from_str(clean)
        .map_err(|e| format!("Invalid preset JSON '{}': {e}", src_path.display()))?;

    if preset.schema_version.trim() != PRESET_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported preset schemaVersion '{}'; expected '{}'.",
            preset.schema_version, PRESET_SCHEMA_VERSION
        ));
    }

    let missing_paths = find_missing_preset_paths(&preset);
    Ok(PresetImportResult {
        preset,
        missing_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{ConflictPolicy, FileInjectionItem, FileTarget};
    use crate::registry::RegQueueEntry;
    use crate::updates::UpdateQueueItem;
    use std::path::PathBuf;
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
    fn roundtrips_full_preset_v1_with_files_and_metadata() {
        let base = make_temp_dir("isocraft_preset_rt");
        let preset_file = base.join("nested").join("my-preset.json");

        let sxs_dir = base.join("sxs");
        let drv_dir = base.join("drv");
        fs::create_dir_all(&sxs_dir).unwrap();
        fs::create_dir_all(&drv_dir).unwrap();

        let appx_file = base.join("app.msix");
        let dep_file = base.join("vclibs.appx");
        let lic_file = base.join("license.xml");
        let upd_file = base.join("cu.msu");
        let reg_file = base.join("tweaks.reg");
        let custom_file = base.join("SetupComplete.cmd");
        let unattend_file = base.join("autounattend.xml");

        for f in [
            &appx_file,
            &dep_file,
            &lic_file,
            &upd_file,
            &reg_file,
            &custom_file,
            &unattend_file,
        ] {
            fs::write(f, b"stub").unwrap();
        }

        let preset = StudioPreset {
            schema_version: String::new(),
            meta: PresetMeta {
                name: "Gaming Minimal".to_string(),
                description: "Clean Win11 Pro x64".to_string(),
                created_at: String::new(),
                target_arch: String::new(),
                target_edition_hint: Some("Windows 11 Pro".to_string()),
            },
            apps: PresetApps {
                remove_packages: vec![PresetRemoveAppx::Detailed {
                    package_family_name: "Microsoft.BingNews_8wekyb3d8bbwe".to_string(),
                    display_name_hint: Some("Bing News".to_string()),
                }],
                add_packages: vec![PresetAddAppx {
                    package_path: appx_file.to_string_lossy().to_string(),
                    dependency_paths: vec![dep_file.to_string_lossy().to_string()],
                    license_path: Some(lic_file.to_string_lossy().to_string()),
                    skip_license: false,
                    region_all: true,
                    stub_option: StubOption::UsePreference,
                }],
            },
            features: PresetFeatures {
                enable: vec![PresetFeatureEnable {
                    name: "NetFx3".to_string(),
                    enable_all: true,
                    custom_sxs_path: Some(sxs_dir.to_string_lossy().to_string()),
                }],
                disable: vec!["WorkFolders-Client".to_string()],
                capabilities_add: vec![PresetCapabilityAdd {
                    name: "OpenSSH.Client~~~~0.0.1.0".to_string(),
                    limit_access: true,
                    custom_sxs_path: None,
                }],
                capabilities_remove: vec!["Hello.Face.20134~~~~0.0.1.0".to_string()],
                default_sxs_path: Some(sxs_dir.to_string_lossy().to_string()),
            },
            updates: UpdatesBuildPlan {
                items: vec![UpdateQueueItem {
                    path: upd_file.to_string_lossy().to_string(),
                    is_directory: false,
                }],
                run_component_cleanup: true,
                cleanup_reset_base: false,
            },
            drivers: PresetDrivers {
                items: vec![PresetDriverItem {
                    path: drv_dir.to_string_lossy().to_string(),
                    is_directory: Some(true),
                    recurse: true,
                    force_unsigned: false,
                    inject_to_install_wim: true,
                    inject_to_boot_wim: true,
                }],
            },
            registry: RegistryBuildPlan {
                reg_files: vec![RegQueueEntry {
                    path: reg_file.to_string_lossy().to_string(),
                    enabled: true,
                }],
            },
            files: FilesBuildPlan {
                items: vec![FileInjectionItem {
                    source_path: custom_file.to_string_lossy().to_string(),
                    is_directory: false,
                    target: FileTarget::InstallWim,
                    destination_path: r"Windows\Setup\Scripts".to_string(),
                    conflict_policy: ConflictPolicy::Overwrite,
                }],
            },
            output: PresetOutput {
                target: "iso".to_string(),
                install_format: InstallOutputFormat::Esd,
                split_size_mb: 3800,
                autounattend_path: Some(unattend_file.to_string_lossy().to_string()),
                add_generic_ei_cfg: true,
                uefi_no_prompt: true,
                volume_label: "WIN11_PRO_X64".to_string(),
            },
        };

        export_preset_to_file(&preset_file, preset).unwrap();
        let imported = import_preset_from_file(&preset_file).unwrap();

        assert_eq!(imported.preset.schema_version, "1.0");
        assert!(!imported.preset.meta.created_at.is_empty());
        assert_eq!(imported.preset.meta.target_arch, "x64");
        assert_eq!(imported.preset.apps.remove_packages.len(), 1);
        assert_eq!(
            imported.preset.apps.remove_packages[0].package_family_name(),
            "Microsoft.BingNews_8wekyb3d8bbwe"
        );
        assert_eq!(
            imported.preset.output.install_format,
            InstallOutputFormat::Esd
        );
        assert_eq!(imported.preset.files.items.len(), 1);
        assert!(imported.missing_paths.is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn imports_legacy_string_remove_packages_and_minimal_driver_items() {
        let base = make_temp_dir("isocraft_preset_compat");
        let preset_file = base.join("compat.json");

        // UTF-8 BOM + string-form removePackages + minimal driver item without isDirectory + omitted files
        let raw_json = "\u{feff}{\
          \"schemaVersion\": \"1.0\",\
          \"meta\": { \"name\": \"Legacy Spec\" },\
          \"apps\": {\
            \"removePackages\": [\
              \"Microsoft.XboxGamingOverlay_8wekyb3d8bbwe\",\
              { \"packageFamilyName\": \"Microsoft.Todos_8wekyb3d8bbwe\", \"displayNameHint\": \"To Do\" }\
            ]\
          },\
          \"drivers\": {\
            \"items\": [\
              { \"path\": \"C:\\\\missing_driver\\\\nvme.inf\", \"recurse\": false, \"forceUnsigned\": true, \"injectToInstallWim\": true, \"injectToBootWim\": true }\
            ]\
          },\
          \"output\": {\
            \"target\": \"image\",\
            \"installFormat\": \"swm\",\
            \"splitSizeMb\": 3500\
          }\
        }";
        fs::write(&preset_file, raw_json).unwrap();

        let imported = import_preset_from_file(&preset_file).unwrap();
        assert_eq!(imported.preset.apps.remove_packages.len(), 2);
        assert_eq!(
            imported.preset.apps.remove_packages[0].package_family_name(),
            "Microsoft.XboxGamingOverlay_8wekyb3d8bbwe"
        );
        assert_eq!(
            imported.preset.apps.remove_packages[1].package_family_name(),
            "Microsoft.Todos_8wekyb3d8bbwe"
        );
        assert_eq!(
            imported.preset.output.install_format,
            InstallOutputFormat::Swm
        );
        assert_eq!(imported.preset.output.split_size_mb, 3500);
        assert!(imported.preset.files.items.is_empty());

        // Single .inf omitted is_directory -> inferred as expected_directory = false
        assert_eq!(imported.missing_paths.len(), 1);
        assert_eq!(imported.missing_paths[0].kind, MissingPathKind::DriverItem);
        assert!(!imported.missing_paths[0].expected_directory);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let base = make_temp_dir("isocraft_preset_ver");
        let preset_file = base.join("v2.json");
        fs::write(&preset_file, r#"{ "schemaVersion": "2.0" }"#).unwrap();

        let err = import_preset_from_file(&preset_file).unwrap_err();
        assert!(err.contains("schemaVersion"));
        assert!(err.contains("2.0"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn detects_all_missing_local_paths_accurately() {
        let base = make_temp_dir("isocraft_preset_missing");
        let existing_sxs = base.join("existing_sxs");
        fs::create_dir_all(&existing_sxs).unwrap();
        let existing_reg = base.join("ok.reg");
        fs::write(&existing_reg, b"Windows Registry Editor Version 5.00\r\n").unwrap();

        let missing_sxs = base.join("missing_sxs").to_string_lossy().to_string();
        let preset = StudioPreset {
            schema_version: "1.0".to_string(),
            apps: PresetApps {
                add_packages: vec![PresetAddAppx {
                    package_path: base.join("missing.msix").to_string_lossy().to_string(),
                    dependency_paths: vec![base
                        .join("missing_dep.appx")
                        .to_string_lossy()
                        .to_string()],
                    license_path: Some(base.join("missing_lic.xml").to_string_lossy().to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            features: PresetFeatures {
                default_sxs_path: Some(missing_sxs.clone()),
                enable: vec![
                    PresetFeatureEnable {
                        name: "NetFx3".to_string(),
                        enable_all: true,
                        custom_sxs_path: Some(missing_sxs), // duplicate should be reported once
                    },
                    PresetFeatureEnable {
                        name: "TelnetClient".to_string(),
                        enable_all: true,
                        custom_sxs_path: Some(existing_sxs.to_string_lossy().to_string()),
                    },
                ],
                ..Default::default()
            },
            updates: UpdatesBuildPlan {
                items: vec![UpdateQueueItem {
                    path: base.join("missing_upd_dir").to_string_lossy().to_string(),
                    is_directory: true,
                }],
                ..Default::default()
            },
            drivers: PresetDrivers {
                items: vec![PresetDriverItem {
                    path: base.join("missing_drv_dir").to_string_lossy().to_string(),
                    is_directory: Some(true),
                    ..Default::default()
                }],
            },
            registry: RegistryBuildPlan {
                reg_files: vec![
                    RegQueueEntry {
                        path: existing_reg.to_string_lossy().to_string(),
                        enabled: true,
                    },
                    RegQueueEntry {
                        path: base.join("missing.reg").to_string_lossy().to_string(),
                        enabled: true,
                    },
                ],
            },
            files: FilesBuildPlan {
                items: vec![FileInjectionItem {
                    source_path: base
                        .join("missing_script.cmd")
                        .to_string_lossy()
                        .to_string(),
                    is_directory: false,
                    target: FileTarget::InstallWim,
                    destination_path: r"Windows\Setup\Scripts".to_string(),
                    conflict_policy: ConflictPolicy::Overwrite,
                }],
            },
            output: PresetOutput {
                autounattend_path: Some(
                    base.join("missing_unattend.xml")
                        .to_string_lossy()
                        .to_string(),
                ),
                ..Default::default()
            },
            ..Default::default()
        };

        let missing = find_missing_preset_paths(&preset);
        assert_eq!(missing.len(), 9);
        assert_eq!(missing[0].kind, MissingPathKind::AppxPackage);
        assert_eq!(missing[1].kind, MissingPathKind::AppxDependency);
        assert_eq!(missing[2].kind, MissingPathKind::AppxLicense);
        assert_eq!(missing[3].kind, MissingPathKind::FeaturesSxs);
        assert!(missing[3].expected_directory);
        assert_eq!(missing[4].kind, MissingPathKind::UpdateItem);
        assert!(missing[4].expected_directory);
        assert_eq!(missing[5].kind, MissingPathKind::DriverItem);
        assert!(missing[5].expected_directory);
        assert_eq!(missing[6].kind, MissingPathKind::RegFile);
        assert_eq!(missing[7].kind, MissingPathKind::CustomFile);
        assert_eq!(missing[8].kind, MissingPathKind::AutounattendXml);

        let _ = fs::remove_dir_all(&base);
    }
}
