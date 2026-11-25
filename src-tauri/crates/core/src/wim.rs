use crate::env::WorkspaceState;
use crate::fs_utils::{copy_file_cancellable, is_path_within_dir, write_file_atomic};
use crate::image::run_wimlib_command;
use crate::process::{ProcessGuard, run_tracked_command};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static WORKSPACE_STATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallOutputFormat {
    Wim,
    Esd,
}

pub fn parse_dism_progress(line: &str) -> Option<f32> {
    // Matches DISM progress bar lines like: "[==========================100.0%==========================]"
    // or " 45.2% " or localized prefixes ending directly before digits.
    let pct_idx = line.find('%')?;
    let prefix = &line[..pct_idx];
    let num_start = prefix
        .char_indices()
        .rev()
        .find(|&(_, c)| !c.is_ascii_digit() && c != '.')
        .map(|(i, c)| i + c.len_utf8())
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
    if let Some(open) = line.rfind('(')
        && let Some(pct_rel) = line[open + 1..].find('%')
    {
        let num_str = line[open + 1..open + 1 + pct_rel].trim();
        if let Ok(val) = num_str.parse::<f32>()
            && (0.0..=100.0).contains(&val)
        {
            return Some(val);
        }
    }
    None
}

fn state_file_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("workspace_state.json")
}

pub fn read_workspace_state_status(workspace_dir: &Path) -> (WorkspaceState, bool) {
    let path = state_file_path(workspace_dir);
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<WorkspaceState>(&raw) {
                Ok(parsed) => return (parsed, false),
                Err(_) => {
                    return (
                        WorkspaceState {
                            active_mounts: Vec::new(),
                            loaded_hives: Vec::new(),
                            updated_at: Local::now().to_rfc3339(),
                        },
                        true,
                    );
                }
            },
            Err(_) => {
                return (
                    WorkspaceState {
                        active_mounts: Vec::new(),
                        loaded_hives: Vec::new(),
                        updated_at: Local::now().to_rfc3339(),
                    },
                    true,
                );
            }
        }
    }
    (
        WorkspaceState {
            active_mounts: Vec::new(),
            loaded_hives: Vec::new(),
            updated_at: Local::now().to_rfc3339(),
        },
        false,
    )
}

pub fn read_workspace_state(workspace_dir: &Path) -> WorkspaceState {
    read_workspace_state_status(workspace_dir).0
}

pub fn write_workspace_state(workspace_dir: &Path, state: &WorkspaceState) -> Result<(), String> {
    fs::create_dir_all(workspace_dir)
        .map_err(|e| format!("Failed creating workspace directory: {e}"))?;
    let path = state_file_path(workspace_dir);
    if state.active_mounts.is_empty() && state.loaded_hives.is_empty() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed removing workspace_state.json: {e}"))?;
        }
        return Ok(());
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed serializing workspace_state.json: {e}"))?;
    write_file_atomic(&path, json.as_bytes())
        .map_err(|e| format!("Failed writing workspace_state.json: {e}"))
}

fn register_mount_in_state(workspace_dir: &Path, mount_dir: &Path) -> Result<(), String> {
    let _lock = WORKSPACE_STATE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut state = read_workspace_state(workspace_dir);
    let m = mount_dir.to_string_lossy().to_string();
    if !state.active_mounts.contains(&m) {
        state.active_mounts.push(m);
    }
    state.updated_at = Local::now().to_rfc3339();
    write_workspace_state(workspace_dir, &state)
}

fn unregister_mount_in_state(workspace_dir: &Path, mount_dir: &Path) -> Result<(), String> {
    let _lock = WORKSPACE_STATE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut state = read_workspace_state(workspace_dir);
    let target = mount_dir.to_string_lossy().to_string();
    state
        .active_mounts
        .retain(|m| !m.eq_ignore_ascii_case(&target));
    state.updated_at = Local::now().to_rfc3339();
    write_workspace_state(workspace_dir, &state)
}

