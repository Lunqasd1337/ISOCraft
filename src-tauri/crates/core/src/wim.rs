use crate::env::WorkspaceState;
use crate::image::run_wimlib_command;
use crate::process::{run_tracked_command, ProcessGuard};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallOutputFormat {
    Wim,
    Esd,
    Swm,
}

pub fn parse_dism_progress(line: &str) -> Option<f32> {
    // Matches DISM progress bar lines like: "[==========================100.0%==========================]"
    // or " 45.2% "
    let pct_idx = line.find('%')?;
    let prefix = &line[..pct_idx];
    let num_start = prefix
        .rfind(|c: char| !c.is_ascii_digit() && c != '.')
        .map(|i| i + 1)
        .unwrap_or(0);
    let num_str = prefix[num_start..].trim();
    if num_str.is_empty() {
        return None;
    }
    let val = num_str.parse::<f32>().ok()?;
    if (0.0..=100.0).contains(&val) {
        Some(val)
    } else {
        None
    }
}

pub fn parse_wimlib_progress(line: &str) -> Option<f32> {
    // Matches e.g. "1234 MiB of 4567 MiB (27%) done" or "(100%)"
    if let Some(open) = line.rfind('(') {
        if let Some(pct_rel) = line[open + 1..].find('%') {
            let num_str = line[open + 1..open + 1 + pct_rel].trim();
            if let Ok(val) = num_str.parse::<f32>() {
                if (0.0..=100.0).contains(&val) {
                    return Some(val);
                }
            }
        }
    }
    None
}

fn state_file_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("workspace_state.json")
}

pub fn read_workspace_state(workspace_dir: &Path) -> WorkspaceState {
    let path = state_file_path(workspace_dir);
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(parsed) = serde_json::from_str::<WorkspaceState>(&raw) {
            return parsed;
        }
    }
    WorkspaceState {
        active_mounts: Vec::new(),
        loaded_hives: Vec::new(),
        updated_at: Local::now().to_rfc3339(),
    }
}

pub fn write_workspace_state(workspace_dir: &Path, state: &WorkspaceState) -> Result<(), String> {
    fs::create_dir_all(workspace_dir)
        .map_err(|e| format!("Failed creating workspace directory: {e}"))?;
    let path = state_file_path(workspace_dir);
    if state.active_mounts.is_empty() && state.loaded_hives.is_empty() {
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        return Ok(());
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed serializing workspace_state.json: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed writing workspace_state.json: {e}"))
}

fn register_mount_in_state(workspace_dir: &Path, mount_dir: &Path) -> Result<(), String> {
    let mut state = read_workspace_state(workspace_dir);
    let m = mount_dir.to_string_lossy().to_string();
    if !state.active_mounts.contains(&m) {
        state.active_mounts.push(m);
    }
    state.updated_at = Local::now().to_rfc3339();
    write_workspace_state(workspace_dir, &state)
}

fn unregister_mount_in_state(workspace_dir: &Path, mount_dir: &Path) -> Result<(), String> {
    let mut state = read_workspace_state(workspace_dir);
    let target = mount_dir.to_string_lossy().to_string();
    state
        .active_mounts
        .retain(|m| !m.eq_ignore_ascii_case(&target));
    state.updated_at = Local::now().to_rfc3339();
    write_workspace_state(workspace_dir, &state)
}

pub fn register_hive_in_state(workspace_dir: &Path, hive_name: &str) -> Result<(), String> {
    let mut state = read_workspace_state(workspace_dir);
    if !state
        .loaded_hives
        .iter()
        .any(|h| h.eq_ignore_ascii_case(hive_name))
    {
        state.loaded_hives.push(hive_name.to_string());
    }
    state.updated_at = Local::now().to_rfc3339();
    write_workspace_state(workspace_dir, &state)
}

pub fn unregister_hive_in_state(workspace_dir: &Path, hive_name: &str) -> Result<(), String> {
    let mut state = read_workspace_state(workspace_dir);
    state
        .loaded_hives
        .retain(|h| !h.eq_ignore_ascii_case(hive_name));
    state.updated_at = Local::now().to_rfc3339();
    write_workspace_state(workspace_dir, &state)
}

/// Exports ONLY the chosen edition index from source `.wim` or `.esd` into a single-edition
/// `install.wim` (becomes Index 1), trimming all unused editions.
pub async fn export_single_edition<F>(
    wimlib_path: &Path,
    src_install: &Path,
    selected_index: u32,
    dest_wim: &Path,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    if let Some(parent) = dest_wim.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating directory '{}': {e}", parent.display()))?;
    }
    if dest_wim.exists() {
        fs::remove_file(dest_wim)
            .map_err(|e| format!("Failed removing existing '{}': {e}", dest_wim.display()))?;
    }

    on_log(&format!(
        "Exporting single edition (Index {selected_index}) from '{}' -> '{}'",
        src_install.display(),
        dest_wim.display()
    ));

    let args = vec![
        "export".to_string(),
        src_install.to_string_lossy().to_string(),
        selected_index.to_string(),
        dest_wim.to_string_lossy().to_string(),
        "--compress=LZX".to_string(),
    ];

    run_wimlib_command(wimlib_path, &args, guard, |line| {
        if !line.trim().is_empty() {
            on_log(line);
        }
    })
    .await
}

