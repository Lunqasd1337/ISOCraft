use crate::image::decode_text_bytes;
use crate::process::{run_tracked_command, ProcessGuard};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutounattendValidation {
    pub valid: bool,
    pub root_element: Option<String>,
    pub passes: Vec<String>,
    pub error: Option<String>,
}

pub fn validate_autounattend(path: &Path) -> AutounattendValidation {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return AutounattendValidation {
                valid: false,
                root_element: None,
                passes: Vec::new(),
                error: Some(format!("Failed to read file: {e}")),
            }
        }
    };

    let content = decode_text_bytes(&bytes);
    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    let mut root_element: Option<String> = None;
    let mut passes = Vec::new();
    let mut open_tags: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_string();
                let local = name.rsplit(':').next().unwrap_or(&name).to_string();

                if root_element.is_none() {
                    root_element = Some(local.clone());
                    if !local.eq_ignore_ascii_case("unattend") {
                        return AutounattendValidation {
                            valid: false,
                            root_element,
                            passes,
                            error: Some(format!(
                                "Expected root XML element <unattend>, found <{local}>."
                            )),
                        };
                    }
                } else if local.eq_ignore_ascii_case("settings") {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().eq_ignore_ascii_case("pass") {
                            let pass_val = attr.value.trim().to_string();
                            if !pass_val.is_empty() && !passes.contains(&pass_val) {
                                passes.push(pass_val);
                            }
                        }
                    }
                }
                open_tags.push(local);
            }
            Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_string();
                let local = name.rsplit(':').next().unwrap_or(&name).to_string();
                if root_element.is_none() {
                    root_element = Some(local.clone());
                    if !local.eq_ignore_ascii_case("unattend") {
                        return AutounattendValidation {
                            valid: false,
                            root_element,
                            passes,
                            error: Some(format!(
                                "Expected root XML element <unattend>, found <{local}>."
                            )),
                        };
                    }
                } else if local.eq_ignore_ascii_case("settings") {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().eq_ignore_ascii_case("pass") {
                            let pass_val = attr.value.trim().to_string();
                            if !pass_val.is_empty() && !passes.contains(&pass_val) {
                                passes.push(pass_val);
                            }
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name().as_ref().to_string();
                let local = name.rsplit(':').next().unwrap_or(&name).to_string();
                match open_tags.pop() {
                    Some(expected) if expected == local => {}
                    Some(expected) => {
                        return AutounattendValidation {
                            valid: false,
                            root_element,
                            passes,
                            error: Some(format!(
                                "Mismatched closing tag </{local}>, expected </{expected}>."
                            )),
                        };
                    }
                    None => {
                        return AutounattendValidation {
                            valid: false,
                            root_element,
                            passes,
                            error: Some(format!("Unexpected closing tag </{local}>.")),
                        };
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return AutounattendValidation {
                    valid: false,
                    root_element,
                    passes,
                    error: Some(format!("Invalid XML syntax: {e}")),
                }
            }
            _ => {}
        }
    }

    if !open_tags.is_empty() {
        return AutounattendValidation {
            valid: false,
            root_element,
            passes,
            error: Some(format!("Unclosed XML element(s): {}", open_tags.join(", "))),
        };
    }

    if root_element.is_none() {
        return AutounattendValidation {
            valid: false,
            root_element: None,
            passes,
            error: Some("XML file contains no root element.".to_string()),
        };
    }

    AutounattendValidation {
        valid: true,
        root_element,
        passes,
        error: None,
    }
}

pub fn write_generic_ei_cfg(iso_root: &Path) -> Result<(), String> {
    let sources = iso_root.join("sources");
    fs::create_dir_all(&sources).map_err(|e| format!("Failed creating sources directory: {e}"))?;
    let ei_path = sources.join("ei.cfg");
    let content = "[Channel]\r\nRetail\r\n[VL]\r\n0\r\n";
    fs::write(&ei_path, content).map_err(|e| format!("Failed writing '{}': {e}", ei_path.display()))
}

pub fn to_cygwin_path(win_path: &Path) -> String {
    let raw = win_path.to_string_lossy().replace('\\', "/");
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = &raw[2..];
        let rest_trimmed = rest.strip_prefix('/').unwrap_or(rest);
        if rest_trimmed.is_empty() {
            format!("/cygdrive/{drive}")
        } else {
            format!("/cygdrive/{drive}/{rest_trimmed}")
        }
    } else {
        raw
    }
}

pub struct IsoBuildOptions<'a> {
    pub iso_root: &'a Path,
    pub output_iso: &'a Path,
    pub volume_label: &'a str,
    pub uefi_no_prompt: bool,
    pub autounattend_path: Option<&'a Path>,
    pub add_generic_ei_cfg: bool,
    pub oscdimg_path: Option<&'a Path>,
    pub xorriso_path: Option<&'a Path>,
}