pub fn register_hive_in_state(workspace_dir: &Path, hive_name: &str) -> Result<(), String> {
    let _lock = WORKSPACE_STATE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
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
    let _lock = WORKSPACE_STATE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
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
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed creating directory '{}': {e}", parent.display()))?;
    }
    if dest_wim.exists() {
        tokio::fs::remove_file(dest_wim)
            .await
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

#[allow(clippy::too_many_arguments)]
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
    let existing_state = read_workspace_state(workspace_dir);
    let mount_str = mount_dir.to_string_lossy();
    if existing_state
        .active_mounts
        .iter()
        .any(|m| m.eq_ignore_ascii_case(&mount_str))
    {
        return Err(format!(
            "Mount directory '{}' is already registered as an active WIM mount. Run workspace recovery first.",
            mount_dir.display()
        ));
    }

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
            let cancelled = guard.map(|g| g.is_cancel_requested()).unwrap_or(false);
            if !cancelled {
                let _ = unregister_mount_in_state(workspace_dir, mount_dir);
            }
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

pub fn parse_dism_mounted_dirs(stdout: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some((lhs, rhs)) = trimmed.split_once(':')
            && lhs.trim().eq_ignore_ascii_case("Mount Dir")
        {
            let val = rhs.trim();
            if !val.is_empty() {
                dirs.push(val.to_string());
            }
        }
    }
    dirs
}

async fn query_dism_mounted_dirs(dism_path: &Path, log_path: &Path) -> Option<HashSet<String>> {
    let args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        "/Get-MountedImageInfo".to_string(),
    ];
    let out = run_tracked_command(&dism_path.to_string_lossy(), &args, None, None, |_| {})
        .await
        .ok()?;
    if out.exit_code != 0 {
        return None;
    }
    let set = parse_dism_mounted_dirs(&out.stdout)
        .into_iter()
        .map(|d| d.trim_end_matches(['\\', '/']).to_ascii_lowercase())
        .collect();
    Some(set)
}

pub async fn recover_workspace_async<F>(
    dism_path: &Path,
    workspace_dir: &Path,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(&str) + Send,
{
    let (state, is_corrupted) = read_workspace_state_status(workspace_dir);
    if is_corrupted {
        on_log(
            "Detected corrupted workspace_state.json; running DISM cleanup and resetting state...",
        );
    }
    let log_path = workspace_dir.join("dism.log");
    let mut remaining_hives = Vec::new();
    let mut remaining_mounts = Vec::new();
    let mut errors = Vec::new();

    // Unload any stale offline registry hives strictly BEFORE discarding WIM mounts
    for hive_name in &state.loaded_hives {
        on_log(&format!("Unloading stale registry hive: HKLM\\{hive_name}"));
        if let Err(err) = crate::registry::unload_hive_by_name(hive_name) {
            let msg = format!("Failed unloading hive HKLM\\{hive_name}: {err}");
            on_log(&msg);
            errors.push(msg);
            remaining_hives.push(hive_name.clone());
        }
    }

    let live_mounts = query_dism_mounted_dirs(dism_path, &log_path).await;

    for mount in &state.active_mounts {
        let mount_path = PathBuf::from(mount);
        if !is_path_within_dir(&mount_path, workspace_dir) {
            let msg =
                format!("Refusing to discard stale mount outside workspace directory: '{mount}'");
            on_log(&msg);
            errors.push(msg);
            remaining_mounts.push(mount.clone());
            continue;
        }
        let norm_mount = mount.trim_end_matches(['\\', '/']).to_ascii_lowercase();
        if let Some(ref live) = live_mounts
            && !live.contains(&norm_mount)
        {
            on_log(&format!(
                "Stale mount entry '{mount}' is no longer mounted in DISM; clearing state."
            ));
            continue;
        }
        if mount_path.exists() {
            on_log(&format!("Discarding stale WIM mount: {mount}"));
            let args = vec![
                "/English".to_string(),
                format!("/LogPath:{}", log_path.display()),
                "/Unmount-Image".to_string(),
                format!("/MountDir:{mount}"),
                "/Discard".to_string(),
            ];
            match run_tracked_command(&dism_path.to_string_lossy(), &args, None, None, |l| {
                if !l.trim().is_empty() {
                    on_log(l);
                }
            })
            .await
            {
                Ok(out) if out.exit_code == 0 => {}
                Ok(out) => {
                    let msg = format!(
                        "DISM /Unmount-Image /Discard failed for '{mount}' (code {})",
                        out.exit_code
                    );
                    on_log(&msg);
                    errors.push(msg);
                    remaining_mounts.push(mount.clone());
                }
                Err(err) => {
                    let msg = format!("DISM /Unmount-Image /Discard failed for '{mount}': {err}");
                    on_log(&msg);
                    errors.push(msg);
                    remaining_mounts.push(mount.clone());
                }
            }
        }
    }

    on_log("Running DISM /Cleanup-Wim...");
    let cleanup_args = vec![
        "/English".to_string(),
        format!("/LogPath:{}", log_path.display()),
        "/Cleanup-Wim".to_string(),
    ];
    let cleanup_ok = match run_tracked_command(
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
    .await
    {
        Ok(out) if out.exit_code == 0 => true,
        Ok(out) => {
            let msg = format!("DISM /Cleanup-Wim failed (code {})", out.exit_code);
            on_log(&msg);
            errors.push(msg);
            false
        }
        Err(err) => {
            let msg = format!("DISM /Cleanup-Wim failed: {err}");
            on_log(&msg);
            errors.push(msg);
            false
        }
    };

    if cleanup_ok
        && !remaining_mounts.is_empty()
        && let Some(after_mounts) = query_dism_mounted_dirs(dism_path, &log_path).await
    {
        let before_len = remaining_mounts.len();
        remaining_mounts.retain(|m| {
            let norm = m.trim_end_matches(['\\', '/']).to_ascii_lowercase();
            !is_path_within_dir(Path::new(m), workspace_dir) || after_mounts.contains(&norm)
        });
        if remaining_mounts.len() < before_len && remaining_mounts.is_empty() {
            errors.retain(|e| !e.contains("DISM /Unmount-Image /Discard failed"));
        }
    }

    let next_state = WorkspaceState {
        active_mounts: remaining_mounts,
        loaded_hives: remaining_hives,
        updated_at: Local::now().to_rfc3339(),
    };
    {
        let _lock = WORKSPACE_STATE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        write_workspace_state(workspace_dir, &next_state)?;
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Workspace recovery completed with errors: {}",
            errors.join("; ")
        ))
    }
}

