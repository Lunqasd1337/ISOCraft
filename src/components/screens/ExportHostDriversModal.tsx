import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckSquare,
  FolderOpen,
  HardDriveDownload,
  Loader2,
  RefreshCw,
  Search,
  Square,
  X,
} from "lucide-react";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { localizeBackendError } from "../../utils/backendErrors";
import { driverCategoryLabelKey } from "../../utils/driverCategories";
import type {
  DriverExportProgress,
  HostDriverExportResult,
  InstalledDriverInfo,
} from "../../types/studio";

interface ExportHostDriversModalProps {
  workspaceDir: string;
  canInjectBootWim: boolean;
  onExported: (result: HostDriverExportResult, injectStorageToBootWim: boolean) => void;
  onClose: () => void;
}

const CATEGORY_ORDER = [
  "all",
  "storage",
  "network",
  "display",
  "audio",
  "usb",
  "bluetooth",
  "input",
  "other",
] as const;

function buildDefaultExportDir(workspaceDir: string): string {
  const cleanWs = workspaceDir.trim().replace(/[\\/]+$/, "");
  const now = new Date();
  const yyyy = now.getFullYear();
  const mm = String(now.getMonth() + 1).padStart(2, "0");
  const dd = String(now.getDate()).padStart(2, "0");
  const hh = String(now.getHours()).padStart(2, "0");
  const mi = String(now.getMinutes()).padStart(2, "0");
  const base = cleanWs || "C:\\ISOCraft";
  return `${base}\\exports\\drivers\\host_${yyyy}${mm}${dd}_${hh}${mi}`;
}