pub async fn build_bootable_iso<F>(
    opts: IsoBuildOptions<'_>,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<String, String>
where
    F: FnMut(&str) + Send,
{
    if let Some(autounattend) = opts.autounattend_path {
        let validation = validate_autounattend(autounattend);
        if !validation.valid {
            return Err(format!(
                "Invalid autounattend.xml: {}",
                validation
                    .error
                    .unwrap_or_else(|| "unknown XML error".to_string())
            ));
        }
        let dest = opts.iso_root.join("autounattend.xml");
        fs::copy(autounattend, &dest)
            .map_err(|e| format!("Failed copying autounattend.xml into ISO root: {e}"))?;
        on_log(&format!("Copied autounattend.xml to '{}'", dest.display()));
    }

    if opts.add_generic_ei_cfg {
        write_generic_ei_cfg(opts.iso_root)?;
        on_log("Generated sources\\ei.cfg (Retail).");
    }

    let bios_boot_rel = "boot/etfsboot.com";
    let bios_boot_abs = opts.iso_root.join("boot").join("etfsboot.com");
    if !bios_boot_abs.is_file() {
        return Err(format!(
            "BIOS boot image missing in ISO root: {}",
            bios_boot_abs.display()
        ));
    }

    let efi_noprompt_abs = opts
        .iso_root
        .join("efi")
        .join("microsoft")
        .join("boot")
        .join("efisys_noprompt.bin");
    let efi_prompt_abs = opts
        .iso_root
        .join("efi")
        .join("microsoft")
        .join("boot")
        .join("efisys.bin");

    let (uefi_boot_rel, uefi_boot_abs) = if opts.uefi_no_prompt && efi_noprompt_abs.is_file() {
        ("efi/microsoft/boot/efisys_noprompt.bin", efi_noprompt_abs)
    } else if efi_prompt_abs.is_file() {
        ("efi/microsoft/boot/efisys.bin", efi_prompt_abs)
    } else {
        return Err(
            "UEFI boot image (efisys.bin / efisys_noprompt.bin) missing in ISO root.".to_string(),
        );
    };

    if let Some(parent) = opts.output_iso.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed creating output directory '{}': {e}",
                parent.display()
            )
        })?;
    }
    if opts.output_iso.exists() {
        fs::remove_file(opts.output_iso).map_err(|e| {
            format!(
                "Failed removing existing ISO '{}': {e}",
                opts.output_iso.display()
            )
        })?;
    }

    let clean_label: String = opts
        .volume_label
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(32)
        .collect();
    let label = if clean_label.is_empty() {
        "WIN11_CUSTOM".to_string()
    } else {
        clean_label
    };

    if let Some(oscdimg) = opts.oscdimg_path.filter(|p| p.is_file()) {
        on_log(&format!(
            "Building ISO using oscdimg.exe: {}",
            oscdimg.display()
        ));
        let args = build_oscdimg_args(
            opts.iso_root,
            opts.output_iso,
            &bios_boot_abs,
            &uefi_boot_abs,
            &label,
        );

        let out = run_tracked_command(&oscdimg.to_string_lossy(), &args, None, guard, |line| {
            if !line.trim().is_empty() {
                on_log(line);
            }
        })
        .await?;

        if out.exit_code != 0 {
            return Err(format!(
                "oscdimg.exe failed (code {}): {} {}",
                out.exit_code,
                out.stdout.trim(),
                out.stderr.trim()
            ));
        }
        return Ok("oscdimg".to_string());
    }

    if let Some(xorriso) = opts.xorriso_path.filter(|p| p.is_file()) {
        on_log(&format!(
            "Building ISO using xorriso fallback: {}",
            xorriso.display()
        ));
        let args = build_xorriso_args(
            opts.iso_root,
            opts.output_iso,
            bios_boot_rel,
            uefi_boot_rel,
            &label,
        );

        let out = run_tracked_command(&xorriso.to_string_lossy(), &args, None, guard, |line| {
            if !line.trim().is_empty() {
                on_log(line);
            }
        })
        .await?;

        if out.exit_code != 0 {
            return Err(format!(
                "xorriso failed (code {}): {} {}",
                out.exit_code,
                out.stdout.trim(),
                out.stderr.trim()
            ));
        }
        return Ok("xorriso".to_string());
    }

    Err("Neither oscdimg.exe nor xorriso.exe is available to build the ISO.".to_string())
}

pub fn build_oscdimg_args(
    iso_root: &Path,
    output_iso: &Path,
    bios_boot_abs: &Path,
    uefi_boot_abs: &Path,
    label: &str,
) -> Vec<String> {
    let bootdata = format!(
        "2#p0,e,b{}#pEF,e,b{}",
        bios_boot_abs.display(),
        uefi_boot_abs.display()
    );
    vec![
        "-m".to_string(),
        "-o".to_string(),
        "-u2".to_string(),
        "-udfver102".to_string(),
        format!("-l{label}"),
        format!("-bootdata:{bootdata}"),
        iso_root.to_string_lossy().to_string(),
        output_iso.to_string_lossy().to_string(),
    ]
}

