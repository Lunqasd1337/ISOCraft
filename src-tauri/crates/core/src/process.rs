use std::collections::HashSet;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::Notify;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_STDOUT_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_STDERR_CAPTURE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Generic,
    ImageInspect,
    CatalogLoad,
    CatalogDownload,
    DriverExport,
    Build,
    Recovery,
    StorageClear,
    AppUpdate,
}

#[cfg(windows)]
mod job_impl {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    pub struct OwnedJobObject(HANDLE);

    unsafe impl Send for OwnedJobObject {}
    unsafe impl Sync for OwnedJobObject {}

    impl Drop for OwnedJobObject {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    impl OwnedJobObject {
        pub fn create_kill_on_close() -> Option<Self> {
            unsafe {
                let Ok(job) = CreateJobObjectW(None, None) else {
                    return None;
                };
                if job.is_invalid() {
                    return None;
                }
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let res = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if res.is_err() {
                    let _ = CloseHandle(job);
                    return None;
                }
                Some(Self(job))
            }
        }

        pub fn assign_pid(&self, pid: u32) -> bool {
            unsafe {
                let Ok(proc_handle) =
                    OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)
                else {
                    return false;
                };
                let ok = AssignProcessToJobObject(self.0, proc_handle).is_ok();
                let _ = CloseHandle(proc_handle);
                ok
            }
        }

        pub fn terminate(&self) {
            unsafe {
                let _ = TerminateJobObject(self.0, 1);
            }
        }
    }
}

pub struct ProcessGuard {
    active_operation: AtomicBool,
    generation: AtomicU64,
    active_kind: Mutex<Option<OperationKind>>,
    current_cancel: Mutex<Arc<AtomicBool>>,
    tracked_pids: Mutex<HashSet<u32>>,
    #[cfg(windows)]
    active_job: Mutex<Option<job_impl::OwnedJobObject>>,
    idle_notify: Notify,
}

pub struct BusyScope<'a> {
    guard: &'a ProcessGuard,
    generation: u64,
}

impl<'a> Drop for BusyScope<'a> {
    fn drop(&mut self) {
        self.guard.exit_busy(self.generation);
    }
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
            generation: AtomicU64::new(0),
            active_kind: Mutex::new(None),
            current_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            tracked_pids: Mutex::new(HashSet::new()),
            #[cfg(windows)]
            active_job: Mutex::new(None),
            idle_notify: Notify::new(),
        }
    }

    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        let guard = self
            .current_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Arc::clone(&guard)
    }

    pub fn is_busy(&self) -> bool {
        self.active_operation.load(Ordering::SeqCst)
    }

    pub fn active_operation_kind(&self) -> Option<OperationKind> {
        *self.active_kind.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn enter_busy(&self) -> BusyScope<'_> {
        self.active_operation.store(true, Ordering::SeqCst);
        let generation = self.begin_generation(OperationKind::Generic);
        BusyScope {
            guard: self,
            generation,
        }
    }

    pub fn try_enter_busy(&self) -> Option<BusyScope<'_>> {
        self.try_enter_busy_for(OperationKind::Generic)
    }

    pub fn try_enter_busy_for(&self, kind: OperationKind) -> Option<BusyScope<'_>> {
        if self
            .active_operation
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let generation = self.begin_generation(kind);
            Some(BusyScope {
                guard: self,
                generation,
            })
        } else {
            None
        }
    }

    fn begin_generation(&self, kind: OperationKind) -> u64 {
        let next_cancel = Arc::new(AtomicBool::new(false));
        {
            let mut slot = self
                .current_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *slot = next_cancel;
        }
        {
            let mut kslot = self.active_kind.lock().unwrap_or_else(|e| e.into_inner());
            *kslot = Some(kind);
        }
        #[cfg(windows)]
        {
            let mut jslot = self.active_job.lock().unwrap_or_else(|e| e.into_inner());
            *jslot = job_impl::OwnedJobObject::create_kill_on_close();
        }
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn exit_busy(&self, expected_gen: u64) {
        if self.generation.load(Ordering::SeqCst) != expected_gen {
            return;
        }
        {
            let mut slot = self
                .current_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *slot = Arc::new(AtomicBool::new(false));
        }
        {
            let mut kslot = self.active_kind.lock().unwrap_or_else(|e| e.into_inner());
            *kslot = None;
        }
        #[cfg(windows)]
        {
            let mut jslot = self.active_job.lock().unwrap_or_else(|e| e.into_inner());
            *jslot = None;
        }
        self.active_operation.store(false, Ordering::SeqCst);
        self.idle_notify.notify_waiters();
    }

    pub async fn wait_until_idle(&self) {
        loop {
            let notified = self.idle_notify.notified();
            if !self.is_busy() {
                return;
            }
            notified.await;
        }
    }

    pub fn request_cancel(&self) {
        self.cancel_handle().store(true, Ordering::SeqCst);
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_handle().load(Ordering::SeqCst)
    }

    pub fn register_pid(&self, pid: u32) {
        #[cfg(windows)]
        let assigned_to_job = {
            let jslot = self.active_job.lock().unwrap_or_else(|e| e.into_inner());
            jslot.as_ref().is_some_and(|job| job.assign_pid(pid))
        };
        #[cfg(not(windows))]
        let assigned_to_job = false;

        if !assigned_to_job && let Ok(mut guard) = self.tracked_pids.lock() {
            guard.insert(pid);
        }
    }

    pub fn unregister_pid(&self, pid: u32) {
        if let Ok(mut guard) = self.tracked_pids.lock() {
            guard.remove(&pid);
        }
    }

    /// Cancels and terminates tracked processes only if the active operation matches `expected`.
    pub fn cancel_operation_kind(&self, expected: OperationKind) -> bool {
        if !self.is_busy() || self.active_operation_kind() != Some(expected) {
            return false;
        }
        self.kill_all_tracked_trees();
        true
    }

    /// Signals cancellation and terminates all tracked child process trees.
    /// Does NOT clear `active_operation` so the owning operation can finish its
    /// unmount/cleanup path before marking the guard idle.
    pub fn kill_all_tracked_trees(&self) {
        self.request_cancel();
        #[cfg(windows)]
        {
            let jslot = self.active_job.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref job) = *jslot {
                job.terminate();
            }
        }
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
    on_line: F,
) -> Result<CommandOutput, String>
where
    F: FnMut(&str) + Send,
{
    run_tracked_command_with_env(program, args, current_dir, &[], guard, on_line).await
}

