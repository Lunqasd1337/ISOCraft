use crate::fs_utils::is_symlink_or_reparse;
use crate::process::{ProcessGuard, run_tracked_command};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionedAppxInfo {
    pub display_name: String,
    pub package_name: String,
    pub package_family_name: String,
    pub version: String,
    pub publisher_id: String,
    pub is_protected: bool,
    pub protection_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum StubOption {
    #[default]
    None,
    UseStub,
    UsePreference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddAppxRequest {
    pub package_path: String,
    pub dependency_paths: Vec<String>,
    pub license_path: Option<String>,
    pub skip_license: bool,
    pub region_all: bool,
    pub stub_option: StubOption,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedDependency {
    pub path: String,
    pub detected_arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyScanResult {
    pub compatible: Vec<String>,
    pub skipped_incompatible: Vec<SkippedDependency>,
}

pub fn extract_package_family_name(package_name: &str) -> String {
    let trimmed = package_name.trim();
    if let (Some(first_us), Some(last_us)) = (trimmed.find('_'), trimmed.rfind('_'))
        && first_us < last_us
    {
        let name = &trimmed[..first_us];
        let pub_id = &trimmed[last_us + 1..];
        if !name.is_empty() && !pub_id.is_empty() {
            return format!("{name}_{pub_id}");
        }
    }
    trimmed.to_string()
}

pub fn protected_appx_reason(package_name_or_family: &str) -> Option<&'static str> {
    let lower = package_name_or_family.to_ascii_lowercase();
    let name_part = lower.split('_').next().unwrap_or(&lower);

    if name_part == "microsoft.windowsstore" {
        return Some("store");
    }
    if name_part == "microsoft.storepurchaseapp" {
        return Some("store_purchase");
    }
    if name_part == "microsoft.desktopappinstaller" {
        return Some("app_installer");
    }
    if name_part == "microsoft.sechealthui" {
        return Some("windows_security");
    }
    if name_part == "microsoft.win32webviewhost" || name_part.contains("webview2") {
        return Some("webview");
    }
    if name_part.starts_with("microsoft.ui.xaml") {
        return Some("ui_xaml");
    }
    if name_part.starts_with("microsoft.vclibs") {
        return Some("vclibs");
    }
    if name_part.starts_with("microsoft.net.native") {
        return Some("net_native");
    }
    if name_part.starts_with("microsoft.windowsappruntime") {
        return Some("windows_app_runtime");
    }
    None
}

/// Detects Appx/MSIX dependency package architecture from standard package filenames
/// (e.g. `Microsoft.VCLibs.x64.14.00.appx`, `Microsoft.UI.Xaml.2.8_8.2310.30001.0_x64__8wekyb3d8bbwe.appx`,
/// `Foo_1.0_neutral_~_pub.appxbundle`, `Bar_x86.appx`, `Baz_arm64.msix`).
pub fn classify_appx_arch(path: &Path) -> &'static str {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Bundles usually carry multi-arch or neutral payloads unless explicitly tagged
    let tokens: Vec<&str> = name
        .split(['_', '.', '-', ' '])
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.contains(&"arm64") || tokens.contains(&"aarch64") {
        return "arm64";
    }
    if tokens.contains(&"x64") || tokens.contains(&"amd64") {
        return "x64";
    }
    if tokens.contains(&"x86") || tokens.contains(&"win32") {
        return "x86";
    }
    if tokens.contains(&"arm") {
        return "arm";
    }
    if tokens.contains(&"neutral") || ext.ends_with("bundle") {
        return "neutral";
    }
    "unknown"
}

pub fn scan_appx_dependencies(paths: &[String]) -> DependencyScanResult {
    let mut compatible = Vec::new();
    let mut skipped_incompatible = Vec::new();

    for raw in paths {
        let p = PathBuf::from(raw);
        let Ok(top_meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        if is_symlink_or_reparse(&top_meta) {
            continue;
        }
        if top_meta.is_dir() {
            let mut stack = vec![p];
            while let Some(dir) = stack.pop() {
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let child = entry.path();
                        let Ok(meta) = fs::symlink_metadata(&child) else {
                            continue;
                        };
                        if is_symlink_or_reparse(&meta) {
                            continue;
                        }
                        if meta.is_dir() {
                            stack.push(child);
                        } else if meta.is_file() && is_appx_package_file(&child) {
                            classify_and_push(&child, &mut compatible, &mut skipped_incompatible);
                        }
                    }
                }
            }
        } else if top_meta.is_file() && is_appx_package_file(&p) {
            classify_and_push(&p, &mut compatible, &mut skipped_incompatible);
        }
    }

    compatible.sort();
    compatible.dedup();
    DependencyScanResult {
        compatible,
        skipped_incompatible,
    }
}

fn is_appx_package_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("appx" | "appxbundle" | "msix" | "msixbundle")
    )
}