pub fn is_swm_part_for_stem(file_name: &str, stem: &str) -> bool {
    let lower_name = file_name.to_ascii_lowercase();
    let lower_stem = stem.to_ascii_lowercase();
    let Some(rest) = lower_name.strip_prefix(&lower_stem) else {
        return false;
    };
    let Some(num_part) = rest.strip_suffix(".swm") else {
        return false;
    };
    num_part.is_empty() || num_part.chars().all(|c| c.is_ascii_digit())
}

/// Packages a single-index `install.wim` into `dest_dir` with standard `install.{wim,esd}` naming
/// (used for `iso_root\sources`), removing stale `install.*` artifacts in `dest_dir` first.
#[allow(clippy::too_many_arguments)]
pub async fn package_install_image<F>(
    wimlib_path: &Path,
    src_wim: &Path,
    dest_dir: &Path,
    format: &InstallOutputFormat,
    skip_recompress_wim: bool,
    guard: Option<&ProcessGuard>,
    on_log: F,
) -> Result<Vec<PathBuf>, String>
where
    F: FnMut(&str) + Send,
{
    let target_file = match format {
        InstallOutputFormat::Wim => dest_dir.join("install.wim"),
        InstallOutputFormat::Esd => dest_dir.join("install.esd"),
    };
    package_install_image_impl(
        wimlib_path,
        src_wim,
        &target_file,
        format,
        true,
        skip_recompress_wim,
        guard,
        on_log,
    )
    .await
}

