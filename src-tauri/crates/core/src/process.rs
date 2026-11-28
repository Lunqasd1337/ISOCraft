use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_STDOUT_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_STDERR_CAPTURE_BYTES: usize = 2 * 1024 * 1024;
const STREAM_CHANNEL_CAPACITY: usize = 256;
const CANCELLED_ERR: &str = "Operation cancelled by user.";
const TERMINATE_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

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

        pub fn terminate(&self) -> bool {
            unsafe { TerminateJobObject(self.0, 1).is_ok() }
        }
    }
}

struct GuardState {
    active: bool,
    generation: u64,
    active_kind: Option<OperationKind>,
    current_cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    tracked_pids: HashMap<u32, bool>,
    #[cfg(windows)]
    active_job: Option<Arc<job_impl::OwnedJobObject>>,
}

struct KillPlan {
    #[cfg(windows)]
    job: Option<Arc<job_impl::OwnedJobObject>>,
    job_pids: Vec<u32>,
    fallback_pids: Vec<u32>,
}

impl KillPlan {
    fn pids_to_kill(&self, job_terminated: bool) -> Vec<u32> {
        let mut pids = self.fallback_pids.clone();
        if !job_terminated {
            pids.extend(self.job_pids.iter().copied());
        }
        pids
    }
}

impl GuardState {
    fn new() -> Self {
        Self {
            active: false,
            generation: 0,
            active_kind: None,
            current_cancel: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(Notify::new()),
            tracked_pids: HashMap::new(),
            #[cfg(windows)]
            active_job: None,
        }
    }

    fn prepare_kill(&mut self) -> KillPlan {
        self.current_cancel.store(true, Ordering::SeqCst);
        self.cancel_notify.notify_waiters();
        let mut job_pids = Vec::new();
        let mut fallback_pids = Vec::new();
        for (pid, assigned_to_job) in self.tracked_pids.drain() {
            if assigned_to_job {
                job_pids.push(pid);
            } else {
                fallback_pids.push(pid);
            }
        }
        KillPlan {
            #[cfg(windows)]
            job: self.active_job.clone(),
            job_pids,
            fallback_pids,
        }
    }
}