pub async fn mount_wim<F>(
    dism_path: &Path,
    wim_path: &Path,
    index: u32,
    mount_dir: &Path,
    read_only: bool,
    workspace_dir: &Path,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    fs::create_dir_all(mount_dir)
        .map_err(|e| format!("Failed creating mount dir '{}': {e}", mount_dir.display()))?;

    register_mount_in_state(workspace_dir, mount_dir)?;

    let log_path = workspace_dir.join("dism.log");
    let mut args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        "/Mount-Image".to_string(),
        format!("/ImageFile:{}", wim_path.display()),
        format!("/Index:{index}"),
        format!("/MountDir:{}", mount_dir.display()),
    ];
    if read_only {
        args.push("/ReadOnly".to_string());
    }

    on_log(&format!(
        "Mounting '{}' (Index {index}, ro={read_only}) at '{}'",
        wim_path.display(),
        mount_dir.display()
    ));

    let res = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |line| {
        if !line.trim().is_empty() {
            on_log(line);
        }
    })
    .await;

    match res {
        Ok(out) if out.exit_code == 0 => Ok(()),
        Ok(out) => {
            let _ = unregister_mount_in_state(workspace_dir, mount_dir);
            Err(format!(
                "DISM /Mount-Image failed (code {}): {} {}",
                out.exit_code,
                out.stdout.trim(),
                out.stderr.trim()
            ))
        }
        Err(e) => {
            let _ = unregister_mount_in_state(workspace_dir, mount_dir);
            Err(e)
        }
    }
}

pub async fn unmount_wim<F>(
    dism_path: &Path,
    mount_dir: &Path,
    commit: bool,
    workspace_dir: &Path,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let log_path = workspace_dir.join("dism.log");
    let mode = if commit { "/Commit" } else { "/Discard" };
    let args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        "/Unmount-Image".to_string(),
        format!("/MountDir:{}", mount_dir.display()),
        mode.to_string(),
    ];

    on_log(&format!("Unmounting '{}' ({mode})...", mount_dir.display()));

    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, guard, |line| {
        if !line.trim().is_empty() {
            on_log(line);
        }
    })
    .await?;

    if out.exit_code != 0 {
        return Err(format!(
            "DISM /Unmount-Image {mode} failed (code {}): {} {}",
            out.exit_code,
            out.stdout.trim(),
            out.stderr.trim()
        ));
    }

    unregister_mount_in_state(workspace_dir, mount_dir)?;
    Ok(())
}

pub async fn recover_workspace_async<F>(
    dism_path: &Path,
    workspace_dir: &Path,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let state = read_workspace_state(workspace_dir);
    let log_path = workspace_dir.join("dism.log");

    // Unload any stale offline registry hives strictly BEFORE discarding WIM mounts
    for hive_name in &state.loaded_hives {
        on_log(&format!("Unloading stale registry hive: HKLM\\{hive_name}"));
        if let Err(err) = crate::registry::unload_hive_by_name(hive_name) {
            on_log(&format!("Warning unloading hive HKLM\\{hive_name}: {err}"));
        }
    }

    for mount in &state.active_mounts {
        let mount_path = PathBuf::from(mount);
        if mount_path.exists() {
            on_log(&format!("Discarding stale WIM mount: {mount}"));
            let args = vec![
                "/English".to_string(),
                format!("/LogPath:{}", log_path.display()),
                "/Unmount-Image".to_string(),
                format!("/MountDir:{mount}"),
                "/Discard".to_string(),
            ];
            let _ = run_tracked_command(&dism_path.to_string_lossy(), &args, None, None, |l| {
                if !l.trim().is_empty() {
                    on_log(l);
                }
            })
            .await;
        }
    }

    on_log("Running DISM /Cleanup-Wim...");
    let cleanup_args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        "/Cleanup-Wim".to_string(),
    ];
    let _ = run_tracked_command(
        &dism_path.to_string_lossy(),
        &cleanup_args,
        None,
        None,
        |l| {
            if !l.trim().is_empty() {
                on_log(l);
            }
        },
    )
    .await;

    let sf = state_file_path(workspace_dir);
    if sf.exists() {
        let _ = fs::remove_file(&sf);
    }
    Ok(())
}

