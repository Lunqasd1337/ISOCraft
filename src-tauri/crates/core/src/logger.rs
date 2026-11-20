use crate::image::decode_text_bytes;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

pub struct SessionLogger {
    entries: Mutex<Vec<LogEntry>>,
    next_id: AtomicU64,
}

impl Default for SessionLogger {
    fn default() -> Self {
        Self::new()
    }
}

fn read_file_tail_bytes(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len <= max_bytes {
        let mut buf = Vec::with_capacity(len as usize);
        file.read_to_end(&mut buf)?;
        return Ok(buf);
    }

    let mut bom = [0u8; 2];
    let has_utf16_bom = if len >= 2 && file.read_exact(&mut bom).is_ok() {
        bom == [0xFF, 0xFE] || bom == [0xFE, 0xFF]
    } else {
        false
    };

    let mut start = len.saturating_sub(max_bytes);
    if has_utf16_bom && start % 2 != 0 {
        start = start.saturating_add(1).min(len);
    }

    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::with_capacity(max_bytes as usize + 2);
    if has_utf16_bom && start >= 2 {
        tail.extend_from_slice(&bom);
    }
    file.read_to_end(&mut tail)?;
    Ok(tail)
}

impl SessionLogger {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn record(&self, level: &str, message: impl Into<String>) -> LogEntry {
        let entry = LogEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            level: level.to_string(),
            message: message.into(),
        };

        if let Ok(mut guard) = self.entries.lock() {
            if guard.len() >= 5000 {
                let drain_count = guard.len() - 4000;
                guard.drain(0..drain_count);
            }
            guard.push(entry.clone());
        }

        entry
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn export_diagnostics(
        &self,
        dest_path: &Path,
        workspace_dir: &Path,
    ) -> Result<String, String> {
        let mut report = String::new();
        report.push_str("=== ISOCraft Diagnostic Log ===\n");
        report.push_str(&format!(
            "Generated: {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        ));
        report.push_str(&format!("Workspace: {}\n\n", workspace_dir.display()));

        report.push_str("--- Application Session Log ---\n");
        for entry in self.snapshot() {
            report.push_str(&format!(
                "[{}] [{}] {}\n",
                entry.timestamp,
                entry.level.to_uppercase(),
                entry.message
            ));
        }

        let dism_log = workspace_dir.join("dism.log");
        if dism_log.exists() {
            report.push_str("\n--- DISM Log (tail) ---\n");
            match read_file_tail_bytes(&dism_log, 256_000) {
                Ok(bytes) => {
                    let content = decode_text_bytes(&bytes);
                    let len = content.len();
                    let tail_start = if len > 256_000 {
                        content.floor_char_boundary(len - 256_000)
                    } else {
                        0
                    };
                    report.push_str(&content[tail_start..]);
                }
                Err(e) => {
                    report.push_str(&format!("(Failed to read dism.log: {e})\n"));
                }
            }
        } else {
            report.push_str("\n--- DISM Log ---\n(No dism.log in current workspace yet)\n");
        }

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create destination folder: {e}"))?;
        }
        fs::write(dest_path, report)
            .map_err(|e| format!("Failed to write diagnostics file: {e}"))?;
        Ok(dest_path.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logger_records_and_exports_entries() {
        let logger = SessionLogger::new();
        logger.record("info", "stage 1 ready");
        logger.record("warn", "low disk space");

        let snap = logger.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].id, 1);
        assert_eq!(snap[1].id, 2);
        assert_eq!(snap[0].level, "info");
        assert_eq!(snap[1].message, "low disk space");
    }

    #[test]
    fn export_diagnostics_handles_non_utf8_dism_log() {
        let dir = std::env::temp_dir().join(format!("w11_diag_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let dism_log = dir.join("dism.log");
        let mut raw = b"DISM started\nError bytes: ".to_vec();
        raw.extend_from_slice(&[0xC0, 0xFF, 0xFE, b'\n']);
        fs::write(&dism_log, raw).unwrap();

        let logger = SessionLogger::new();
        logger.record("info", "diag test");
        let dest = dir.join("out.log");
        logger.export_diagnostics(&dest, &dir).unwrap();

        let exported = fs::read_to_string(&dest).unwrap();
        assert!(exported.contains("DISM started"));
        assert!(exported.contains("diag test"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_diagnostics_reads_only_tail_of_large_dism_log() {
        let dir = std::env::temp_dir().join(format!("w11_diag_tail_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let dism_log = dir.join("dism.log");
        let mut large = b"HEAD_MARKER\n".to_vec();
        large.extend(std::iter::repeat_n(b'A', 300_000));
        large.extend_from_slice(b"\nTAIL_MARKER\n");
        fs::write(&dism_log, large).unwrap();

        let logger = SessionLogger::new();
        let dest = dir.join("tail_out.log");
        logger.export_diagnostics(&dest, &dir).unwrap();

        let exported = fs::read_to_string(&dest).unwrap();
        assert!(!exported.contains("HEAD_MARKER"));
        assert!(exported.contains("TAIL_MARKER"));

        let _ = fs::remove_dir_all(&dir);
    }
}
