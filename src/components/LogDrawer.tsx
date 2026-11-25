import { useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { Check, Copy, Download, Search, X } from "lucide-react";
import { FluentSegmentedControl } from "./FluentMotion";
import { prefersReducedMotion } from "../utils/motion";
import { localizeBackendError } from "../utils/backendErrors";
import { useLogs } from "../hooks/useLogStore";

const LOG_ROW_HEIGHT = 24;
const LOG_OVERSCAN = 12;
const VIRTUALIZE_THRESHOLD = 100;

type LogLevelFilter = "all" | "warn" | "error";

interface LogDrawerProps {
  isOpen: boolean;
  onClose: () => void;
}

export function LogDrawer({ isOpen, onClose }: LogDrawerProps) {
  const { t } = useTranslation();
  const logs = useLogs();
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const isNearBottomRef = useRef(true);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(320);
  const [exportMsg, setExportMsg] = useState<
    { ok: true; path: string } | { ok: false; error: string } | null
  >(null);
  const [levelFilter, setLevelFilter] = useState<LogLevelFilter>("all");
  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<number | null>(null);
  const exportTimerRef = useRef<number | null>(null);
  const [rendered, setRendered] = useState(isOpen);
  const [expanded, setExpanded] = useState(isOpen && prefersReducedMotion());
  const closeTimerRef = useRef<number | null>(null);
  const rafRef = useRef<number | null>(null);
  const rafInnerRef = useRef<number | null>(null);

  const cancelPending = () => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
    if (rafRef.current !== null) {
      window.cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    if (rafInnerRef.current !== null) {
      window.cancelAnimationFrame(rafInnerRef.current);
      rafInnerRef.current = null;
    }
  };

  useEffect(() => {
    cancelPending();

    if (isOpen) {
      setRendered(true);
      if (prefersReducedMotion()) {
        setExpanded(true);
      }
      return;
    }

    setExpanded(false);
    if (prefersReducedMotion()) {
      setRendered(false);
    } else if (rendered) {
      closeTimerRef.current = window.setTimeout(() => {
        closeTimerRef.current = null;
        setRendered(false);
      }, 160);
    }
  }, [isOpen, rendered]);

  useEffect(() => {
    if (!rendered || !isOpen || prefersReducedMotion()) return;
    rafRef.current = window.requestAnimationFrame(() => {
      rafRef.current = null;
      rafInnerRef.current = window.requestAnimationFrame(() => {
        rafInnerRef.current = null;
        setExpanded(true);
      });
    });
    return () => {
      if (rafRef.current !== null) {
        window.cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      if (rafInnerRef.current !== null) {
        window.cancelAnimationFrame(rafInnerRef.current);
        rafInnerRef.current = null;
      }
    };
  }, [rendered, isOpen]);

  useEffect(() => {
    return () => {
      cancelPending();
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
      }
      if (exportTimerRef.current !== null) {
        window.clearTimeout(exportTimerRef.current);
      }
    };
  }, []);

  const filteredLogs = useMemo(() => {
    const q = deferredSearch.trim().toLowerCase();
    return logs.filter((entry) => {
      if (levelFilter === "warn" && entry.level !== "warn" && entry.level !== "error") {
        return false;
      }
      if (levelFilter === "error" && entry.level !== "error") {
        return false;
      }
      if (!q) return true;
      return (
        entry.message.toLowerCase().includes(q) ||
        entry.timestamp.toLowerCase().includes(q)
      );
    });
  }, [deferredSearch, levelFilter, logs]);

  useEffect(() => {
    if (!rendered) return;
    const el = scrollContainerRef.current;
    if (!el) return;
    setViewportHeight(el.clientHeight || 320);
    if (typeof ResizeObserver !== "undefined") {
      const ro = new ResizeObserver(() => {
        setViewportHeight(el.clientHeight || 320);
      });
      ro.observe(el);
      return () => ro.disconnect();
    }
  }, [rendered]);

  useEffect(() => {
    if (!rendered) return;
    const el = scrollContainerRef.current;
    if (el && isNearBottomRef.current) {
      el.scrollTop = el.scrollHeight;
      setScrollTop(el.scrollTop);
    }
  }, [filteredLogs.length, rendered]);

  const handleCopy = async () => {
    const source = filteredLogs.length > 0 ? filteredLogs : logs;
    if (source.length === 0) return;
    const text = source
      .map((e) => `[${e.timestamp}] ${e.level.toUpperCase()} ${e.message}`)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
      }
      copyTimerRef.current = window.setTimeout(() => {
        copyTimerRef.current = null;
        setCopied(false);
      }, 2000);
    } catch {
      // ignore clipboard errors
    }
  };

  const handleScroll = () => {
    const el = scrollContainerRef.current;
    if (!el) return;
    setScrollTop(el.scrollTop);
    isNearBottomRef.current =
      el.scrollHeight - el.scrollTop - el.clientHeight < 48;
  };

  const shouldVirtualize = filteredLogs.length > VIRTUALIZE_THRESHOLD;
  const visibleCount = Math.max(1, Math.ceil(viewportHeight / LOG_ROW_HEIGHT));
  const startIndex = shouldVirtualize
    ? Math.max(0, Math.floor(scrollTop / LOG_ROW_HEIGHT) - LOG_OVERSCAN)
    : 0;
  const endIndex = shouldVirtualize
    ? Math.min(filteredLogs.length, startIndex + visibleCount + LOG_OVERSCAN * 2)
    : filteredLogs.length;
  const topSpacerHeight = shouldVirtualize ? startIndex * LOG_ROW_HEIGHT : 0;
  const bottomSpacerHeight = shouldVirtualize
    ? Math.max(0, (filteredLogs.length - endIndex) * LOG_ROW_HEIGHT)
    : 0;
  const visibleLogs = shouldVirtualize
    ? filteredLogs.slice(startIndex, endIndex)
    : filteredLogs;

  const handleExport = async () => {
    try {
      const dest = await save({
        defaultPath: "isocraft-diagnostics.log",
        filters: [{ name: t("dialogs.logFiles"), extensions: ["log", "txt"] }],
      });
      if (!dest) return;
      const savedPath = await invoke<string>("export_diagnostics", {
        destPath: dest,
      });
      setExportMsg({ ok: true, path: savedPath });
      if (exportTimerRef.current !== null) {
        window.clearTimeout(exportTimerRef.current);
      }
      exportTimerRef.current = window.setTimeout(() => {
        exportTimerRef.current = null;
        setExportMsg(null);
      }, 4000);
    } catch (err) {
      setExportMsg({ ok: false, error: String(err) });
    }
  };

  if (!rendered) return null;

  return (
    <div
      className={`shrink-0 overflow-hidden ${
        expanded
          ? "h-[200px] fluent-drawer-shell-open"
          : "h-0 fluent-drawer-shell-close"
      }`}
    >
      <section
        className={`h-[200px] bg-surface border-t border-border-subtle flex flex-col ${
          expanded
            ? "opacity-100 translate-y-0 fluent-drawer-panel-open"
            : "opacity-0 translate-y-3 fluent-drawer-panel-close"
        }`}
      >
        <div className="min-h-10 px-4 py-1.5 border-b border-border-subtle flex items-center justify-between gap-3 bg-sidebar shrink-0">
          <div className="flex items-center gap-3 min-w-0">
            <span className="text-[12.5px] font-semibold text-text-primary shrink-0">
              {t("logDrawer.title")}
            </span>
            {exportMsg && (
              <span
                className={`animate-fluent-item text-[11.5px] truncate max-w-[280px] ${
                  exportMsg.ok ? "text-status-success" : "text-status-danger"
                }`}
              >
                {exportMsg.ok
                  ? t("logDrawer.exported", { path: exportMsg.path })
                  : localizeBackendError(exportMsg.error)}
              </span>
            )}
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <FluentSegmentedControl<LogLevelFilter>
              value={levelFilter}
              onChange={setLevelFilter}
              variant="accent"
              size="xs"
              options={[
                { value: "all", label: t("logDrawer.filterAll") },
                { value: "warn", label: t("logDrawer.filterWarn") },
                { value: "error", label: t("logDrawer.filterError") },
              ]}
            />
            <div className="relative">
              <Search
                className="w-3.5 h-3.5 text-text-secondary absolute left-2 top-1/2 -translate-y-1/2 pointer-events-none"
                strokeWidth={1.6}
              />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("logDrawer.searchPlaceholder")}
                className="w-36 sm:w-44 pl-7 pr-2 py-1 rounded bg-surface border border-border-subtle text-[11.5px] text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent"
              />
            </div>
            <button
              type="button"
              onClick={handleCopy}
              disabled={logs.length === 0}
              className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium text-text-primary bg-surface border border-border-subtle hover:bg-subtle transition-all duration-150 active:scale-[0.98] disabled:opacity-45 disabled:active:scale-100"
            >
              {copied ? (
                <Check className="w-3.5 h-3.5 text-status-success" strokeWidth={1.8} />
              ) : (
                <Copy className="w-3.5 h-3.5" strokeWidth={1.6} />
              )}
              <span>{copied ? t("logDrawer.copied") : t("logDrawer.copy")}</span>
            </button>
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
              onClick={onClose}
              title={t("logDrawer.close")}
              className="w-7 h-7 rounded flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.95]"
            >
              <X className="w-3.5 h-3.5" strokeWidth={1.75} />
            </button>
          </div>
        </div>

        <div
          ref={scrollContainerRef}
          onScroll={handleScroll}
          className="flex-1 overflow-y-auto px-4 py-2 font-mono text-[12px] leading-relaxed select-text"
        >
          {logs.length === 0 ? (
            <div className="text-text-secondary italic py-4">
              {t("logDrawer.empty")}
            </div>
          ) : filteredLogs.length === 0 ? (
            <div className="text-text-secondary italic py-4">
              {t("logDrawer.noMatches")}
            </div>
          ) : (
            <>
              {topSpacerHeight > 0 && (
                <div style={{ height: topSpacerHeight }} aria-hidden="true" />
              )}
              {visibleLogs.map((entry, idx) => {
                const actualIndex = startIndex + idx;
                const levelColor =
                  entry.level === "error"
                    ? "text-status-danger"
                    : entry.level === "warn"
                    ? "text-status-warning"
                    : "text-text-secondary";
                return (
                  <div
                    key={entry.id ?? actualIndex}
                    className={`flex items-start gap-2.5 py-0.5 ${
                      shouldVirtualize ? "min-h-[24px]" : ""
                    }`}
                  >
                    <span className="text-text-secondary shrink-0">
                      [{entry.timestamp}]
                    </span>
                    <span className={`uppercase font-semibold shrink-0 w-11 ${levelColor}`}>
                      {entry.level}
                    </span>
                    <span className="text-text-primary break-all">{entry.message}</span>
                  </div>
                );
              })}
              {bottomSpacerHeight > 0 && (
                <div style={{ height: bottomSpacerHeight }} aria-hidden="true" />
              )}
            </>
          )}
        </div>
      </section>
    </div>
  );
}