/// Validates that an `AddAppxRequest` references existing regular non-symlink package,
/// dependency, and license files with expected extensions before DISM runs.
pub fn validate_appx_request_paths(req: &AddAppxRequest) -> Result<(), String> {
    let pkg_raw = req.package_path.trim();
    if pkg_raw.is_empty() {
        return Err("Appx package path cannot be empty.".to_string());
    }
    let pkg_path = Path::new(pkg_raw);
    let pkg_meta = fs::symlink_metadata(pkg_path)
        .map_err(|_| format!("Appx package file does not exist: '{}'", pkg_path.display()))?;
    if is_symlink_or_reparse(&pkg_meta) || !pkg_meta.is_file() || !is_appx_package_file(pkg_path) {
        return Err(format!(
            "Invalid Appx/MSIX package file: '{}'",
            pkg_path.display()
        ));
    }

    for dep in &req.dependency_paths {
        let dep_trimmed = dep.trim();
        if dep_trimmed.is_empty() {
            continue;
        }
        let dep_path = Path::new(dep_trimmed);
        let dep_meta = fs::symlink_metadata(dep_path).map_err(|_| {
            format!(
                "Appx dependency file does not exist: '{}'",
                dep_path.display()
            )
        })?;
        if is_symlink_or_reparse(&dep_meta)
            || !dep_meta.is_file()
            || !is_appx_package_file(dep_path)
        {
            return Err(format!(
                "Invalid Appx dependency file: '{}'",
                dep_path.display()
            ));
        }
    }

    if !req.skip_license
        && let Some(lic) = req
            .license_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        let lic_path = Path::new(lic);
        let lic_meta = fs::symlink_metadata(lic_path)
            .map_err(|_| format!("Appx license file does not exist: '{}'", lic_path.display()))?;
        let is_xml = lic_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("xml"))
            .unwrap_or(false);
        if is_symlink_or_reparse(&lic_meta) || !lic_meta.is_file() || !is_xml {
            return Err(format!(
                "Invalid Appx XML license file: '{}'",
                lic_path.display()
            ));
        }
    }

    Ok(())
}

fn classify_and_push(
    path: &Path,
    compatible: &mut Vec<String>,
    skipped: &mut Vec<SkippedDependency>,
) {
    let arch = classify_appx_arch(path);
    let path_str = path.to_string_lossy().to_string();
    match arch {
        "x86" | "arm" | "arm64" => skipped.push(SkippedDependency {
            path: path_str,
            detected_arch: arch.to_string(),
        }),
        _ => compatible.push(path_str),
    }
}

pub fn build_remove_appx_args(
    mount_dir: &Path,
    log_path: &Path,
    package_name: &str,
) -> Vec<String> {
    vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Remove-ProvisionedAppxPackage".to_string(),
        format!("/PackageName:{package_name}"),
    ]
}

pub fn build_add_appx_args(mount_dir: &Path, log_path: &Path, req: &AddAppxRequest) -> Vec<String> {
    let mut args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        format!("/Image:{}", mount_dir.display()),
        "/Add-ProvisionedAppxPackage".to_string(),
        format!("/PackagePath:{}", req.package_path),
    ];

    for dep in &req.dependency_paths {
        let p = Path::new(dep);
        let arch = classify_appx_arch(p);
        if !matches!(arch, "x86" | "arm" | "arm64") {
            args.push(format!("/DependencyPackagePath:{dep}"));
        }
    }

    if req.skip_license {
        args.push("/SkipLicense".to_string());
    } else if let Some(lic) = req.license_path.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push(format!("/LicensePath:{lic}"));
    } else {
        args.push("/SkipLicense".to_string());
    }

    if req.region_all {
        args.push("/Region:all".to_string());
    }

    match req.stub_option {
        StubOption::None => {}
        StubOption::UseStub => args.push("/StubPackageOption:UseStub".to_string()),
        StubOption::UsePreference => args.push("/StubPackageOption:UsePreference".to_string()),
    }

    args
}

