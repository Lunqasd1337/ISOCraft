import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  CheckCircle2,
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
}

const WIN11_VERSIONS = ["26H2", "25H2", "24H2", "23H2", "22H2", "21H2"] as const;

function resolveWin11Version(buildNumber: number | null): string {
  if (buildNumber === null) return "26H2";
  if (buildNumber >= 26300) return "26H2";
  if (buildNumber >= 26200) return "25H2";
  if (buildNumber >= 26100) return "24H2";
  if (buildNumber >= 22631) return "23H2";
  if (buildNumber >= 22621) return "22H2";
  if (buildNumber >= 22000) return "21H2";
  return "26H2";
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`;
  }
  return `${bytes} B`;
}

export function CatalogModal({
  editionBuildNumber,
  workspaceDir,
  queuedPaths,
  onDownloaded,
  onClose,
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

  const searchSeqRef = useRef(0);

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
    const unlisten = listen<CatalogDownloadProgress>(
      "catalog-download-progress",
      (event) => {
        setProgress(event.payload);
      }
    );
    return () => {
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
    if (downloadingId) return;
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
      setDownloadError(String(err));
    } finally {
      setDownloadingId(null);
      setCancelling(false);
      setProgress(null);
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
      q: `Cumulative Update for Windows 11 Version ${targetVersion} for x64`,
    },
    {
      label: t("screens.updates.catalogModal.quickDotNet", { version: targetVersion }),
      q: `.NET Framework Windows 11 Version ${targetVersion} for x64`,
    },
  ];

  return (
    <div className="animate-fluent-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4">
      <div className="animate-fluent-modal w-full max-w-[820px] bg-surface border border-border-subtle rounded-lg shadow-xl overflow-hidden flex flex-col max-h-[90vh]">
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
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-secondary hover:text-text-primary transition-colors"
            >
              <ExternalLink className="w-3.5 h-3.5" strokeWidth={1.6} />
              <span>{t("screens.updates.catalogModal.openCatalogWeb")}</span>
            </button>
            <button
              type="button"
              disabled={Boolean(downloadingId)}
              onClick={onClose}
              className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <X className="w-4 h-4" strokeWidth={1.75} />
            </button>
          </div>
        </div>

        {/* Search & Filters */}
        <div className="px-5 py-3.5 border-b border-border-subtle space-y-2.5 bg-sidebar">
          <form onSubmit={handleSubmitSearch} className="flex items-center gap-2">
            <select
              aria-label={t("screens.updates.catalogModal.targetVersionLabel")}
              value={targetVersion}
              disabled={searching || Boolean(downloadingId)}
              onChange={(e) => handleChangeTargetVersion(e.target.value)}
              className="px-2.5 py-1.5 rounded-md bg-canvas border border-border-subtle text-[13px] font-medium text-text-primary focus:outline-none focus:border-accent disabled:opacity-60 shrink-0"
            >
              {WIN11_VERSIONS.map((ver) => (
                <option key={ver} value={ver}>
                  {ver}
                </option>
              ))}
            </select>
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
                className="w-full pl-9 pr-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[13px] text-text-primary focus:outline-none focus:border-accent disabled:opacity-60"
              />
            </div>
            <button
              type="submit"
              disabled={searching || Boolean(downloadingId) || !query.trim()}
              className="inline-flex items-center gap-1.5 px-4 py-1.5 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 disabled:cursor-not-allowed shrink-0"
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
                    className={`px-2.5 py-1 rounded-md text-[11.5px] font-medium border transition-colors disabled:opacity-50 ${
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
          <div className="px-5 py-3 border-b border-border-subtle bg-canvas space-y-2">
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
                  className="px-2.5 py-1 rounded-md text-[12px] font-medium bg-status-danger-bg text-status-danger hover:opacity-90 disabled:opacity-50"
                >
                  {cancelling
                    ? t("screens.updates.catalogModal.cancelling")
                    : t("screens.updates.catalogModal.cancelDownload")}
                </button>
              </div>
            </div>
            <div className="h-1.5 w-full rounded-full bg-subtle overflow-hidden">
              <div
                className="h-full bg-accent transition-all duration-150"
                style={{
                  width: `${
                    progress && progress.totalBytes > 0
                      ? Math.min(100, Math.max(2, progress.percent))
                      : 15
                  }%`,
                }}
              />
            </div>
          </div>
        )}

        {/* Notifications */}
        {(searchError || downloadError || downloadSuccess) && (
          <div className="px-5 pt-3 space-y-2">
            {searchError && (
              <div className="flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                <div className="flex items-center gap-2 min-w-0">
                  <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                  <span className="break-words">{searchError}</span>
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
              <div className="flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                <div className="flex items-center gap-2 min-w-0">
                  <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                  <span className="break-words">{downloadError}</span>
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
              <div className="flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-status-success-bg text-status-success text-[12px]">
                <div className="flex items-center gap-2 min-w-0">
                  <CheckCircle2 className="w-4 h-4 shrink-0" strokeWidth={1.75} />
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
            <div className="h-full min-h-[220px] flex flex-col items-center justify-center gap-2.5 text-text-secondary text-[13px]">
              <Loader2 className="w-5 h-5 animate-spin text-accent" strokeWidth={1.8} />
              <span>{t("screens.updates.catalogModal.searching")}</span>
            </div>
          ) : hasSearched && results.length === 0 ? (
            <div className="h-full min-h-[220px] flex flex-col items-center justify-center text-center px-6 text-[13px] text-text-secondary">
              {t("screens.updates.catalogModal.noResults")}
            </div>
          ) : (
            <div className="border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {results.map((entry) => {
                const isThisDownloading = downloadingId === entry.updateId;
                const queued = isEntryQueued(entry);

                return (
                  <div
                    key={entry.updateId}
                    className="px-4 py-3 flex items-center justify-between gap-4 hover:bg-subtle transition-colors"
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
                          <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-success-bg text-status-success shrink-0">
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
                        disabled={Boolean(downloadingId)}
                        onClick={() => handleDownload(entry)}
                        className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
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
            onClick={onClose}
            className="px-4 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {t("screens.updates.catalogModal.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