pub struct ProcessGuard {
    state: Mutex<GuardState>,
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
            state: Mutex::new(GuardState::new()),
            idle_notify: Notify::new(),
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, GuardState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.lock_state().current_cancel)
    }

    fn cancel_signal(&self) -> (Arc<AtomicBool>, Arc<Notify>) {
        let state = self.lock_state();
        (
            Arc::clone(&state.current_cancel),
            Arc::clone(&state.cancel_notify),
        )
    }

    pub fn is_busy(&self) -> bool {
        self.lock_state().active
    }

    pub fn active_operation_kind(&self) -> Option<OperationKind> {
        self.lock_state().active_kind
    }

    pub fn try_enter_busy(&self) -> Option<BusyScope<'_>> {
        self.try_enter_busy_for(OperationKind::Generic)
    }

    pub fn try_enter_busy_for(&self, kind: OperationKind) -> Option<BusyScope<'_>> {
        let mut state = self.lock_state();
        if state.active {
            return None;
        }
        state.active = true;
        state.active_kind = Some(kind);
        state.current_cancel = Arc::new(AtomicBool::new(false));
        state.cancel_notify = Arc::new(Notify::new());
        state.tracked_pids.clear();
        #[cfg(windows)]
        {
            state.active_job = job_impl::OwnedJobObject::create_kill_on_close().map(Arc::new);
        }
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        Some(BusyScope {
            guard: self,
            generation,
        })
    }

    fn exit_busy(&self, expected_gen: u64) {
        #[cfg(windows)]
        let old_job;
        {
            let mut state = self.lock_state();
            if state.generation != expected_gen {
                return;
            }
            state.active = false;
            state.active_kind = None;
            state.current_cancel = Arc::new(AtomicBool::new(false));
            state.cancel_notify = Arc::new(Notify::new());
            state.tracked_pids.clear();
            #[cfg(windows)]
            {
                old_job = state.active_job.take();
            }
            self.idle_notify.notify_waiters();
        }
        #[cfg(windows)]
        drop(old_job);
    }

    pub async fn wait_until_idle(&self) {
        loop {
            let notified = self.idle_notify.notified();
            tokio::pin!(notified);
            let _ = notified.as_mut().enable();
            if !self.is_busy() {
                return;
            }
            notified.await;
        }
    }

    pub fn request_cancel(&self) {
        let state = self.lock_state();
        state.current_cancel.store(true, Ordering::SeqCst);
        state.cancel_notify.notify_waiters();
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.lock_state().current_cancel.load(Ordering::SeqCst)
    }

    pub fn register_pid(&self, pid: u32) {
        let mut state = self.lock_state();
        #[cfg(windows)]
        let assigned_to_job = state
            .active_job
            .as_ref()
            .is_some_and(|job| job.assign_pid(pid));
        #[cfg(not(windows))]
        let assigned_to_job = false;

        state.tracked_pids.insert(pid, assigned_to_job);
    }

    pub fn unregister_pid(&self, pid: u32) {
        self.lock_state().tracked_pids.remove(&pid);
    }

    fn execute_kill_plan(plan: KillPlan) {
        #[cfg(windows)]
        let job_terminated = plan.job.as_ref().is_some_and(|job| job.terminate());
        #[cfg(not(windows))]
        let job_terminated = false;

        for pid in plan.pids_to_kill(job_terminated) {
            kill_process_tree(pid);
        }
    }

    /// Cancels and terminates tracked processes only if the active operation matches `expected`.
    pub fn cancel_operation_kind(&self, expected: OperationKind) -> bool {
        let plan = {
            let mut state = self.lock_state();
            if !state.active || state.active_kind != Some(expected) {
                return false;
            }
            state.prepare_kill()
        };
        Self::execute_kill_plan(plan);
        true
    }

    /// Signals cancellation and terminates all tracked child process trees.
    /// Does NOT clear `active` so the owning operation can finish its
    /// unmount/cleanup path before marking the guard idle.
    pub fn kill_all_tracked_trees(&self) {
        let plan = self.lock_state().prepare_kill();
        Self::execute_kill_plan(plan);
    }

    #[cfg(test)]
    fn tracked_pid_count(&self) -> usize {
        self.lock_state().tracked_pids.len()
    }
}

pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct RawCommandOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

struct PidRegistrationGuard<'a> {
    guard: Option<&'a ProcessGuard>,
    pid: Option<u32>,
}

impl<'a> PidRegistrationGuard<'a> {
    fn new(guard: Option<&'a ProcessGuard>, pid: Option<u32>) -> Self {
        if let (Some(g), Some(p)) = (guard, pid) {
            g.register_pid(p);
        }
        Self { guard, pid }
    }
}

impl Drop for PidRegistrationGuard<'_> {
    fn drop(&mut self) {
        if let (Some(g), Some(p)) = (self.guard, self.pid) {
            g.unregister_pid(p);
        }
    }
}

struct TaskAbortGuard(Vec<JoinHandle<()>>);

impl Drop for TaskAbortGuard {
    fn drop(&mut self) {
        for handle in &self.0 {
            handle.abort();
        }
    }
}

struct SpawnedChild<'a> {
    // Declared before `child` so `unregister_pid` runs before `Child` drops its process handle.
    _pid_guard: PidRegistrationGuard<'a>,
    child: Child,
    child_pid: Option<u32>,
    cancel_signal: Option<(Arc<AtomicBool>, Arc<Notify>)>,
}

async fn wait_for_cancel(signal: Option<&(Arc<AtomicBool>, Arc<Notify>)>) {
    let Some((flag, notify)) = signal else {
        std::future::pending::<()>().await;
        return;
    };

    loop {
        let notified = notify.notified();
        tokio::pin!(notified);
        let _ = notified.as_mut().enable();
        if flag.load(Ordering::SeqCst) {
            return;
        }
        notified.await;
    }
}