pub async fn remove_provisioned_appx<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    package_name: &str,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let args = build_remove_appx_args(mount_dir, log_path, package_name);
    on_log(&format!("Removing Provisioned Appx: {package_name}"));
    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed removing Appx '{package_name}' (exit {}): {} {}",
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

pub async fn add_provisioned_appx<F>(
    dism_path: &Path,
    mount_dir: &Path,
    log_path: &Path,
    req: &AddAppxRequest,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let args = build_add_appx_args(mount_dir, log_path, req);
    on_log(&format!("Adding Provisioned Appx: {}", req.package_path));
    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |l| {
        if !l.trim().is_empty() {
            on_log(l);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "Failed adding Appx '{}' (exit {}): {} {}",
            req.package_path,
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_package_family_name_accurately() {
        assert_eq!(
            extract_package_family_name("Microsoft.WindowsStore_22409.1401.6.0_x64__8wekyb3d8bbwe"),
            "Microsoft.WindowsStore_8wekyb3d8bbwe"
        );
        assert_eq!(
            extract_package_family_name("Microsoft.VCLibs.140.00_14.0.33519.0_x64__8wekyb3d8bbwe"),
            "Microsoft.VCLibs.140.00_8wekyb3d8bbwe"
        );
    }

    #[test]
    fn identifies_protected_system_appx() {
        assert_eq!(
            protected_appx_reason("Microsoft.WindowsStore_8wekyb3d8bbwe"),
            Some("store")
        );
        assert_eq!(
            protected_appx_reason("Microsoft.DesktopAppInstaller_8wekyb3d8bbwe"),
            Some("app_installer")
        );
        assert_eq!(
            protected_appx_reason("Microsoft.UI.Xaml.2.8_8.2310.30001.0_x64__8wekyb3d8bbwe"),
            Some("ui_xaml")
        );
        assert_eq!(
            protected_appx_reason(
                "Microsoft.Win32WebViewHost_10.0.26100.1_neutral_neutral_cw5n1h2txyewy"
            ),
            Some("webview")
        );
        assert_eq!(
            protected_appx_reason("Clipchamp.Clipchamp_3.0.0.0_neutral__yxz26nhyzhsrt"),
            None
        );
    }

    #[test]
    fn scans_and_filters_dependencies_by_arch() {
        let dir = std::env::temp_dir().join(format!("w11_appx_dep_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let x64_dep = dir.join("Microsoft.VCLibs.x64.14.00.appx");
        let neutral_dep = dir.join("Framework_1.0_neutral_~_8wekyb3d8bbwe.msixbundle");
        let x86_dep = dir.join("Microsoft.VCLibs.x86.14.00.appx");
        let arm64_dep = dir.join("Microsoft.UI.Xaml.2.8_arm64.appx");
        let txt = dir.join("readme.txt");

        for p in [&x64_dep, &neutral_dep, &x86_dep, &arm64_dep, &txt] {
            fs::write(p, b"dummy").unwrap();
        }

        let res = scan_appx_dependencies(&[dir.to_string_lossy().to_string()]);
        assert_eq!(res.compatible.len(), 2);
        assert_eq!(res.skipped_incompatible.len(), 2);
        assert!(
            res.skipped_incompatible
                .iter()
                .any(|s| s.detected_arch == "x86")
        );
        assert!(
            res.skipped_incompatible
                .iter()
                .any(|s| s.detected_arch == "arm64")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn builds_add_appx_dism_args_correctly() {
        let req = AddAppxRequest {
            package_path: r"C:\Apps\MyApp.msixbundle".to_string(),
            dependency_paths: vec![
                r"C:\Apps\Microsoft.VCLibs.x64.appx".to_string(),
                r"C:\Apps\Microsoft.VCLibs.x86.appx".to_string(),
            ],
            license_path: None,
            skip_license: true,
            region_all: true,
            stub_option: StubOption::UsePreference,
        };
        let args = build_add_appx_args(
            Path::new(r"C:\work\mount"),
            Path::new(r"C:\work\dism.log"),
            &req,
        );
        assert!(args.contains(&"/SkipLicense".to_string()));
        assert!(args.contains(&"/Region:all".to_string()));
        assert!(args.contains(&"/StubPackageOption:UsePreference".to_string()));
        assert!(
            args.contains(&r"/DependencyPackagePath:C:\Apps\Microsoft.VCLibs.x64.appx".to_string())
        );
        assert!(
            !args
                .contains(&r"/DependencyPackagePath:C:\Apps\Microsoft.VCLibs.x86.appx".to_string())
        );
    }

    #[test]
    fn builds_appx_args_with_spaces_cyrillic_and_license() {
        let req = AddAppxRequest {
            package_path: r"C:\Пакеты Win11\My App\App.msixbundle".to_string(),
            dependency_paths: vec![
                r"C:\Пакеты Win11\Deps\Microsoft.UI.Xaml.x64.appx".to_string(),
                r"C:\Пакеты Win11\Deps\Neutral.Lib.appx".to_string(),
            ],
            license_path: Some(r"C:\Пакеты Win11\License File\app_license.xml".to_string()),
            skip_license: false,
            region_all: true,
            stub_option: StubOption::UseStub,
        };
        let args = build_add_appx_args(
            Path::new(r"C:\Рабочая папка\mount_install"),
            Path::new(r"C:\Рабочая папка\dism.log"),
            &req,
        );
        assert!(args.contains(&r"/Image:C:\Рабочая папка\mount_install".to_string()));
        assert!(args.contains(&r"/LogPath:C:\Рабочая папка\dism.log".to_string()));
        assert!(args.contains(&r"/PackagePath:C:\Пакеты Win11\My App\App.msixbundle".to_string()));
        assert!(args.contains(
            &r"/DependencyPackagePath:C:\Пакеты Win11\Deps\Microsoft.UI.Xaml.x64.appx".to_string()
        ));
        assert!(args.contains(
            &r"/DependencyPackagePath:C:\Пакеты Win11\Deps\Neutral.Lib.appx".to_string()
        ));
        assert!(
            args.contains(
                &r"/LicensePath:C:\Пакеты Win11\License File\app_license.xml".to_string()
            )
        );
        assert!(!args.contains(&"/SkipLicense".to_string()));
        assert!(args.contains(&"/StubPackageOption:UseStub".to_string()));
        assert!(args.contains(&"/Region:all".to_string()));
    }

    #[test]
    fn classifies_win32_x64_and_scans_subfolders_recursively() {
        assert_eq!(
            classify_appx_arch(Path::new(r"C:\Deps\MyApp_Win32_x64.msix")),
            "x64"
        );
        assert_eq!(
            classify_appx_arch(Path::new(r"C:\Deps\MyApp x86.appx")),
            "x86"
        );

        let dir = std::env::temp_dir().join(format!("w11_appx_rec_{}", std::process::id()));
        let sub_x64 = dir.join("x64");
        let sub_x86 = dir.join("x86");
        fs::create_dir_all(&sub_x64).unwrap();
        fs::create_dir_all(&sub_x86).unwrap();
        fs::write(sub_x64.join("VCLibs_Win32_x64.appx"), b"x64").unwrap();
        fs::write(sub_x86.join("VCLibs_x86.appx"), b"x86").unwrap();

        let res = scan_appx_dependencies(&[dir.to_string_lossy().to_string()]);
        assert_eq!(res.compatible.len(), 1);
        assert!(res.compatible[0].ends_with("VCLibs_Win32_x64.appx"));
        assert_eq!(res.skipped_incompatible.len(), 1);
        assert_eq!(res.skipped_incompatible[0].detected_arch, "x86");

        let _ = fs::remove_dir_all(&dir);
    }
}