async fn stream_lossy_lines<R>(
    mut reader: R,
    is_err: bool,
    tx: tokio::sync::mpsc::UnboundedSender<(bool, String)>,
) where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 8192];
    let mut line_buf: Vec<u8> = Vec::new();
    let mut prev_cr = false;

    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => {
                if !line_buf.is_empty() {
                    let line = String::from_utf8_lossy(&line_buf).into_owned();
                    let _ = tx.send((is_err, line));
                }
                break;
            }
            Ok(n) => {
                for &b in &chunk[..n] {
                    if b == b'\n' {
                        if prev_cr && line_buf.is_empty() {
                            prev_cr = false;
                            continue;
                        }
                        prev_cr = false;
                        let line = String::from_utf8_lossy(&line_buf).into_owned();
                        line_buf.clear();
                        let _ = tx.send((is_err, line));
                    } else if b == b'\r' {
                        prev_cr = true;
                        let line = String::from_utf8_lossy(&line_buf).into_owned();
                        line_buf.clear();
                        let _ = tx.send((is_err, line));
                    } else {
                        prev_cr = false;
                        if line_buf.len() < MAX_LINE_BYTES {
                            line_buf.push(b);
                        } else {
                            let line = String::from_utf8_lossy(&line_buf).into_owned();
                            line_buf.clear();
                            line_buf.push(b);
                            let _ = tx.send((is_err, line));
                        }
                    }
                }
            }
            Err(_) => {
                if !line_buf.is_empty() {
                    let line = String::from_utf8_lossy(&line_buf).into_owned();
                    let _ = tx.send((is_err, line));
                }
                break;
            }
        }
    }
}

fn push_capped_line(buf: &mut String, line: &str, max_bytes: usize) {
    if buf.len() >= max_bytes {
        return;
    }
    let remaining = max_bytes.saturating_sub(buf.len());
    if line.len() < remaining {
        buf.push_str(line);
        buf.push('\n');
    } else if remaining > 1 {
        let safe_end = line.floor_char_boundary(remaining - 1);
        buf.push_str(&line[..safe_end]);
        buf.push('\n');
    }
}