fn is_cancelled(signal: Option<&(Arc<AtomicBool>, Arc<Notify>)>) -> bool {
    signal.is_some_and(|(flag, _)| flag.load(Ordering::SeqCst))
}

async fn terminate_child(child: &mut Child, child_pid: Option<u32>) {
    if let Some(pid) = child_pid {
        kill_process_tree(pid);
    }
    let _ = child.start_kill();
    let _ = tokio::time::timeout(TERMINATE_WAIT_TIMEOUT, child.wait()).await;
}

async fn spawn_tracked_child<'a>(
    program: &str,
    args: &[String],
    current_dir: Option<&Path>,
    envs: &[(&str, &std::ffi::OsStr)],
    guard: Option<&'a ProcessGuard>,
) -> Result<SpawnedChild<'a>, String> {
    let cancel_signal = guard.map(|g| g.cancel_signal());
    if is_cancelled(cancel_signal.as_ref()) {
        return Err(CANCELLED_ERR.to_string());
    }

    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true);

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
    let pid_guard = PidRegistrationGuard::new(guard, child_pid);

    if is_cancelled(cancel_signal.as_ref()) {
        terminate_child(&mut child, child_pid).await;
        return Err(CANCELLED_ERR.to_string());
    }

    Ok(SpawnedChild {
        _pid_guard: pid_guard,
        child,
        child_pid,
        cancel_signal,
    })
}

async fn wait_child_cancellable(
    child: &mut Child,
    child_pid: Option<u32>,
    program: &str,
    cancel_signal: Option<&(Arc<AtomicBool>, Arc<Notify>)>,
) -> Result<std::process::ExitStatus, String> {
    let status = tokio::select! {
        biased;
        _ = wait_for_cancel(cancel_signal) => {
            terminate_child(child, child_pid).await;
            return Err(CANCELLED_ERR.to_string());
        }
        res = child.wait() => {
            res.map_err(|e| format!("Failed waiting for '{program}': {e}"))?
        }
    };

    if is_cancelled(cancel_signal) {
        return Err(CANCELLED_ERR.to_string());
    }

    Ok(status)
}

fn utf8_safe_split_len(buf: &[u8]) -> usize {
    let len = buf.len();
    let lookback = len.min(4);
    for offset in 1..=lookback {
        let idx = len - offset;
        let b = buf[idx];
        if b < 0x80 {
            return len;
        }
        if (b & 0xC0) == 0xC0 {
            let expected_len = match b {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => 1,
            };
            if offset < expected_len && idx > 0 {
                return idx;
            }
            return len;
        }
    }
    len
}

