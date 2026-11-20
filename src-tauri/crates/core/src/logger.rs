use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

pub struct SessionLogger {
    entries: Mutex<Vec<LogEntry>>,
}

impl Default for SessionLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLogger {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    pub fn record(&self, level: &str, message: impl Into<String>) -> LogEntry {
        let entry = LogEntry {
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
            if let Ok(content) = fs::read_to_string(&dism_log) {
                let len = content.len();
                let tail_start = if len > 256_000 {
                    content.floor_char_boundary(len - 256_000)
                } else {
                    0
                };
                report.push_str(&content[tail_start..]);
            } else {
                report.push_str("(Failed to read dism.log as UTF-8)\n");
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
        assert_eq!(snap[0].level, "info");
        assert_eq!(snap[1].message, "low disk space");
    }
}