pub fn build_xorriso_args(
    iso_root: &Path,
    output_iso: &Path,
    bios_boot_rel: &str,
    uefi_boot_rel: &str,
    label: &str,
) -> Vec<String> {
    let cyg_out = to_cygwin_path(output_iso);
    let cyg_root = to_cygwin_path(iso_root);

    vec![
        "-as".to_string(),
        "mkisofs".to_string(),
        "-iso-level".to_string(),
        "3".to_string(),
        "-full-iso9660-filenames".to_string(),
        "-relaxed-filenames".to_string(),
        "-allow-lowercase".to_string(),
        "-J".to_string(),
        "-joliet-long".to_string(),
        "-V".to_string(),
        label.to_string(),
        "-b".to_string(),
        bios_boot_rel.to_string(),
        "-no-emul-boot".to_string(),
        "-boot-load-size".to_string(),
        "8".to_string(),
        "-eltorito-alt-boot".to_string(),
        "-e".to_string(),
        uefi_boot_rel.to_string(),
        "-no-emul-boot".to_string(),
        "-isohybrid-gpt-basdat".to_string(),
        "-o".to_string(),
        cyg_out,
        cyg_root,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_windows_paths_to_cygwin_with_spaces_and_cyrillic() {
        assert_eq!(
            to_cygwin_path(Path::new(r"C:\Projects\ISOCraft App\Образ 24H2\out.iso")),
            "/cygdrive/c/Projects/ISOCraft App/Образ 24H2/out.iso"
        );
        assert_eq!(to_cygwin_path(Path::new(r"D:\")), "/cygdrive/d");
    }

    #[test]
    fn validates_autounattend_xml_strictly() {
        let dir = std::env::temp_dir().join(format!("w11_xml_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let good = dir.join("autounattend.xml");
        fs::write(
            &good,
            r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="windowsPE"></settings>
  <settings pass="oobeSystem"></settings>
</unattend>"#,
        )
        .unwrap();
        let res = validate_autounattend(&good);
        assert!(res.valid);
        assert_eq!(res.root_element.as_deref(), Some("unattend"));
        assert_eq!(res.passes, vec!["windowsPE", "oobeSystem"]);

        let wrong_root = dir.join("wrong.xml");
        fs::write(&wrong_root, "<configuration><item/></configuration>").unwrap();
        let res_bad = validate_autounattend(&wrong_root);
        assert!(!res_bad.valid);

        let broken = dir.join("broken.xml");
        fs::write(&broken, "<unattend><unclosed>").unwrap();
        let res_broken = validate_autounattend(&broken);
        assert!(!res_broken.valid);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_generic_ei_cfg_crlf() {
        let dir = std::env::temp_dir().join(format!("w11_eicfg_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        write_generic_ei_cfg(&dir).unwrap();
        let content = fs::read_to_string(dir.join("sources").join("ei.cfg")).unwrap();
        assert_eq!(content, "[Channel]\r\nRetail\r\n[VL]\r\n0\r\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn builds_oscdimg_args_with_spaces_and_cyrillic() {
        let root = Path::new(r"C:\Рабочая папка\iso_root");
        let out = Path::new(r"D:\Сборка Win11\ISOCraft Final.iso");
        let bios = root.join("boot").join("etfsboot.com");
        let uefi = root
            .join("efi")
            .join("microsoft")
            .join("boot")
            .join("efisys_noprompt.bin");

        let args = build_oscdimg_args(root, out, &bios, &uefi, "WIN11_CUSTOM");
        assert!(args.contains(&"-m".to_string()));
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"-u2".to_string()));
        assert!(args.contains(&"-udfver102".to_string()));
        assert!(args.contains(&"-lWIN11_CUSTOM".to_string()));
        let bootdata = args.iter().find(|a| a.starts_with("-bootdata:")).unwrap();
        assert_eq!(
            bootdata,
            r"-bootdata:2#p0,e,bC:\Рабочая папка\iso_root\boot\etfsboot.com#pEF,e,bC:\Рабочая папка\iso_root\efi\microsoft\boot\efisys_noprompt.bin"
        );
        assert_eq!(args[args.len() - 2], r"C:\Рабочая папка\iso_root");
        assert_eq!(args[args.len() - 1], r"D:\Сборка Win11\ISOCraft Final.iso");
    }

    #[test]
    fn builds_xorriso_args_with_cygwin_paths_and_dual_boot() {
        let root = Path::new(r"C:\Мои проекты\ISOCraft App\iso_root");
        let out = Path::new(r"E:\Готовые образы\Win11 Custom.iso");

        let args = build_xorriso_args(
            root,
            out,
            "boot/etfsboot.com",
            "efi/microsoft/boot/efisys_noprompt.bin",
            "ISOCRAFT_11",
        );
        assert!(!args.contains(&"-udf".to_string()));
        assert_eq!(args[0], "-as");
        assert_eq!(args[1], "mkisofs");
        assert!(args.windows(2).any(|w| w == ["-iso-level", "3"]));
        assert!(args.windows(2).any(|w| w == ["-b", "boot/etfsboot.com"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["-e", "efi/microsoft/boot/efisys_noprompt.bin"]));
        assert_eq!(
            args[args.len() - 2],
            "/cygdrive/e/Готовые образы/Win11 Custom.iso"
        );
        assert_eq!(
            args[args.len() - 1],
            "/cygdrive/c/Мои проекты/ISOCraft App/iso_root"
        );
    }
}