async fn stream_lossy_lines<R>(mut reader: R, is_err: bool, tx: mpsc::Sender<(bool, String)>)
where
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
                    let _ = tx.send((is_err, line)).await;
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
                        if tx.send((is_err, line)).await.is_err() {
                            return;
                        }
                    } else if b == b'\r' {
                        prev_cr = true;
                        let line = String::from_utf8_lossy(&line_buf).into_owned();
                        line_buf.clear();
                        if tx.send((is_err, line)).await.is_err() {
                            return;
                        }
                    } else {
                        prev_cr = false;
                        if line_buf.len() < MAX_LINE_BYTES {
                            line_buf.push(b);
                        } else {
                            let split_at = utf8_safe_split_len(&line_buf);
                            let line = String::from_utf8_lossy(&line_buf[..split_at]).into_owned();
                            line_buf.drain(..split_at);
                            line_buf.push(b);
                            if tx.send((is_err, line)).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                if !line_buf.is_empty() {
                    let line = String::from_utf8_lossy(&line_buf).into_owned();
                    let _ = tx.send((is_err, line)).await;
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

struct CappedBytes {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_capped_bytes<R>(mut reader: R, max_bytes: usize) -> CappedBytes
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 8192];
    let mut bytes = Vec::new();
    let mut truncated = false;

    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if bytes.len() < max_bytes {
                    let take = (max_bytes - bytes.len()).min(n);
                    bytes.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
        }
    }

    CappedBytes { bytes, truncated }
}

async fn read_optional_capped_bytes<R>(reader: Option<R>, max_bytes: usize) -> CappedBytes
where
    R: AsyncRead + Unpin,
{
    match reader {
        Some(r) => read_capped_bytes(r, max_bytes).await,
        None => CappedBytes {
            bytes: Vec::new(),
            truncated: false,
        },
    }
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
    let mut spawned = spawn_tracked_child(program, args, current_dir, envs, guard).await?;

    let stdout_handle = spawned.child.stdout.take();
    let stderr_handle = spawned.child.stderr.take();

    let (tx, mut rx) = mpsc::channel::<(bool, String)>(STREAM_CHANNEL_CAPACITY);
    let mut tasks = Vec::new();

    if let Some(out) = stdout_handle {
        tasks.push(tokio::spawn(stream_lossy_lines(out, false, tx.clone())));
    }
    if let Some(err) = stderr_handle {
        tasks.push(tokio::spawn(stream_lossy_lines(err, true, tx.clone())));
    }
    drop(tx);
    let reader_tasks = TaskAbortGuard(tasks);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

    {
        let cancel_fut = wait_for_cancel(spawned.cancel_signal.as_ref());
        tokio::pin!(cancel_fut);

        loop {
            tokio::select! {
                biased;
                _ = &mut cancel_fut => {
                    drop(rx);
                    drop(reader_tasks);
                    terminate_child(&mut spawned.child, spawned.child_pid).await;
                    return Err(CANCELLED_ERR.to_string());
                }
                item = rx.recv() => {
                    let Some((is_err, line)) = item else {
                        break;
                    };
                    on_line(&line);
                    if is_err {
                        push_capped_line(&mut stderr_buf, &line, MAX_STDERR_CAPTURE_BYTES);
                    } else {
                        push_capped_line(&mut stdout_buf, &line, MAX_STDOUT_CAPTURE_BYTES);
                    }
                }
            }
        }
    }

    let status = wait_child_cancellable(
        &mut spawned.child,
        spawned.child_pid,
        program,
        spawned.cancel_signal.as_ref(),
    )
    .await?;

    Ok(CommandOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout: stdout_buf,
        stderr: stderr_buf,
    })
}

pub async fn run_tracked_command_raw(
    program: &str,
    args: &[String],
    current_dir: Option<&Path>,
    guard: Option<&ProcessGuard>,
) -> Result<RawCommandOutput, String> {
    run_tracked_command_raw_with_env(program, args, current_dir, &[], guard).await
}

