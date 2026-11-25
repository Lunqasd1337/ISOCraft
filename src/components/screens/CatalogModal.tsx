import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { prefersReducedMotion } from "../../utils/motion";
import { formatBytes } from "../../utils/format";
import { localizeBackendError } from "../../utils/backendErrors";
import { WIN11_VERSIONS, resolveWin11Version } from "../../utils/win11Version";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  ChevronDown,
  Download,
  ExternalLink,
  FolderOpen,
  Loader2,
  Search,
  X,
} from "lucide-react";
import type {
  CatalogDownloadProgress,
  CatalogDownloadResult,
  CatalogUpdateEntry,
  UpdateQueueItem,
} from "../../types/studio";

interface CatalogModalProps {
  editionBuildNumber: number | null;
  workspaceDir: string;
  queuedPaths: string[];
  onDownloaded: (items: UpdateQueueItem[]) => void;
  onClose: () => void;
  isBuilding?: boolean;
}

export function CatalogModal({
  editionBuildNumber,
  workspaceDir,
  queuedPaths,
  onDownloaded,
  onClose,
  isBuilding = false,
}: CatalogModalProps) {
  const { t } = useTranslation();
  const [targetVersion, setTargetVersion] = useState<string>(() =>
    resolveWin11Version(editionBuildNumber)
  );
  const initialQueryRef = useRef(`Windows 11 Version ${targetVersion} for x64`);

  const [query, setQuery] = useState(initialQueryRef.current);
  const [onlyWin11X64, setOnlyWin11X64] = useState(true);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [results, setResults] = useState<CatalogUpdateEntry[]>([]);
  const [hasSearched, setHasSearched] = useState(false);

  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [progress, setProgress] = useState<CatalogDownloadProgress | null>(null);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const [downloadSuccess, setDownloadSuccess] = useState<string | null>(null);
  const [downloadedByUpdate, setDownloadedByUpdate] = useState<Record<string, string[]>>({});
  const [isVersionMenuOpen, setIsVersionMenuOpen] = useState(false);
  const [isVersionMenuClosing, setIsVersionMenuClosing] = useState(false);

  const searchSeqRef = useRef(0);
  const downloadingIdRef = useRef<string | null>(null);
  const mountedRef = useRef(true);
  const versionMenuRef = useRef<HTMLDivElement>(null);
  const versionMenuTimerRef = useRef<number | null>(null);

  const clearVersionMenuTimer = useCallback(() => {
    if (versionMenuTimerRef.current !== null) {
      window.clearTimeout(versionMenuTimerRef.current);
      versionMenuTimerRef.current = null;
    }
  }, []);

  const openVersionMenu = useCallback(() => {
    clearVersionMenuTimer();
    setIsVersionMenuClosing(false);
    setIsVersionMenuOpen(true);
  }, [clearVersionMenuTimer]);

  const closeVersionMenu = useCallback(() => {
    if (!isVersionMenuOpen || isVersionMenuClosing) return;
    if (prefersReducedMotion()) {
      clearVersionMenuTimer();
      setIsVersionMenuOpen(false);
      setIsVersionMenuClosing(false);
      return;
    }
    setIsVersionMenuClosing(true);
    clearVersionMenuTimer();
    versionMenuTimerRef.current = window.setTimeout(() => {
      versionMenuTimerRef.current = null;
      setIsVersionMenuOpen(false);
      setIsVersionMenuClosing(false);
    }, 110);
  }, [clearVersionMenuTimer, isVersionMenuClosing, isVersionMenuOpen]);

  const handleModalClose = useCallback(() => {
    clearVersionMenuTimer();
    setIsVersionMenuOpen(false);
    setIsVersionMenuClosing(false);
    onClose();
  }, [clearVersionMenuTimer, onClose]);

  const handleBeforeEscape = useCallback(() => {
    if (isVersionMenuOpen) {
      closeVersionMenu();
      return true;
    }
    return false;
  }, [closeVersionMenu, isVersionMenuOpen]);

  const { isClosing, requestClose } = useAnimatedModalClose(handleModalClose, {
    locked: Boolean(downloadingId),
    onBeforeEscape: handleBeforeEscape,
  });

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      searchSeqRef.current += 1;
      clearVersionMenuTimer();
    };
  }, [clearVersionMenuTimer]);

  useEffect(() => {
    if (!isVersionMenuOpen) return;
    const onMouseDown = (e: MouseEvent) => {
      if (
        versionMenuRef.current &&
        !versionMenuRef.current.contains(e.target as Node)
      ) {
        closeVersionMenu();
      }
    };
    document.addEventListener("mousedown", onMouseDown);
    return () => document.removeEventListener("mousedown", onMouseDown);
  }, [closeVersionMenu, isVersionMenuOpen]);

  const normalizedWorkspace = workspaceDir.trim().replace(/[\\/]+$/, "");
  const updatesDownloadDir = normalizedWorkspace
    ? `${normalizedWorkspace}\\downloads\\updates`
    : "downloads\\updates";

  const runSearch = useCallback(
    async (targetQuery: string, filterWin11X64: boolean) => {
      const trimmed = targetQuery.trim();
      if (!trimmed) return;
      const reqId = ++searchSeqRef.current;
      setSearching(true);
      setSearchError(null);
      try {
        const items = await invoke<CatalogUpdateEntry[]>("search_update_catalog", {
          query: trimmed,
          onlyWin11X64: filterWin11X64,
        });
        if (searchSeqRef.current !== reqId) return;
        setResults(items);
        setHasSearched(true);
      } catch (err) {
        if (searchSeqRef.current !== reqId) return;
        setSearchError(String(err));
        setResults([]);
        setHasSearched(true);
      } finally {
        if (searchSeqRef.current === reqId) {
          setSearching(false);
        }
      }
    },
    []
  );

  useEffect(() => {
    runSearch(initialQueryRef.current, true);
  }, [runSearch]);

  useEffect(() => {
    let active = true;
    const unlisten = listen<CatalogDownloadProgress>(
      "catalog-download-progress",
      (event) => {
        if (!active) return;
        if (downloadingIdRef.current !== event.payload.updateId) return;
        setProgress(event.payload);
      }
    );
    return () => {
      active = false;
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const handleSubmitSearch = (e: React.FormEvent) => {
    e.preventDefault();
    runSearch(query, onlyWin11X64);
  };

  const handleQuickPreset = (presetQuery: string) => {
    setQuery(presetQuery);
    runSearch(presetQuery, onlyWin11X64);
  };

  const handleToggleWin11Filter = (checked: boolean) => {
    setOnlyWin11X64(checked);
    if (query.trim()) {
      runSearch(query, checked);
    }
  };

  const handleChangeTargetVersion = (nextVersion: string) => {
    setTargetVersion(nextVersion);
    const nextQuery = `Windows 11 Version ${nextVersion} for x64`;
    setQuery(nextQuery);
    runSearch(nextQuery, onlyWin11X64);
  };

  const handleDownload = async (entry: CatalogUpdateEntry) => {
    if (downloadingId || isBuilding) return;
    downloadingIdRef.current = entry.updateId;
    setDownloadingId(entry.updateId);
    setCancelling(false);
    setProgress(null);
    setDownloadError(null);
    setDownloadSuccess(null);

    try {
      const res = await invoke<CatalogDownloadResult>("download_catalog_update", {
        updateId: entry.updateId,
        title: entry.title,
        expectedSizeBytes: entry.sizeBytes,
      });
      if (!mountedRef.current) return;
      setDownloadedByUpdate((prev) => ({
        ...prev,
        [entry.updateId]: res.downloadedFiles.map((f) => f.path.toLowerCase()),
      }));
      onDownloaded(res.downloadedFiles);
      setDownloadSuccess(
        t("screens.updates.catalogModal.downloadSuccess", {
          count: res.downloadedFiles.length,
        })
      );
    } catch (err) {
      if (!mountedRef.current) return;
      setDownloadError(String(err));
    } finally {
      if (downloadingIdRef.current === entry.updateId) {
        downloadingIdRef.current = null;
      }
      if (mountedRef.current) {
        setDownloadingId(null);
        setCancelling(false);
        setProgress(null);
      }
    }
  };

  const handleCancelDownload = async () => {
    if (!downloadingId || cancelling) return;
    setCancelling(true);
    try {
      await invoke("cancel_catalog_download");
    } catch {
      // ignore
    }
  };

  const handleOpenExternalCatalog = async () => {
    try {
      await invoke("open_external_url", {
        url: "https://www.catalog.update.microsoft.com/",
      });
    } catch {
      // ignore
    }
  };

  const queuedLowerSet = new Set(queuedPaths.map((p) => p.toLowerCase()));

  const isEntryQueued = (entry: CatalogUpdateEntry): boolean => {
    const trackedPaths = downloadedByUpdate[entry.updateId];
    if (trackedPaths && trackedPaths.length > 0) {
      return trackedPaths.every((p) => queuedLowerSet.has(p));
    }
    if (entry.kb) {
      const kbLower = entry.kb.toLowerCase();
      return queuedPaths.some((p) => p.toLowerCase().includes(kbLower));
    }
    return false;
  };

  const quickChips = [
    {
      label: t("screens.updates.catalogModal.quickAll", { version: targetVersion }),
      q: `Windows 11 Version ${targetVersion} for x64`,
    },
    {
      label: t("screens.updates.catalogModal.quickLcu", { version: targetVersion }),
      q: `Cumulative Update Windows 11 Version ${targetVersion} for x64-based Systems`,
    },
    {
      label: t("screens.updates.catalogModal.quickDotNet", { version: targetVersion }),
      q: `.NET Framework Windows 11 Version ${targetVersion} for x64`,
    },
  ];

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestClose}
      maxWidthClass="max-w-[820px]"
      panelClassName="flex flex-col max-h-[85vh]"
    >
      {/* Header */}
        <div className="px-5 py-4 border-b border-border-subtle flex items-center justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-[15.5px] font-semibold text-text-primary truncate">
              {t("screens.updates.catalogModal.title")}
            </h2>
            <p className="text-[12px] text-text-secondary mt-0.5 truncate">
              {t("screens.updates.catalogModal.subtitle", { version: targetVersion })}
            </p>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <button
              type="button"
              onClick={handleOpenExternalCatalog}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.98]"
            >
              <ExternalLink className="w-3.5 h-3.5" strokeWidth={1.6} />
              <span>{t("screens.updates.catalogModal.openCatalogWeb")}</span>
            </button>
            <button
              type="button"
              aria-label={t("modals.closeAria")}
              disabled={Boolean(downloadingId)}
              onClick={requestClose}
              className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.96] disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <X className="w-4 h-4" strokeWidth={1.75} />
            </button>
          </div>
        </div>

        {/* Search & Filters */}
        <div className="px-5 py-3.5 border-b border-border-subtle space-y-2.5 bg-sidebar">
          <form onSubmit={handleSubmitSearch} className="flex items-center gap-2">
            <div ref={versionMenuRef} className="relative shrink-0">
              <button
                type="button"
                aria-label={t("screens.updates.catalogModal.targetVersionLabel")}
                aria-haspopup="listbox"
                aria-expanded={isVersionMenuOpen && !isVersionMenuClosing}
                disabled={searching || Boolean(downloadingId)}
                onClick={() => {
                  if (isVersionMenuOpen && !isVersionMenuClosing) {
                    closeVersionMenu();
                  } else {
                    openVersionMenu();
                  }
                }}
                className={`inline-flex items-center justify-between gap-2 min-w-[82px] px-3 py-1.5 rounded-md bg-canvas border text-[13px] font-semibold text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98] disabled:opacity-60 disabled:cursor-not-allowed disabled:active:scale-100 ${
                  isVersionMenuOpen && !isVersionMenuClosing
                    ? "border-accent"
                    : "border-border-subtle"
                }`}
              >
                <span>{targetVersion}</span>
                <ChevronDown
                  className={`w-3.5 h-3.5 transition-transform duration-150 ${
                    isVersionMenuOpen && !isVersionMenuClosing
                      ? "rotate-180 text-accent"
                      : "text-text-secondary"
                  }`}
                  strokeWidth={1.8}
                />
              </button>

              {isVersionMenuOpen && (
                <div
                  role="listbox"
                  aria-label={t("screens.updates.catalogModal.targetVersionLabel")}
                  className={`${
                    isVersionMenuClosing ? "animate-fluent-menu-exit" : "animate-fluent-menu"
                  } absolute left-0 top-full mt-1.5 z-30 min-w-[112px] p-1 rounded-lg bg-surface border border-border-subtle shadow-xl space-y-0.5`}
                >
                  {WIN11_VERSIONS.map((ver) => {
                    const selected = ver === targetVersion;
                    return (
                      <button
                        key={ver}
                        type="button"
                        role="option"
                        aria-selected={selected}
                        onClick={() => {
                          closeVersionMenu();
                          if (ver !== targetVersion) {
                            handleChangeTargetVersion(ver);
                          }
                        }}
                        className={`relative w-full flex items-center justify-between gap-2 pl-3 pr-2.5 py-1.5 rounded-md text-[12.5px] transition-colors ${
                          selected
                            ? "bg-subtle text-text-primary font-semibold"
                            : "text-text-secondary hover:text-text-primary hover:bg-subtle font-normal"
                        }`}
                      >
                        {selected && (
                          <span
                            className="animate-fluent-pill absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-[14px] rounded-full bg-accent"
                            aria-hidden="true"
                          />
                        )}
                        <span>{ver}</span>
                        {selected && (
                          <Check
                            className="w-3.5 h-3.5 text-accent animate-fluent-icon-pop"
                            strokeWidth={2}
                          />
                        )}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
            <div className="relative flex-1">
              <Search
                className="w-4 h-4 text-text-secondary absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none"
                strokeWidth={1.6}
              />
              <input
                type="text"
                value={query}
                disabled={Boolean(downloadingId)}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("screens.updates.catalogModal.searchPlaceholder")}
                className="w-full pl-9 pr-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[13px] text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent disabled:opacity-60"
              />
            </div>
            <button
              type="submit"
              disabled={searching || Boolean(downloadingId) || !query.trim()}
              className="inline-flex items-center gap-1.5 px-4 py-1.5 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed shrink-0"
            >
              {searching ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" strokeWidth={1.8} />
              ) : (
                <Search className="w-3.5 h-3.5" strokeWidth={1.7} />
              )}
              <span>{t("screens.updates.catalogModal.searchBtn")}</span>
            </button>
          </form>

          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-1.5">
              {quickChips.map((chip) => {
                const active = query.trim().toLowerCase() === chip.q.toLowerCase();
                return (
                  <button
                    key={chip.q}
                    type="button"
                    disabled={searching || Boolean(downloadingId)}
                    onClick={() => handleQuickPreset(chip.q)}
                    className={`px-2.5 py-1 rounded-md text-[11.5px] font-medium border transition-all duration-150 active:scale-[0.98] disabled:opacity-50 ${
                      active
                        ? "bg-accent text-accent-fg border-accent"
                        : "bg-surface text-text-secondary border-border-subtle hover:bg-subtle hover:text-text-primary"
                    }`}
                  >
                    {chip.label}
                  </button>
                );
              })}
            </div>

            <label className="inline-flex items-center gap-2 text-[12px] text-text-secondary cursor-pointer select-none">
              <input
                type="checkbox"
                checked={onlyWin11X64}
                disabled={searching || Boolean(downloadingId)}
                onChange={(e) => handleToggleWin11Filter(e.target.checked)}
                className="accent-accent"
              />
              <span>{t("screens.updates.catalogModal.onlyWin11X64")}</span>
            </label>
          </div>

          <div className="flex items-center gap-2 text-[11.5px] text-text-secondary">
            <FolderOpen className="w-3.5 h-3.5 shrink-0 text-accent" strokeWidth={1.6} />
            <span className="shrink-0">{t("screens.updates.catalogModal.savePathLabel")}</span>
            <span className="font-mono text-text-primary truncate">{updatesDownloadDir}</span>
          </div>
        </div>

        {/* Active Download Progress Banner */}
        {downloadingId && (
          <div className="animate-fluent-item px-5 py-3 border-b border-border-subtle bg-canvas space-y-2">
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="text-[12.5px] font-semibold text-text-primary">
                  {t("screens.updates.catalogModal.downloadProgressTitle")}
                </div>
                <div className="text-[11.5px] text-text-secondary font-mono truncate">
                  {progress
                    ? t("screens.updates.catalogModal.fileProgress", {
                        index: progress.fileIndex,
                        total: progress.totalFiles,
                        name: progress.fileName,
                      })
                    : t("screens.updates.catalogModal.downloadingBtn")}
                </div>
              </div>
              <div className="flex items-center gap-3 shrink-0">
                <span className="text-[12px] font-mono text-text-secondary">
                  {progress && progress.totalBytes > 0
                    ? `${formatBytes(progress.downloadedBytes)} / ${formatBytes(progress.totalBytes)} (${progress.percent.toFixed(0)}%)`
                    : progress
                    ? `${formatBytes(progress.downloadedBytes)} (${t("screens.updates.catalogModal.sizeUnknown")})`
                    : "..."}
                </span>
                <button
                  type="button"
                  disabled={cancelling}
                  onClick={handleCancelDownload}
                  className="px-2.5 py-1 rounded-md text-[12px] font-medium bg-status-danger-bg text-status-danger hover:opacity-90 transition-all duration-150 active:scale-[0.98] disabled:opacity-50"
                >
                  {cancelling
                    ? t("screens.updates.catalogModal.cancelling")
                    : t("screens.updates.catalogModal.cancelDownload")}
                </button>
              </div>
            </div>
            <div className="h-1.5 w-full rounded-full bg-subtle overflow-hidden">
              {progress && progress.totalBytes > 0 ? (
                <div
                  className="h-full bg-accent transition-[width] duration-200 ease-out"
                  style={{
                    width: `${Math.min(100, Math.max(2, progress.percent))}%`,
                  }}
                />
              ) : (
                <div className="h-full w-2/5 rounded-full bg-accent animate-fluent-indeterminate" />
              )}
            </div>
          </div>
        )}

        {/* Notifications */}
        {(searchError || downloadError || downloadSuccess) && (
          <div className="px-5 pt-3 space-y-2">
            {searchError && (
              <div className="animate-fluent-item flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                <div className="flex items-center gap-2 min-w-0">
                  <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                  <span className="break-words">{localizeBackendError(searchError)}</span>
                </div>
                <button
                  type="button"
                  onClick={() => setSearchError(null)}
                  className="p-0.5 rounded hover:bg-subtle shrink-0"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            )}
            {downloadError && (
              <div className="animate-fluent-item flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                <div className="flex items-center gap-2 min-w-0">
                  <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                  <span className="break-words">{localizeBackendError(downloadError)}</span>
                </div>
                <button
                  type="button"
                  onClick={() => setDownloadError(null)}
                  className="p-0.5 rounded hover:bg-subtle shrink-0"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            )}
            {downloadSuccess && (
              <div className="animate-fluent-item flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-status-success-bg text-status-success text-[12px]">
                <div className="flex items-center gap-2 min-w-0">
                  <CheckCircle2 className="w-4 h-4 shrink-0 animate-fluent-icon-pop" strokeWidth={1.75} />
                  <span className="break-words">{downloadSuccess}</span>
                </div>
                <button
                  type="button"
                  onClick={() => setDownloadSuccess(null)}
                  className="p-0.5 rounded hover:bg-subtle shrink-0"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            )}
          </div>
        )}

        {/* Results List */}
        <div className="flex-1 min-h-[260px] overflow-y-auto p-5">
          {searching ? (
            <div className="animate-fluent-item h-full min-h-[220px] flex flex-col items-center justify-center gap-2.5 text-text-secondary text-[13px]">
              <Loader2 className="w-5 h-5 animate-spin text-accent" strokeWidth={1.8} />
              <span>{t("screens.updates.catalogModal.searching")}</span>
            </div>
          ) : hasSearched && results.length === 0 ? (
            <div className="animate-fluent-item h-full min-h-[220px] flex flex-col items-center justify-center text-center px-6 text-[13px] text-text-secondary">
              {t("screens.updates.catalogModal.noResults")}
            </div>
          ) : (
            <div className="border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {results.map((entry, index) => {
                const isThisDownloading = downloadingId === entry.updateId;
                const queued = isEntryQueued(entry);

                return (
                  <div
                    key={entry.updateId}
                    style={{ animationDelay: `${Math.min(index * 20, 200)}ms` }}
                    className="animate-fluent-item px-4 py-3 flex items-center justify-between gap-4 hover:bg-subtle transition-colors"
                  >
                    <div className="min-w-0 flex-1 space-y-1">
                      <div className="flex items-center gap-2 flex-wrap">
                        {entry.kb && (
                          <span className="px-1.5 py-0.5 rounded text-[11px] font-mono font-semibold bg-subtle text-accent shrink-0">
                            {entry.kb}
                          </span>
                        )}
                        <span className="text-[13px] font-semibold text-text-primary leading-snug">
                          {entry.title}
                        </span>
                        {queued && (
                          <span className="animate-fluent-icon-pop inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-success-bg text-status-success shrink-0">
                            <CheckCircle2 className="w-3 h-3" strokeWidth={1.8} />
                            {t("screens.updates.catalogModal.queuedBadge")}
                          </span>
                        )}
                      </div>
                      <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11.5px] text-text-secondary">
                        {entry.products && <span>{entry.products}</span>}
                        {entry.classification && <span>• {entry.classification}</span>}
                        {entry.lastUpdated && <span>• {entry.lastUpdated}</span>}
                        {entry.version && entry.version !== "n/a" && (
                          <span className="font-mono">• v{entry.version}</span>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-3 shrink-0">
                      <span className="text-[12px] font-mono text-text-secondary min-w-[70px] text-right">
                        {entry.sizeText ||
                          (entry.sizeBytes > 0 ? formatBytes(entry.sizeBytes) : "—")}
                      </span>
                      <button
                        type="button"
                        disabled={Boolean(downloadingId) || isBuilding}
                        onClick={() => handleDownload(entry)}
                        className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
                      >
                        {isThisDownloading ? (
                          <>
                            <Loader2
                              className="w-3.5 h-3.5 animate-spin"
                              strokeWidth={1.8}
                            />
                            <span>{t("screens.updates.catalogModal.downloadingBtn")}</span>
                          </>
                        ) : (
                          <>
                            <Download className="w-3.5 h-3.5" strokeWidth={1.7} />
                            <span>{t("screens.updates.catalogModal.downloadBtn")}</span>
                          </>
                        )}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-border-subtle bg-sidebar flex items-center justify-end">
          <button
            type="button"
            disabled={Boolean(downloadingId)}
            onClick={requestClose}
            className="px-4 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {t("screens.updates.catalogModal.close")}
          </button>
        </div>
    </FluentModal>
  );
}