export function ExportHostDriversModal({
  workspaceDir,
  canInjectBootWim,
  onExported,
  onClose,
}: ExportHostDriversModalProps) {
  const { t } = useTranslation();
  const [drivers, setDrivers] = useState<InstalledDriverInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  const [categoryFilter, setCategoryFilter] = useState<string>("all");
  const [selectedNames, setSelectedNames] = useState<Set<string>>(() => new Set());
  const [destinationDir, setDestinationDir] = useState<string>(() =>
    buildDefaultExportDir(workspaceDir)
  );
  const [autoSplitStorage, setAutoSplitStorage] = useState<boolean>(canInjectBootWim);

  const [exporting, setExporting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [exportProgress, setExportProgress] = useState<DriverExportProgress | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);

  const mountedRef = useRef(true);
  const loadSeqRef = useRef(0);

  const { isClosing, requestClose } = useAnimatedModalClose(onClose, {
    locked: exporting,
  });

  const loadDrivers = useCallback(async () => {
    const seq = ++loadSeqRef.current;
    setLoading(true);
    setLoadError(null);
    try {
      const items = await invoke<InstalledDriverInfo[]>("list_host_drivers");
      if (!mountedRef.current || loadSeqRef.current !== seq) return;
      setDrivers(items);
    } catch (err) {
      if (!mountedRef.current || loadSeqRef.current !== seq) return;
      setLoadError(String(err));
    } finally {
      if (mountedRef.current && loadSeqRef.current === seq) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void loadDrivers();
    return () => {
      mountedRef.current = false;
      loadSeqRef.current += 1;
    };
  }, [loadDrivers]);

  useEffect(() => {
    let active = true;
    const unlisten = listen<DriverExportProgress>("driver-export-progress", (event) => {
      if (!active) return;
      setExportProgress(event.payload);
    });
    return () => {
      active = false;
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const availableCategories = useMemo(() => {
    const counts = new Map<string, number>();
    for (const d of drivers) {
      counts.set(d.category, (counts.get(d.category) ?? 0) + 1);
    }
    return CATEGORY_ORDER.filter(
      (cat) => cat === "all" || (counts.get(cat) ?? 0) > 0
    ).map((cat) => ({
      id: cat,
      count: cat === "all" ? drivers.length : counts.get(cat) ?? 0,
    }));
  }, [drivers]);

  const filteredDrivers = useMemo(() => {
    const q = deferredSearch.trim().toLowerCase();
    return drivers.filter((d) => {
      if (categoryFilter !== "all" && d.category !== categoryFilter) return false;
      if (!q) return true;
      return (
        d.originalFileName.toLowerCase().includes(q) ||
        d.publishedName.toLowerCase().includes(q) ||
        d.providerName.toLowerCase().includes(q) ||
        d.className.toLowerCase().includes(q)
      );
    });
  }, [categoryFilter, deferredSearch, drivers]);

  const toggleDriver = (publishedName: string) => {
    if (exporting) return;
    const key = publishedName.toLowerCase();
    setSelectedNames((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleSelectVisible = () => {
    if (exporting) return;
    setSelectedNames((prev) => {
      const next = new Set(prev);
      for (const d of filteredDrivers) {
        next.add(d.publishedName.toLowerCase());
      }
      return next;
    });
  };

  const handleClearSelection = () => {
    if (exporting) return;
    setSelectedNames(new Set());
  };

  const handleBrowseDestination = async () => {
    if (exporting) return;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (typeof selected === "string" && selected.trim()) {
        setDestinationDir(selected);
      }
    } catch (err) {
      setExportError(String(err));
    }
  };

  const runExport = async (exportAll: boolean) => {
    if (exporting || loading) return;
    if (!exportAll && selectedNames.size === 0) return;

    setExporting(true);
    setCancelling(false);
    setExportError(null);
    setExportProgress(null);

    try {
      const publishedNames = drivers
        .filter((d) => selectedNames.has(d.publishedName.toLowerCase()))
        .map((d) => d.publishedName);

      const res = await invoke<HostDriverExportResult>("export_host_drivers", {
        request: {
          publishedNames: exportAll ? [] : publishedNames,
          exportAll,
          destinationDir: destinationDir.trim(),
          autoSplitStorage: canInjectBootWim ? autoSplitStorage : false,
        },
      });
      if (!mountedRef.current) return;
      onExported(res, canInjectBootWim && autoSplitStorage);
      setExporting(false);
      onClose();
    } catch (err) {
      if (!mountedRef.current) return;
      setExportError(String(err));
      setExporting(false);
      setCancelling(false);
    }
  };

  const handleCancelExport = async () => {
    if (!exporting || cancelling) return;
    setCancelling(true);
    try {
      await invoke("cancel_driver_export");
    } catch (err) {
      setExportError(String(err));
    }
  };

  const progressPct =
    exportProgress && exportProgress.total > 0
      ? Math.min(100, Math.round((exportProgress.current / exportProgress.total) * 100))
      : null;

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestClose}
      ariaLabel={t("screens.drivers.hostExportModal.title")}
      maxWidthClass="max-w-[760px]"
      panelClassName="flex flex-col max-h-[86vh]"
    >
      {/* Header */}
      <div className="px-5 py-4 border-b border-border-subtle flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="text-[15.5px] font-semibold text-text-primary flex items-center gap-2">
            <HardDriveDownload className="w-4 h-4 text-accent shrink-0" strokeWidth={1.75} />
            <span>{t("screens.drivers.hostExportModal.title")}</span>
          </h2>
          <p className="text-[12px] text-text-secondary mt-0.5">
            {t("screens.drivers.hostExportModal.subtitle")}
          </p>
        </div>
        <button
          type="button"
          disabled={exporting}
          onClick={requestClose}
          aria-label={t("modals.closeAria")}
          className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.95] disabled:opacity-40 disabled:cursor-not-allowed shrink-0"
        >
          <X className="w-4 h-4" strokeWidth={1.75} />
        </button>
      </div>

      {/* Filter & Search Bar */}
      <div className="px-5 py-3 border-b border-border-subtle space-y-2.5 bg-canvas">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative flex-1 min-w-[200px]">
            <Search
              className="w-3.5 h-3.5 text-text-secondary absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none"
              strokeWidth={1.6}
            />
            <input
              type="text"
              disabled={loading || exporting}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("screens.drivers.hostExportModal.searchPlaceholder")}
              className="w-full pl-8 pr-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[12.5px] text-text-primary focus:outline-none focus:border-accent transition-colors duration-150 disabled:opacity-50"
            />
          </div>
          <button
            type="button"
            disabled={loading || exporting || filteredDrivers.length === 0}
            onClick={handleSelectVisible}
            className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <CheckSquare className="w-3.5 h-3.5" strokeWidth={1.6} />
            <span>{t("screens.drivers.hostExportModal.selectVisible")}</span>
          </button>
          <button
            type="button"
            disabled={loading || exporting || selectedNames.size === 0}
            onClick={handleClearSelection}
            className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Square className="w-3.5 h-3.5" strokeWidth={1.6} />
            <span>{t("screens.drivers.hostExportModal.clearSelection")}</span>
          </button>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {availableCategories.map((cat) => {
            const active = categoryFilter === cat.id;
            return (
              <button
                key={cat.id}
                type="button"
                disabled={loading || exporting}
                onClick={() => setCategoryFilter(cat.id)}
                className={`px-2.5 py-1 rounded-md text-[11.5px] font-medium border transition-all duration-150 active:scale-[0.97] disabled:opacity-50 ${
                  active
                    ? "bg-accent border-accent text-accent-fg"
                    : "bg-surface border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle"
                }`}
              >
                {t(driverCategoryLabelKey(cat.id))} ({cat.count})
              </button>
            );
          })}
        </div>
      </div>

      {/* Driver List */}
      <div className="flex-1 overflow-y-auto divide-y divide-border-subtle min-h-[220px]">
        {loading ? (
          <div className="py-12 flex flex-col items-center justify-center gap-2.5 text-[13px] text-text-secondary">
            <Loader2 className="w-5 h-5 animate-spin text-accent" strokeWidth={1.8} />
            <span>{t("screens.drivers.hostExportModal.loading")}</span>
          </div>
        ) : loadError ? (
          <div className="p-6 flex flex-col items-center text-center gap-3">
            <div className="px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px] flex items-center gap-2">
              <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
              <span>{localizeBackendError(loadError)}</span>
            </div>
            <button
              type="button"
              onClick={() => void loadDrivers()}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
            >
              <RefreshCw className="w-3.5 h-3.5" strokeWidth={1.6} />
              <span>{t("screens.drivers.hostExportModal.retry")}</span>
            </button>
          </div>
        ) : filteredDrivers.length === 0 ? (
          <div className="py-10 text-center text-[13px] text-text-secondary">
            {t("screens.drivers.hostExportModal.noMatches")}
          </div>
        ) : (
          filteredDrivers.map((drv) => {
            const key = drv.publishedName.toLowerCase();
            const checked = selectedNames.has(key);
            return (
              <label
                key={drv.publishedName}
                className={`px-5 py-2.5 flex items-start gap-3 transition-colors ${
                  exporting ? "cursor-not-allowed opacity-70" : "cursor-pointer hover:bg-subtle"
                }`}
              >
                <input
                  type="checkbox"
                  disabled={exporting}
                  checked={checked}
                  onChange={() => toggleDriver(drv.publishedName)}
                  className="accent-accent mt-1 shrink-0"
                />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[13px] font-semibold text-text-primary truncate">
                      {drv.originalFileName}
                    </span>
                    <span className="px-1.5 py-0.5 rounded text-[11px] font-mono bg-subtle text-text-secondary shrink-0">
                      {drv.publishedName}
                    </span>
                    <span className="px-1.5 py-0.5 rounded text-[11px] font-medium bg-canvas border border-border-subtle text-text-secondary shrink-0">
                      {drv.className || t(driverCategoryLabelKey(drv.category))}
                    </span>
                    {drv.bootCritical && (
                      <span className="px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-warning-bg text-status-warning shrink-0">
                        {t("screens.drivers.bootCriticalBadge")}
                      </span>
                    )}
                  </div>
                  <div className="text-[11.5px] text-text-secondary flex flex-wrap items-center gap-x-2.5 mt-0.5">
                    {drv.providerName && <span>{drv.providerName}</span>}
                    {drv.version && <span className="font-mono">v{drv.version}</span>}
                    {drv.date && <span className="font-mono">{drv.date}</span>}
                  </div>
                </div>
              </label>
            );
          })
        )}
      </div>

      {/* Footer / Destination & Export Progress */}
      <div className="px-5 py-4 border-t border-border-subtle bg-canvas space-y-3">
        {exportError && (
          <div className="animate-fluent-item flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
            <div className="flex items-center gap-2 min-w-0">
              <AlertTriangle className="w-3.5 h-3.5 shrink-0" strokeWidth={1.75} />
              <span className="break-words">{localizeBackendError(exportError)}</span>
            </div>
            <button
              type="button"
              onClick={() => setExportError(null)}
              className="p-0.5 rounded hover:bg-subtle shrink-0"
            >
              <X className="w-3.5 h-3.5" strokeWidth={1.7} />
            </button>
          </div>
        )}

        {exporting ? (
          <div className="animate-fluent-item space-y-2.5">
            <div className="flex items-center justify-between gap-3 text-[12.5px]">
              <div className="flex items-center gap-2 text-text-primary font-medium min-w-0">
                <Loader2 className="w-4 h-4 animate-spin text-accent shrink-0" strokeWidth={1.8} />
                <span className="truncate">
                  {exportProgress
                    ? t("screens.drivers.hostExportModal.exportProgress", {
                        current: exportProgress.current,
                        total: exportProgress.total,
                        name: exportProgress.driverName,
                      })
                    : t("screens.drivers.hostExportModal.exportingLabel")}
                </span>
              </div>
              {progressPct !== null && (
                <span className="font-mono text-text-secondary shrink-0">
                  {progressPct}%
                </span>
              )}
            </div>
            <div className="w-full h-1.5 rounded-full bg-subtle overflow-hidden">
              {progressPct !== null ? (
                <div
                  className="h-full bg-accent transition-[width] duration-200 ease-out"
                  style={{ width: `${progressPct}%` }}
                />
              ) : (
                <div className="h-full w-2/5 bg-accent animate-fluent-indeterminate rounded-full" />
              )}
            </div>
            <div className="flex justify-end">
              <button
                type="button"
                disabled={cancelling}
                onClick={handleCancelExport}
                className="px-3 py-1.5 rounded-md text-[12.5px] font-semibold bg-status-danger text-white hover:opacity-90 transition-all duration-150 active:scale-[0.98] disabled:opacity-50"
              >
                {cancelling
                  ? t("screens.drivers.hostExportModal.cancelling")
                  : t("screens.drivers.hostExportModal.cancelExport")}
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="space-y-1.5">
              <label className="block text-[12px] font-semibold text-text-primary">
                {t("screens.drivers.hostExportModal.destLabel")}
              </label>
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={destinationDir}
                  onChange={(e) => setDestinationDir(e.target.value)}
                  className="flex-1 px-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[12px] font-mono text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
                />
                <button
                  type="button"
                  onClick={handleBrowseDestination}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] shrink-0"
                >
                  <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                  <span>{t("screens.drivers.hostExportModal.browse")}</span>
                </button>
              </div>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
              <label
                title={
                  canInjectBootWim
                    ? undefined
                    : t("screens.drivers.bootWimUnavailable")
                }
                className={`flex items-center gap-2 text-[12.5px] ${
                  canInjectBootWim
                    ? "cursor-pointer text-text-primary"
                    : "cursor-not-allowed opacity-50 text-text-secondary"
                }`}
              >
                <input
                  type="checkbox"
                  disabled={!canInjectBootWim}
                  checked={canInjectBootWim && autoSplitStorage}
                  onChange={(e) => setAutoSplitStorage(e.target.checked)}
                  className="accent-accent"
                />
                <span>{t("screens.drivers.hostExportModal.autoSplitStorage")}</span>
              </label>

              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={requestClose}
                  className="px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                >
                  {t("screens.drivers.hostExportModal.close")}
                </button>
                <button
                  type="button"
                  disabled={loading || drivers.length === 0 || !destinationDir.trim()}
                  onClick={() => void runExport(true)}
                  className="px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {t("screens.drivers.hostExportModal.exportAllBtn", {
                    count: drivers.length,
                  })}
                </button>
                <button
                  type="button"
                  disabled={loading || selectedNames.size === 0 || !destinationDir.trim()}
                  onClick={() => void runExport(false)}
                  className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-md text-[12.5px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <HardDriveDownload className="w-3.5 h-3.5" strokeWidth={1.7} />
                  <span>
                    {t("screens.drivers.hostExportModal.exportSelectedBtn", {
                      count: selectedNames.size,
                    })}
                  </span>
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </FluentModal>
  );
}