pub async fn run_tracked_command_raw_with_env(
    program: &str,
    args: &[String],
    current_dir: Option<&Path>,
    envs: &[(&str, &std::ffi::OsStr)],
    guard: Option<&ProcessGuard>,
) -> Result<RawCommandOutput, String> {
    let mut spawned = spawn_tracked_child(program, args, current_dir, envs, guard).await?;

    let stdout_handle = spawned.child.stdout.take();
    let stderr_handle = spawned.child.stderr.take();

    let read_result = {
        let cancel_fut = wait_for_cancel(spawned.cancel_signal.as_ref());
        tokio::pin!(cancel_fut);

        tokio::select! {
            biased;
            _ = &mut cancel_fut => None,
            res = async {
                tokio::join!(
                    read_optional_capped_bytes(stdout_handle, MAX_STDOUT_CAPTURE_BYTES),
                    read_optional_capped_bytes(stderr_handle, MAX_STDERR_CAPTURE_BYTES),
                )
            } => Some(res),
        }
    };

    let Some((stdout_res, stderr_res)) = read_result else {
        terminate_child(&mut spawned.child, spawned.child_pid).await;
        return Err(CANCELLED_ERR.to_string());
    };

    let status = wait_child_cancellable(
        &mut spawned.child,
        spawned.child_pid,
        program,
        spawned.cancel_signal.as_ref(),
    )
    .await?;

    Ok(RawCommandOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout: stdout_res.bytes,
        stderr: stderr_res.bytes,
        stdout_truncated: stdout_res.truncated,
        stderr_truncated: stderr_res.truncated,
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
        let scope = guard.try_enter_busy().unwrap();
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
        let (tx, mut rx) = mpsc::channel(64);
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
        let (tx, mut rx) = mpsc::channel(64);
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

    #[tokio::test]
    async fn stream_lossy_lines_preserves_utf8_across_max_line_boundary_and_stops_on_drop() {
        // Place a 2-byte Cyrillic character ('Я' = 0xD0 0xAF) straddling byte 65536 (MAX_LINE_BYTES).
        let mut payload = vec![b'a'; MAX_LINE_BYTES - 1];
        payload.extend_from_slice("Якорь".as_bytes());

        let (tx, mut rx) = mpsc::channel(16);
        stream_lossy_lines(payload.as_slice(), false, tx).await;

        let mut lines = Vec::new();
        while let Some((_, line)) = rx.recv().await {
            lines.push(line);
        }
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].contains('\u{FFFD}'));
        assert!(!lines[1].contains('\u{FFFD}'));
        assert_eq!(
            format!("{}{}", lines[0], lines[1]),
            String::from_utf8(payload).unwrap()
        );

        // Dropping receiver must stop stream_lossy_lines cleanly without blocking.
        let big = vec![b'x'; MAX_LINE_BYTES * 8];
        let (tx2, rx2) = mpsc::channel(1);
        drop(rx2);
        tokio::time::timeout(
            Duration::from_secs(1),
            stream_lossy_lines(big.as_slice(), false, tx2),
        )
        .await
        .expect("stream_lossy_lines did not stop after receiver drop");
    }

    #[tokio::test]
    async fn read_capped_bytes_preserves_large_single_line_utf8_json_over_64kib() {
        let item = "Пакет_Кириллица_Тест_1234567890";
        let repeated: Vec<&str> = std::iter::repeat_n(item, 3000).collect();
        let json = serde_json::to_string(&repeated).unwrap();
        assert!(json.len() > MAX_LINE_BYTES * 2);
        assert!(!json.contains('\n'));

        let res = read_capped_bytes(json.as_bytes(), MAX_STDOUT_CAPTURE_BYTES).await;
        assert!(!res.truncated);
        assert_eq!(res.bytes, json.as_bytes());
        let parsed: Vec<String> = serde_json::from_slice(&res.bytes).unwrap();
        assert_eq!(parsed.len(), 3000);
        assert_eq!(parsed[0], item);
    }

    #[tokio::test]
    async fn read_capped_bytes_truncates_and_drains_oversized_stream() {
        let payload = vec![b'Z'; 4096];
        let res = read_capped_bytes(payload.as_slice(), 1024).await;
        assert!(res.truncated);
        assert_eq!(res.bytes.len(), 1024);
        assert!(res.bytes.iter().all(|&b| b == b'Z'));
    }

    #[tokio::test]
    async fn wait_until_idle_never_misses_wakeup_under_contention() {
        let guard = Arc::new(ProcessGuard::new());
        for _ in 0..200 {
            let scope = guard.try_enter_busy().unwrap();
            let g1 = Arc::clone(&guard);
            let g2 = Arc::clone(&guard);
            let w1 = tokio::spawn(async move { g1.wait_until_idle().await });
            let w2 = tokio::spawn(async move { g2.wait_until_idle().await });
            tokio::task::yield_now().await;
            drop(scope);
            tokio::time::timeout(Duration::from_secs(5), async {
                w1.await.unwrap();
                w2.await.unwrap();
            })
            .await
            .expect("lost wakeup detected in wait_until_idle");
        }
    }

    #[test]
    fn poisoned_mutex_recovers_consistently_across_all_methods() {
        let guard = Arc::new(ProcessGuard::new());
        let poison_target = Arc::clone(&guard);
        let _ = std::thread::spawn(move || {
            let _lock = poison_target.state.lock().unwrap();
            panic!("intentional poison");
        })
        .join();

        assert!(!guard.is_busy());
        let scope = guard.try_enter_busy_for(OperationKind::Build).unwrap();
        guard.register_pid(999_991);
        assert_eq!(guard.tracked_pid_count(), 1);
        guard.unregister_pid(999_991);
        assert_eq!(guard.tracked_pid_count(), 0);

        guard.register_pid(999_992);
        assert!(guard.cancel_operation_kind(OperationKind::Build));
        assert_eq!(guard.tracked_pid_count(), 0);
        assert!(guard.is_cancel_requested());

        guard.kill_all_tracked_trees();
        drop(scope);
        assert!(!guard.is_busy());
    }

    #[test]
    fn kill_plan_falls_back_to_job_pids_when_job_termination_fails() {
        let plan = KillPlan {
            #[cfg(windows)]
            job: None,
            job_pids: vec![101, 102],
            fallback_pids: vec![201],
        };

        assert_eq!(plan.pids_to_kill(true), vec![201]);
        assert_eq!(plan.pids_to_kill(false), vec![201, 101, 102]);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn run_tracked_command_raw_captures_large_single_line_json_from_child() {
        let script = "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
            $items = 1..1500 | ForEach-Object { [PSCustomObject]@{ Id = $_; Name = 'Пакет_Тест_Юникод_' + $_ } }; \
            ConvertTo-Json -InputObject @($items) -Depth 3 -Compress";
        let args = vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            script.to_string(),
        ];

        let out = run_tracked_command_raw("powershell.exe", &args, None, None)
            .await
            .expect("powershell raw command failed");
        assert_eq!(
            out.exit_code,
            0,
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.stdout_truncated);
        assert!(out.stdout.len() > MAX_LINE_BYTES);

        let trimmed = String::from_utf8(out.stdout).expect("stdout must be valid UTF-8");
        let parsed: Vec<serde_json::Value> = serde_json::from_str(trimmed.trim())
            .expect("large single-line JSON must parse cleanly");
        assert_eq!(parsed.len(), 1500);
        assert_eq!(parsed[1499]["Name"], "Пакет_Тест_Юникод_1500");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn run_tracked_command_cancels_promptly_via_select_and_unregisters_pid() {
        let guard = Arc::new(ProcessGuard::new());
        let _scope = guard.try_enter_busy_for(OperationKind::Build).unwrap();

        let g_cancel = Arc::clone(&guard);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            g_cancel.request_cancel();
        });

        let args = vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            "Start-Sleep -Seconds 30".to_string(),
        ];
        let res = tokio::time::timeout(
            Duration::from_secs(5),
            run_tracked_command("powershell.exe", &args, None, Some(&guard), |_| {}),
        )
        .await
        .expect("streaming cancel timed out");

        assert_eq!(res.err().as_deref(), Some(CANCELLED_ERR));
        assert_eq!(guard.tracked_pid_count(), 0);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn run_tracked_command_raw_cancels_promptly_and_unregisters_pid() {
        let guard = Arc::new(ProcessGuard::new());
        let _scope = guard
            .try_enter_busy_for(OperationKind::CatalogLoad)
            .unwrap();

        let g_cancel = Arc::clone(&guard);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            g_cancel.request_cancel();
        });

        let args = vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            "Start-Sleep -Seconds 30".to_string(),
        ];
        let res = tokio::time::timeout(
            Duration::from_secs(5),
            run_tracked_command_raw("powershell.exe", &args, None, Some(&guard)),
        )
        .await
        .expect("raw cancel timed out");

        assert_eq!(res.err().as_deref(), Some(CANCELLED_ERR));
        assert_eq!(guard.tracked_pid_count(), 0);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn run_tracked_command_drop_unregisters_pid() {
        let guard = ProcessGuard::new();
        let _scope = guard.try_enter_busy_for(OperationKind::Generic).unwrap();

        let args = vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            "Start-Sleep -Seconds 30".to_string(),
        ];
        let timed = tokio::time::timeout(
            Duration::from_millis(150),
            run_tracked_command("powershell.exe", &args, None, Some(&guard), |_| {}),
        )
        .await;

        assert!(timed.is_err(), "expected timeout to drop command future");
        assert_eq!(guard.tracked_pid_count(), 0);
    }
}