/// Packages a single-index `install.wim` directly to `target_file` (used for standalone image export)
/// without removing unrelated `install.*` files in the destination directory.
#[allow(clippy::too_many_arguments)]
pub async fn package_install_image_to_path<F>(
    wimlib_path: &Path,
    src_wim: &Path,
    target_file: &Path,
    format: &InstallOutputFormat,
    skip_recompress_wim: bool,
    guard: Option<&ProcessGuard>,
    on_log: F,
) -> Result<Vec<PathBuf>, String>
where
    F: FnMut(&str) + Send,
{
    package_install_image_impl(
        wimlib_path,
        src_wim,
        target_file,
        format,
        false,
        skip_recompress_wim,
        guard,
        on_log,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn package_install_image_impl<F>(
    wimlib_path: &Path,
    src_wim: &Path,
    target_file: &Path,
    format: &InstallOutputFormat,
    clean_all_install_artifacts: bool,
    skip_recompress_wim: bool,
    guard: Option<&ProcessGuard>,
    mut on_log: F,
) -> Result<Vec<PathBuf>, String>
where
    F: FnMut(&str) + Send,
{
    let dest_dir = target_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    tokio::fs::create_dir_all(dest_dir).await.map_err(|e| {
        format!(
            "Failed creating destination dir '{}': {e}",
            dest_dir.display()
        )
    })?;

    if clean_all_install_artifacts {
        // Clean any pre-existing install.* artifacts in dest_dir (when packaging inside iso_root\sources)
        if let Ok(mut entries) = tokio::fs::read_dir(dest_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                let is_install_art = name == "install.wim"
                    || name == "install.esd"
                    || is_swm_part_for_stem(&name, "install");
                if is_install_art && entry.path() != src_wim {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
    } else if target_file.exists() && target_file != src_wim {
        let _ = tokio::fs::remove_file(target_file).await;
    }

    match format {
        InstallOutputFormat::Wim => {
            if src_wim != target_file {
                if skip_recompress_wim {
                    on_log(&format!(
                        "Moving unmodified single-edition WIM image to '{}'...",
                        target_file.display()
                    ));
                    if tokio::fs::rename(src_wim, target_file).await.is_err() {
                        let s = src_wim.to_path_buf();
                        let t = target_file.to_path_buf();
                        let cancel_flag = guard.map(|g| g.cancel_handle());
                        tokio::task::spawn_blocking(move || {
                            copy_file_cancellable(&s, &t, || {
                                cancel_flag
                                    .as_ref()
                                    .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
                            })
                        })
                        .await
                        .map_err(|e| format!("WIM copy task failed: {e}"))??;
                    }
                } else {
                    on_log(&format!(
                        "Optimizing/exporting WIM image to '{}'...",
                        target_file.display()
                    ));
                    let args = vec![
                        "export".to_string(),
                        src_wim.to_string_lossy().to_string(),
                        "1".to_string(),
                        target_file.to_string_lossy().to_string(),
                        "--compress=LZX".to_string(),
                    ];
                    run_wimlib_command(wimlib_path, &args, guard, |l| {
                        if !l.trim().is_empty() {
                            on_log(l);
                        }
                    })
                    .await?;
                }
            }
            Ok(vec![target_file.to_path_buf()])
        }
        InstallOutputFormat::Esd => {
            on_log(&format!(
                "Compressing to ESD (LZMS solid) at '{}'...",
                target_file.display()
            ));
            let args = vec![
                "export".to_string(),
                src_wim.to_string_lossy().to_string(),
                "1".to_string(),
                target_file.to_string_lossy().to_string(),
                "--compress=LZMS".to_string(),
                "--solid".to_string(),
            ];
            run_wimlib_command(wimlib_path, &args, guard, |l| {
                if !l.trim().is_empty() {
                    on_log(l);
                }
            })
            .await?;
            Ok(vec![target_file.to_path_buf()])
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
        assert_eq!(parse_dism_progress("Прогресс45.5%"), Some(45.5));
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

    #[test]
    fn matches_swm_parts_by_stem_strictly() {
        assert!(is_swm_part_for_stem("install.swm", "install"));
        assert!(is_swm_part_for_stem("install2.swm", "install"));
        assert!(is_swm_part_for_stem("Win11Pro12.SWM", "Win11Pro"));
        assert!(!is_swm_part_for_stem("install_backup.swm", "install"));
        assert!(!is_swm_part_for_stem("install.wim", "install"));
        assert!(!is_swm_part_for_stem("other2.swm", "install"));
    }

    #[test]
    fn parses_dism_mounted_dirs_and_detects_corrupted_state() {
        let dism_out = "\
Deployment Image Servicing and Management tool\r\n\
Mounted images:\r\n\
\r\n\
Mount Dir : C:\\ISOCraft\\work\\session_current\\mount_install\r\n\
Image File : C:\\ISOCraft\\work\\session_current\\work\\install.wim\r\n\
Image Index : 1\r\n\
Mounted Read/Write : Yes\r\n\
Status : Ok\r\n\
\r\n\
Mount Dir : D:\\Temp\\mount_boot\r\n\
Status : Needs Remount\r\n";
        let dirs = parse_dism_mounted_dirs(dism_out);
        assert_eq!(
            dirs,
            vec![
                r"C:\ISOCraft\work\session_current\mount_install",
                r"D:\Temp\mount_boot"
            ]
        );

        let dir = std::env::temp_dir().join(format!("isocraft_corrupt_ws_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("workspace_state.json"), b"{broken json").unwrap();
        let (st, corrupted) = read_workspace_state_status(&dir);
        assert!(corrupted);
        assert!(st.active_mounts.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