pub async fn run_tracked_command_with_env<F>(
    program: &str,
    args: &[String],
    current_dir: Option<&Path>,
    envs: &[(&str, &std::ffi::OsStr)],
    guard: Option<&ProcessGuard>,
    mut on_line: F,
) -> Result<CommandOutput, String>
where
    F: FnMut(&str) + Send,
{
    let cancel_handle = guard.map(|g| g.cancel_handle());
    if cancel_handle
        .as_ref()
        .is_some_and(|c| c.load(Ordering::SeqCst))
    {
        return Err("Operation cancelled by user.".to_string());
    }

    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }

    if !envs.is_empty() {
        cmd.envs(envs.iter().copied());
    }

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn '{program}': {e}"))?;

    let child_pid = child.id();
    if let (Some(g), Some(pid)) = (guard, child_pid) {
        g.register_pid(pid);
        if cancel_handle
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            kill_process_tree(pid);
            let _ = child.wait().await;
            g.unregister_pid(pid);
            return Err("Operation cancelled by user.".to_string());
        }
    }

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(bool, String)>();

    if let Some(out) = stdout_handle {
        tokio::spawn(stream_lossy_lines(out, false, tx.clone()));
    }

    if let Some(err) = stderr_handle {
        tokio::spawn(stream_lossy_lines(err, true, tx.clone()));
    }
    drop(tx);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

    while let Some((is_err, line)) = rx.recv().await {
        on_line(&line);
        if is_err {
            push_capped_line(&mut stderr_buf, &line, MAX_STDERR_CAPTURE_BYTES);
        } else {
            push_capped_line(&mut stdout_buf, &line, MAX_STDOUT_CAPTURE_BYTES);
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed waiting for '{program}': {e}"))?;

    if let (Some(g), Some(pid)) = (guard, child_pid) {
        g.unregister_pid(pid);
    }

    if cancel_handle
        .as_ref()
        .is_some_and(|c| c.load(Ordering::SeqCst))
    {
        return Err("Operation cancelled by user.".to_string());
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
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn kill_keeps_busy_until_scope_drops_and_notifies_waiters() {
        let guard = Arc::new(ProcessGuard::new());
        let scope = guard.enter_busy();
        assert!(guard.is_busy());
        assert!(!guard.is_cancel_requested());

        guard.kill_all_tracked_trees();
        assert!(guard.is_busy());
        assert!(guard.is_cancel_requested());

        let waiter_guard = Arc::clone(&guard);
        let waiter = tokio::spawn(async move {
            waiter_guard.wait_until_idle().await;
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished());

        drop(scope);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("wait_until_idle timed out")
            .unwrap();
        assert!(!guard.is_busy());
        assert!(!guard.is_cancel_requested());
    }

    #[test]
    fn try_enter_busy_prevents_concurrent_entry() {
        let guard = ProcessGuard::new();
        let scope = guard.try_enter_busy().expect("first entry must succeed");
        assert!(guard.is_busy());
        assert!(guard.try_enter_busy().is_none());

        drop(scope);
        assert!(!guard.is_busy());
        assert!(guard.try_enter_busy().is_some());
    }

    #[test]
    fn cancel_handle_is_isolated_across_generations() {
        let guard = ProcessGuard::new();
        let scope1 = guard.try_enter_busy().unwrap();
        let h1 = guard.cancel_handle();
        guard.request_cancel();
        assert!(h1.load(Ordering::SeqCst));
        drop(scope1);

        let scope2 = guard.try_enter_busy().unwrap();
        let h2 = guard.cancel_handle();
        assert!(h1.load(Ordering::SeqCst));
        assert!(!h2.load(Ordering::SeqCst));
        drop(scope2);
    }

    #[tokio::test]
    async fn stream_lossy_lines_preserves_non_utf8_output() {
        let payload: &[u8] = b"hello\r\n\xff\xfe_bad_utf8\nlast";
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        stream_lossy_lines(payload, false, tx).await;

        let mut lines = Vec::new();
        while let Some((_, line)) = rx.recv().await {
            lines.push(line);
        }
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "hello");
        assert!(lines[1].ends_with("_bad_utf8"));
        assert_eq!(lines[2], "last");
    }

    #[tokio::test]
    async fn stream_lossy_lines_splits_carriage_return_progress_updates() {
        let payload: &[u8] = b"[== 10.0% ==]\r[== 50.0% ==]\r\nDone";
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        stream_lossy_lines(payload, false, tx).await;

        let mut lines = Vec::new();
        while let Some((_, line)) = rx.recv().await {
            lines.push(line);
        }
        assert_eq!(lines, vec!["[== 10.0% ==]", "[== 50.0% ==]", "Done"]);
    }

    #[test]
    fn operation_kind_scopes_cancellation_to_matching_operation() {
        let guard = ProcessGuard::new();
        let scope = guard.try_enter_busy_for(OperationKind::Build).unwrap();
        assert_eq!(guard.active_operation_kind(), Some(OperationKind::Build));
        assert!(!guard.cancel_operation_kind(OperationKind::CatalogDownload));
        assert!(!guard.is_cancel_requested());

        assert!(guard.cancel_operation_kind(OperationKind::Build));
        assert!(guard.is_cancel_requested());
        drop(scope);
        assert_eq!(guard.active_operation_kind(), None);
    }
}
