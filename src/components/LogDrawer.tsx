import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { Download, X } from "lucide-react";
import type { LogEntry } from "../types/studio";

interface LogDrawerProps {
  logs: LogEntry[];
  onClose: () => void;
}

export function LogDrawer({ logs, onClose }: LogDrawerProps) {
  const { t } = useTranslation();
  const bottomRef = useRef<HTMLDivElement>(null);
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const [isClosing, setIsClosing] = useState(false);
  const closeTimerRef = useRef<number | null>(null);

  const requestClose = useCallback(() => {
    if (isClosing) return;
    setIsClosing(true);
    closeTimerRef.current = window.setTimeout(() => {
      onClose();
    }, 150);
  }, [isClosing, onClose]);

  useEffect(() => {
    return () => {
      if (closeTimerRef.current !== null) {
        window.clearTimeout(closeTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs.length]);

  const handleExport = async () => {
    try {
      const dest = await save({
        defaultPath: "isocraft-diagnostics.log",
        filters: [{ name: "Log files", extensions: ["log", "txt"] }],
      });
      if (!dest) return;
      const savedPath = await invoke<string>("export_diagnostics", {
        destPath: dest,
      });
      setExportMsg(t("logDrawer.exported", { path: savedPath }));
      setTimeout(() => setExportMsg(null), 4000);
    } catch (err) {
      setExportMsg(String(err));
    }
  };

  return (
    <section
      className={`${isClosing ? "animate-fluent-drawer-exit" : "animate-fluent-drawer"} h-[200px] shrink-0 bg-surface border-t border-border-subtle flex flex-col`}
    >
      <div className="h-9 px-4 border-b border-border-subtle flex items-center justify-between bg-sidebar shrink-0">
        <div className="flex items-center gap-3">
          <span className="text-[12.5px] font-semibold text-text-primary">
            {t("logDrawer.title")}
          </span>
          {exportMsg && (
            <span className="animate-fluent-item text-[11.5px] text-status-success truncate max-w-[420px]">
              {exportMsg}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleExport}
            className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium text-text-primary bg-surface border border-border-subtle hover:bg-subtle transition-all duration-150 active:scale-[0.98]"
          >
            <Download className="w-3.5 h-3.5" strokeWidth={1.6} />
            <span>{t("logDrawer.export")}</span>
          </button>
          <button
            type="button"
            onClick={requestClose}
            title={t("logDrawer.close")}
            className="w-7 h-7 rounded flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
          >
            <X className="w-3.5 h-3.5" strokeWidth={1.75} />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-4 py-2 font-mono text-[12px] leading-relaxed select-text">
        {logs.length === 0 ? (
          <div className="text-text-secondary italic py-4">
            {t("logDrawer.empty")}
          </div>
        ) : (
          logs.map((entry, idx) => {
            const levelColor =
              entry.level === "error"
                ? "text-status-danger"
                : entry.level === "warn"
                ? "text-status-warning"
                : "text-text-secondary";
            return (
              <div key={idx} className="flex items-start gap-2.5 py-0.5">
                <span className="text-text-secondary shrink-0">
                  [{entry.timestamp}]
                </span>
                <span className={`uppercase font-semibold shrink-0 w-11 ${levelColor}`}>
                  {entry.level}
                </span>
                <span className="text-text-primary break-all">{entry.message}</span>
              </div>
            );
          })
        )}
        <div ref={bottomRef} />
      </div>
    </section>
  );
}