/// Packages a single-index `install.wim` into the requested output format (`wim`, `esd`, or `swm`).
/// Removes stale `install.wim` / `install.esd` / `install*.swm` files in `dest_dir` first.
pub async fn package_install_image<F>(
    wimlib_path: &Path,
    src_wim: &Path,
    dest_dir: &Path,
    format: &InstallOutputFormat,
    split_size_mb: u32,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<Vec<PathBuf>, String>
where
    F: FnMut(&str) + Send,
{
    fs::create_dir_all(dest_dir).map_err(|e| {
        format!(
            "Failed creating destination dir '{}': {e}",
            dest_dir.display()
        )
    })?;

    // Clean any pre-existing install.* artifacts in dest_dir (when packaging inside iso_root\sources)
    if let Ok(entries) = fs::read_dir(dest_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let is_install_art = name == "install.wim"
                || name == "install.esd"
                || (name.starts_with("install") && name.ends_with(".swm"));
            if is_install_art && entry.path() != src_wim {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    match format {
        InstallOutputFormat::Wim => {
            let dest_wim = dest_dir.join("install.wim");
            if src_wim != dest_wim {
                on_log(&format!(
                    "Optimizing/exporting install.wim to '{}'...",
                    dest_wim.display()
                ));
                let args = vec![
                    "export".to_string(),
                    src_wim.to_string_lossy().to_string(),
                    "1".to_string(),
                    dest_wim.to_string_lossy().to_string(),
                    "--compress=LZX".to_string(),
                ];
                run_wimlib_command(wimlib_path, &args, guard, |l| {
                    if !l.trim().is_empty() {
                        on_log(l);
                    }
                })
                .await?;
            }
            Ok(vec![dest_wim])
        }
        InstallOutputFormat::Esd => {
            let dest_esd = dest_dir.join("install.esd");
            on_log(&format!(
                "Compressing to install.esd (LZMS solid) at '{}'...",
                dest_esd.display()
            ));
            let args = vec![
                "export".to_string(),
                src_wim.to_string_lossy().to_string(),
                "1".to_string(),
                dest_esd.to_string_lossy().to_string(),
                "--compress=LZMS".to_string(),
                "--solid".to_string(),
            ];
            run_wimlib_command(wimlib_path, &args, guard, |l| {
                if !l.trim().is_empty() {
                    on_log(l);
                }
            })
            .await?;
            Ok(vec![dest_esd])
        }
        InstallOutputFormat::Swm => {
            let size = split_size_mb.max(100);
            let base_swm = dest_dir.join("install.swm");
            on_log(&format!(
                "Splitting install.wim into {size} MB parts at '{}'...",
                base_swm.display()
            ));
            let args = vec![
                "split".to_string(),
                src_wim.to_string_lossy().to_string(),
                base_swm.to_string_lossy().to_string(),
                size.to_string(),
            ];
            run_wimlib_command(wimlib_path, &args, guard, |l| {
                if !l.trim().is_empty() {
                    on_log(l);
                }
            })
            .await?;

            let mut parts = Vec::new();
            if let Ok(entries) = fs::read_dir(dest_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    if name.starts_with("install") && name.ends_with(".swm") {
                        parts.push(entry.path());
                    }
                }
            }
            parts.sort();
            Ok(parts)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dism_progress_lines() {
        assert_eq!(
            parse_dism_progress("[=========== 45.0%          ]"),
            Some(45.0)
        );
        assert_eq!(
            parse_dism_progress("[==========================100.0%==========================]"),
            Some(100.0)
        );
        assert_eq!(
            parse_dism_progress("Deployment Image Servicing and Management tool"),
            None
        );
    }

    #[test]
    fn parses_wimlib_progress_lines() {
        assert_eq!(
            parse_wimlib_progress(
                "Writing LZX-compressed data: 1234 MiB of 4567 MiB (27%) written"
            ),
            Some(27.0)
        );
        assert_eq!(parse_wimlib_progress("Done exporting WIM image."), None);
    }

    #[test]
    fn workspace_state_roundtrip_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("w11_ws_test_{}", std::process::id()));
        let mount = dir.join("mount");

        register_mount_in_state(&dir, &mount).unwrap();
        register_hive_in_state(&dir, "ISOCRAFT_123_SOFTWARE").unwrap();
        let st = read_workspace_state(&dir);
        assert_eq!(st.active_mounts.len(), 1);
        assert_eq!(st.loaded_hives, vec!["ISOCRAFT_123_SOFTWARE"]);
        assert!(dir.join("workspace_state.json").exists());

        unregister_hive_in_state(&dir, "ISOCRAFT_123_SOFTWARE").unwrap();
        assert!(dir.join("workspace_state.json").exists());

        unregister_mount_in_state(&dir, &mount).unwrap();
        let st2 = read_workspace_state(&dir);
        assert!(st2.active_mounts.is_empty());
        assert!(st2.loaded_hives.is_empty());
        assert!(!dir.join("workspace_state.json").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_dism_and_wimlib_progress_edge_cases() {
        assert_eq!(parse_dism_progress("[0.0%]"), Some(0.0));
        assert_eq!(parse_dism_progress("[== 5.5% ==]"), Some(5.5));
        assert_eq!(parse_dism_progress("  100.0%  "), Some(100.0));
        assert_eq!(parse_dism_progress("150.0%"), None);
        assert_eq!(parse_dism_progress("no percent here"), None);

        assert_eq!(
            parse_wimlib_progress("Archiving file data: 0 MiB of 100 MiB (0%) done"),
            Some(0.0)
        );
        assert_eq!(
            parse_wimlib_progress(
                "Writing LZX-compressed data: 4567 MiB of 4567 MiB (100%) written"
            ),
            Some(100.0)
        );
        assert_eq!(parse_wimlib_progress("Invalid (120%) percentage"), None);
        assert_eq!(parse_wimlib_progress("Finished."), None);
    }
}
