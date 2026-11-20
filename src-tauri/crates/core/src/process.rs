use std::collections::HashSet;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct ProcessGuard {
    active_operation: AtomicBool,
    cancel_requested: AtomicBool,
    tracked_pids: Mutex<HashSet<u32>>,
}

impl Default for ProcessGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessGuard {
    pub fn new() -> Self {
        Self {
            active_operation: AtomicBool::new(false),
            cancel_requested: AtomicBool::new(false),
            tracked_pids: Mutex::new(HashSet::new()),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.active_operation.load(Ordering::SeqCst)
    }

    pub fn set_busy(&self, busy: bool) {
        self.active_operation.store(busy, Ordering::SeqCst);
        if !busy {
            self.cancel_requested.store(false, Ordering::SeqCst);
        }
    }

    pub fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    pub fn register_pid(&self, pid: u32) {
        if let Ok(mut guard) = self.tracked_pids.lock() {
            guard.insert(pid);
        }
    }

    pub fn unregister_pid(&self, pid: u32) {
        if let Ok(mut guard) = self.tracked_pids.lock() {
            guard.remove(&pid);
        }
    }

    pub fn kill_all_tracked_trees(&self) {
        self.request_cancel();
        let pids: Vec<u32> = self
            .tracked_pids
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default();

        for pid in pids {
            kill_process_tree(pid);
        }

        if let Ok(mut guard) = self.tracked_pids.lock() {
            guard.clear();
        }
        self.set_busy(false);
    }
}

pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub async fn run_tracked_command<F>(
    program: &str,
    args: &[String],
    current_dir: Option<&Path>,
    guard: Option<&ProcessGuard>,
    mut on_line: F,
) -> Result<CommandOutput, String>
where
    F: FnMut(&str) + Send,
{
    if let Some(g) = guard {
        if g.is_cancel_requested() {
            return Err("Operation cancelled by user.".to_string());
        }
    }

    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn '{program}': {e}"))?;

    let child_pid = child.id();
    if let (Some(g), Some(pid)) = (guard, child_pid) {
        g.register_pid(pid);
    }

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(bool, String)>();

    if let Some(out) = stdout_handle {
        let tx_out = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(out).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = tx_out.send((false, line));
            }
        });
    }

    if let Some(err) = stderr_handle {
        let tx_err = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(err).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = tx_err.send((true, line));
            }
        });
    }
    drop(tx);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

    while let Some((is_err, line)) = rx.recv().await {
        on_line(&line);
        if is_err {
            stderr_buf.push_str(&line);
            stderr_buf.push('\n');
        } else {
            stdout_buf.push_str(&line);
            stdout_buf.push('\n');
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed waiting for '{program}': {e}"))?;

    if let (Some(g), Some(pid)) = (guard, child_pid) {
        g.unregister_pid(pid);
    }

    if let Some(g) = guard {
        if g.is_cancel_requested() {
            return Err("Operation cancelled by user.".to_string());
        }
    }

    Ok(CommandOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout: stdout_buf,
        stderr: stderr_buf,
    })
}

#[cfg(windows)]
pub fn kill_process_tree(root_pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    let mut to_kill = vec![root_pid];
    unsafe {
        if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut entries = Vec::new();
            let mut pe = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snapshot, &mut pe).is_ok() {
                loop {
                    entries.push((pe.th32ProcessID, pe.th32ParentProcessID));
                    if Process32NextW(snapshot, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);

            let mut idx = 0;
            while idx < to_kill.len() {
                let parent = to_kill[idx];
                for &(child, p) in &entries {
                    if p == parent && !to_kill.contains(&child) {
                        to_kill.push(child);
                    }
                }
                idx += 1;
            }
        }

        // Terminate children before parent
        for &pid in to_kill.iter().rev() {
            if let Ok(proc_handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(proc_handle, 1);
                let _ = CloseHandle(proc_handle);
            }
        }
    }
}

#[cfg(not(windows))]
pub fn kill_process_tree(_root_pid: u32) {}
