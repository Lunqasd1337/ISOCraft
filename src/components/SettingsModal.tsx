import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FluentModal, useAnimatedModalClose } from "./FluentModal";
import { prefersReducedMotion } from "../utils/motion";
import { localizeBackendError } from "../utils/backendErrors";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowUpCircle,
  CheckCircle2,
  Download,
  ExternalLink,
  FolderOpen,
  RefreshCw,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import { formatBytes } from "../utils/format";
import type {
  AppSettings,
  AppUpdateInfo,
  AppUpdateProgress,
  EnvironmentStatus,
  ToolStatus,
  WorkspaceStorageStats,
} from "../types/studio";

const GITHUB_RELEASES_URL = "https://github.com/Lunqasd1337/ISOCraft/releases";

type UpdateStatus =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "installing"
  | "error";

interface SettingsModalProps {
  settings: AppSettings;
  envStatus: EnvironmentStatus | null;
  isBusy?: boolean;
  onRefreshEnv: () => Promise<void>;
  onSave: (updated: AppSettings) => Promise<void>;
  onClose: () => void;
}

export function SettingsModal({
  settings,
  envStatus,
  isBusy = false,
  onRefreshEnv,
  onSave,
  onClose,
}: SettingsModalProps) {
  const { t } = useTranslation();
  const [workspaceDir, setWorkspaceDir] = useState(settings.workspaceDir);
  const [oscdimgPath, setOscdimgPath] = useState(settings.oscdimgPath ?? "");
  const editedWorkspaceRef = useRef(false);
  const editedOscdimgRef = useRef(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!editedWorkspaceRef.current && settings.workspaceDir) {
      setWorkspaceDir(settings.workspaceDir);
    }
  }, [settings.workspaceDir]);

  useEffect(() => {
    if (!editedOscdimgRef.current && settings.oscdimgPath !== undefined) {
      setOscdimgPath(settings.oscdimgPath ?? "");
    }
  }, [settings.oscdimgPath]);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshSpinning, setRefreshSpinning] = useState(false);
  const refreshingRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [storageStats, setStorageStats] = useState<WorkspaceStorageStats | null>(null);
  const [clearingTarget, setClearingTarget] = useState<"cache" | "downloads" | null>(
    null
  );

  const mountedRef = useRef(true);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [updateCheckSpinning, setUpdateCheckSpinning] = useState(false);
  const checkingRef = useRef(false);
  const [availableUpdate, setAvailableUpdate] = useState<AppUpdateInfo | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<{
    percent: number | null;
    downloadedBytes: number;
  }>({ percent: null, downloadedBytes: 0 });
  const [updateError, setUpdateError] = useState<string | null>(null);

  const isUpdateLocked =
    updateStatus === "downloading" || updateStatus === "installing";

  const { isClosing, requestClose } = useAnimatedModalClose(onClose, {
    locked: isUpdateLocked,
  });

  useEffect(() => {
    mountedRef.current = true;
    let active = true;
    let unlistenUpdate: UnlistenFn | null = null;

    getVersion()
      .then((ver) => {
        if (active) setAppVersion(ver);
      })
      .catch(() => {});
    invoke<WorkspaceStorageStats>("get_workspace_storage_stats")
      .then((stats) => {
        if (active) setStorageStats(stats);
      })
      .catch(() => {});

    listen<AppUpdateProgress>("app-update-progress", (event) => {
      if (!active) return;
      const p = event.payload;
      if (p.event === "finished") {
        setDownloadProgress({
          percent: 100,
          downloadedBytes: p.downloadedBytes,
        });
        setUpdateStatus("installing");
      } else {
        setDownloadProgress({
          percent: p.percent,
          downloadedBytes: p.downloadedBytes,
        });
      }
    })
      .then((fn) => {
        if (!active) {
          fn();
        } else {
          unlistenUpdate = fn;
        }
      })
      .catch(() => {});

    return () => {
      active = false;
      mountedRef.current = false;
      unlistenUpdate?.();
    };
  }, []);

  const handleBrowseWorkspace = async () => {
    if (isBusy || isUpdateLocked) return;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: workspaceDir || undefined,
      });
      if (typeof selected === "string" && selected.trim()) {
        editedWorkspaceRef.current = true;
        setWorkspaceDir(selected);
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const handleBrowseOscdimg = async () => {
    if (isBusy || isUpdateLocked) return;
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "oscdimg.exe", extensions: ["exe"] }],
      });
      if (typeof selected === "string" && selected.trim()) {
        editedOscdimgRef.current = true;
        setOscdimgPath(selected);
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const handleRefresh = async () => {
    refreshingRef.current = true;
    setRefreshing(true);
    setRefreshSpinning(true);
    try {
      await onRefreshEnv();
    } finally {
      refreshingRef.current = false;
      if (mountedRef.current) {
        setRefreshing(false);
        if (prefersReducedMotion()) {
          setRefreshSpinning(false);
        }
      }
    }
  };

  const handleCheckUpdates = async () => {
    if (checkingRef.current || isUpdateLocked) return;
    checkingRef.current = true;
    setUpdateCheckSpinning(true);
    setUpdateStatus("checking");
    setUpdateError(null);
    setAvailableUpdate(null);

    try {
      const update = await invoke<AppUpdateInfo | null>("check_app_update");
      if (!mountedRef.current) return;
      if (update) {
        setAvailableUpdate(update);
        setUpdateStatus("available");
      } else {
        setUpdateStatus("up-to-date");
      }
    } catch (err) {
      if (!mountedRef.current) return;
      setUpdateError(err instanceof Error ? err.message : String(err));
      setUpdateStatus("error");
    } finally {
      checkingRef.current = false;
      if (mountedRef.current && prefersReducedMotion()) {
        setUpdateCheckSpinning(false);
      }
    }
  };

  const handleInstallUpdate = async () => {
    if (!availableUpdate || isBusy || isUpdateLocked) return;

    setUpdateStatus("downloading");
    setUpdateError(null);
    setDownloadProgress({ percent: 0, downloadedBytes: 0 });

    try {
      await invoke("install_app_update");
    } catch (err) {
      if (!mountedRef.current) return;
      setUpdateError(err instanceof Error ? err.message : String(err));
      setUpdateStatus("error");
    }
  };

  const isUpdatingBusy =
    updateStatus === "checking" ||
    updateStatus === "downloading" ||
    updateStatus === "installing";

  const handleClearStorage = async (target: "cache" | "downloads") => {
    if (isBusy || clearingTarget || isUpdateLocked) return;
    setError(null);
    setClearingTarget(target);
    try {
      const next = await invoke<WorkspaceStorageStats>("clear_workspace_storage", {
        target,
      });
      if (!mountedRef.current) return;
      setStorageStats(next);
      await onRefreshEnv();
    } catch (err) {
      if (mountedRef.current) {
        setError(String(err));
      }
    } finally {
      if (mountedRef.current) {
        setClearingTarget(null);
      }
    }
  };

  const handleSave = async () => {
    if (isBusy || saving || isUpdateLocked) return;
    setError(null);
    setSaving(true);
    try {
      await onSave({
        ...settings,
        workspaceDir: workspaceDir.trim(),
        oscdimgPath: oscdimgPath.trim() ? oscdimgPath.trim() : null,
      });
      await onRefreshEnv();
      if (mountedRef.current) {
        requestClose();
      }
    } catch (err) {
      if (mountedRef.current) {
        setError(String(err));
      }
    } finally {
      if (mountedRef.current) {
        setSaving(false);
      }
    }
  };

  const formatSource = (source: string) => {
    switch (source) {
      case "system":
        return t("settings.sourceSystem");
      case "bundled":
        return t("settings.sourceBundled");
      case "adk":
        return t("settings.sourceAdk");
      case "settings":
        return t("settings.sourceSettings");
      case "path":
        return t("settings.sourcePath");
      default:
        return source;
    }
  };

  const renderToolRow = (name: string, tool?: ToolStatus) => (
    <div
      key={name}
      className="py-2 px-3 rounded-md bg-subtle border border-border-subtle space-y-1"
    >
      <div className="flex items-center justify-between gap-3">
        <div className="text-[13px] font-medium font-mono text-text-primary truncate">
          {name}
        </div>
        {tool?.found ? (
          <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium bg-status-success-bg text-status-success shrink-0">
            <CheckCircle2 className="w-3.5 h-3.5" strokeWidth={1.75} />
            {t("settings.toolFound")} ({formatSource(tool.source)})
          </span>
        ) : (
          <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium bg-status-danger-bg text-status-danger shrink-0">
            <XCircle className="w-3.5 h-3.5" strokeWidth={1.75} />
            {t("settings.toolMissing")}
          </span>
        )}
      </div>
      {tool?.path && (
        <div
          title={tool.path}
          className="text-[11px] font-mono text-text-secondary break-all select-text"
        >
          {tool.path}
        </div>
      )}
    </div>
  );

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestClose}
      maxWidthClass="max-w-[600px]"
      panelClassName="flex flex-col max-h-[85vh]"
    >
      {/* Header */}
        <div className="px-5 py-4 border-b border-border-subtle flex items-start justify-between">
          <div>
            <h2 className="text-[16px] font-semibold text-text-primary">
              {t("settings.title")}
            </h2>
            <p className="text-[12.5px] text-text-secondary mt-0.5">
              {t("settings.subtitle")}
            </p>
          </div>
          <button
            type="button"
            onClick={requestClose}
            disabled={isUpdateLocked}
            aria-label={t("settings.cancel")}
            className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.95] disabled:opacity-40 disabled:active:scale-100"
          >
            <X className="w-4 h-4" strokeWidth={1.75} />
          </button>
        </div>

        {/* Body */}
        <div className="p-5 space-y-5 overflow-y-auto">
          {/* Workspace Directory */}
          <div className="space-y-2">
            <label className="block text-[13px] font-semibold text-text-primary">
              {t("settings.workspaceLabel")}
            </label>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={workspaceDir}
                disabled={isBusy || isUpdateLocked}
                onChange={(e) => {
                  editedWorkspaceRef.current = true;
                  setWorkspaceDir(e.target.value);
                }}
                className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent disabled:opacity-60"
              />
              <button
                type="button"
                disabled={isBusy || isUpdateLocked}
                onClick={handleBrowseWorkspace}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:active:scale-100 shrink-0"
              >
                <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                {t("settings.browse")}
              </button>
            </div>
            <p className="text-[12px] text-text-secondary">
              {t("settings.workspaceHint")}
            </p>
            {envStatus && envStatus.freeSpaceGb > 0 && (
              <div
                className={`flex items-center gap-2 px-3 py-2 rounded-md text-[12px] ${
                  envStatus.lowSpaceWarning
                    ? "bg-status-warning-bg text-status-warning"
                    : "bg-subtle text-text-secondary"
                }`}
              >
                {envStatus.lowSpaceWarning && (
                  <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                )}
                <span>
                  {t("settings.freeSpaceLabel", {
                    gb: envStatus.freeSpaceGb.toFixed(1),
                  })}
                  {envStatus.lowSpaceWarning ? ` — ${t("settings.lowSpaceWarning")}` : ""}
                </span>
              </div>
            )}
          </div>

          {/* oscdimg.exe optional path */}
          <div className="space-y-2">
            <label className="block text-[13px] font-semibold text-text-primary">
              {t("settings.oscdimgLabel")}
            </label>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={oscdimgPath}
                disabled={isBusy || isUpdateLocked}
                placeholder="C:\Program Files (x86)\Windows Kits\10\...\oscdimg.exe"
                onChange={(e) => {
                  editedOscdimgRef.current = true;
                  setOscdimgPath(e.target.value);
                }}
                className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent disabled:opacity-60"
              />
              <button
                type="button"
                disabled={isBusy || isUpdateLocked}
                onClick={handleBrowseOscdimg}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:active:scale-100 shrink-0"
              >
                {t("settings.browse")}
              </button>
              {oscdimgPath && (
                <button
                  type="button"
                  disabled={isBusy || isUpdateLocked}
                  onClick={() => {
                    editedOscdimgRef.current = true;
                    setOscdimgPath("");
                  }}
                  className="px-2.5 py-1.5 rounded-md text-[12px] text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:active:scale-100 shrink-0"
                >
                  {t("settings.clear")}
                </button>
              )}
            </div>
            <p className="text-[12px] text-text-secondary">
              {t("settings.oscdimgHint")}
            </p>
          </div>

          {/* Tool Status */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-[13px] font-semibold text-text-primary">
                {t("settings.toolsTitle")}
              </span>
              <button
                type="button"
                onClick={handleRefresh}
                disabled={refreshing}
                className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.97] disabled:active:scale-100"
              >
                <RefreshCw
                  onAnimationIteration={() => {
                    if (!refreshingRef.current) {
                      setRefreshSpinning(false);
                    }
                  }}
                  className={`w-3.5 h-3.5 ${
                    refreshSpinning ? "animate-fluent-spin-loop" : ""
                  }`}
                  strokeWidth={1.6}
                />
                {t("settings.refresh")}
              </button>
            </div>
            <div className="space-y-1.5">
              {renderToolRow("dism.exe", envStatus?.dism)}
              {renderToolRow("wimlib-imagex.exe", envStatus?.wimlib)}
              {renderToolRow("xorriso.exe", envStatus?.xorriso)}
              {renderToolRow("oscdimg.exe", envStatus?.oscdimg)}
            </div>
            <p className="text-[11.5px] text-text-secondary font-mono">
              {t("settings.toolsFetchHint")}
            </p>
          </div>

          {/* GitHub App Updates */}
          <div className="space-y-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[13px] font-semibold text-text-primary">
                {t("settings.updatesTitle")}
              </span>
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={() =>
                    invoke("open_external_url", { url: GITHUB_RELEASES_URL }).catch(
                      (err) => setError(String(err))
                    )
                  }
                  className="inline-flex items-center gap-1 px-2 py-1 rounded text-[12px] font-medium text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98]"
                >
                  <ExternalLink className="w-3.5 h-3.5" strokeWidth={1.6} />
                  {t("settings.updatesOpenReleases")}
                </button>
                <button
                  type="button"
                  onClick={handleCheckUpdates}
                  disabled={isUpdatingBusy}
                  className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.97] disabled:opacity-50 disabled:active:scale-100"
                >
                  <RefreshCw
                    onAnimationIteration={() => {
                      if (!checkingRef.current) {
                        setUpdateCheckSpinning(false);
                      }
                    }}
                    className={`w-3.5 h-3.5 ${
                      updateCheckSpinning ? "animate-fluent-spin-loop" : ""
                    }`}
                    strokeWidth={1.6}
                  />
                  {updateStatus === "checking"
                    ? t("settings.updatesChecking")
                    : t("settings.updatesCheckBtn")}
                </button>
              </div>
            </div>

            {updateStatus === "idle" && (
              <div className="px-3 py-2 rounded-md bg-subtle border border-border-subtle text-[12px] text-text-secondary">
                {t("settings.updatesIdleHint")}
              </div>
            )}

            {updateStatus === "up-to-date" && (
              <div className="animate-fluent-item flex items-center gap-2 px-3 py-2 rounded-md bg-status-success-bg text-status-success text-[12px] font-medium">
                <CheckCircle2 className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span>
                  {t("settings.updatesUpToDate", {
                    version: appVersion ?? "",
                  })}
                </span>
              </div>
            )}

            {updateStatus === "available" && availableUpdate && (
              <div className="animate-fluent-item p-3 rounded-md bg-subtle border border-accent/40 space-y-2.5">
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2 min-w-0">
                    <ArrowUpCircle
                      className="w-4 h-4 text-accent shrink-0"
                      strokeWidth={1.75}
                    />
                    <span className="text-[12.5px] font-semibold text-text-primary truncate">
                      {t("settings.updatesAvailable", {
                        version: availableUpdate.version,
                      })}
                    </span>
                  </div>
                  <button
                    type="button"
                    onClick={handleInstallUpdate}
                    disabled={isBusy}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:active:scale-100 shrink-0"
                  >
                    <Download className="w-3.5 h-3.5" strokeWidth={1.75} />
                    {t("settings.updatesInstallBtn")}
                  </button>
                </div>
                {availableUpdate.body && (
                  <pre className="max-h-24 overflow-y-auto whitespace-pre-wrap text-[11.5px] font-sans text-text-secondary bg-canvas border border-border-subtle rounded p-2">
                    {availableUpdate.body}
                  </pre>
                )}
                {isBusy && (
                  <p className="text-[11.5px] text-status-warning">
                    {t("settings.updatesBuildingHint")}
                  </p>
                )}
              </div>
            )}

            {(updateStatus === "downloading" || updateStatus === "installing") && (
              <div className="animate-fluent-item p-3 rounded-md bg-subtle border border-border-subtle space-y-2">
                <div className="flex items-center justify-between text-[12px] font-medium text-text-primary">
                  <span>
                    {updateStatus === "installing"
                      ? t("settings.updatesInstalling")
                      : downloadProgress.percent !== null
                      ? t("settings.updatesDownloading", {
                          percent: downloadProgress.percent,
                        })
                      : t("settings.updatesDownloadingBytes", {
                          mb: (downloadProgress.downloadedBytes / (1024 * 1024)).toFixed(1),
                        })}
                  </span>
                </div>
                <div className="w-full h-1.5 rounded-full bg-canvas overflow-hidden">
                  {updateStatus === "installing" || downloadProgress.percent === null ? (
                    <div className="h-full w-2/5 rounded-full bg-accent animate-fluent-indeterminate" />
                  ) : (
                    <div
                      className="h-full bg-accent transition-[width] duration-150 ease-out"
                      style={{
                        width: `${Math.min(100, Math.max(2, downloadProgress.percent))}%`,
                      }}
                    />
                  )}
                </div>
              </div>
            )}

            {updateStatus === "error" && updateError && (
              <div className="animate-fluent-item flex items-start gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                <XCircle className="w-4 h-4 shrink-0 mt-0.5" strokeWidth={1.75} />
                <span className="break-words">
                  {t("settings.updatesError", {
                    error: localizeBackendError(updateError),
                  })}
                </span>
              </div>
            )}
          </div>

          {/* Cache & Downloads Storage */}
          <div className="space-y-2">
            <span className="block text-[13px] font-semibold text-text-primary">
              {t("settings.storageTitle")}
            </span>
            <div className="space-y-1.5">
              {(
                [
                  {
                    target: "cache" as const,
                    label: t("settings.storageCacheLabel"),
                    bytes: storageStats?.cacheBytes ?? 0,
                    files: storageStats?.cacheFiles ?? 0,
                  },
                  {
                    target: "downloads" as const,
                    label: t("settings.storageDownloadsLabel"),
                    bytes: storageStats?.downloadsBytes ?? 0,
                    files: storageStats?.downloadsFiles ?? 0,
                  },
                ] as const
              ).map((row) => {
                const isEmpty = row.bytes === 0 && row.files === 0;
                const isClearing = clearingTarget === row.target;
                return (
                  <div
                    key={row.target}
                    className="py-2 px-3 rounded-md bg-subtle border border-border-subtle flex items-center justify-between gap-3"
                  >
                    <div className="min-w-0">
                      <div className="text-[12.5px] font-medium text-text-primary truncate">
                        {row.label}
                      </div>
                      <div className="text-[11.5px] font-mono text-text-secondary">
                        {isEmpty
                          ? t("settings.storageEmpty")
                          : `${formatBytes(row.bytes)} • ${t("settings.storageFilesCount", {
                              count: row.files,
                            })}`}
                      </div>
                    </div>
                    <button
                      type="button"
                      disabled={isBusy || clearingTarget !== null || isEmpty || isUpdateLocked}
                      onClick={() => handleClearStorage(row.target)}
                      className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium bg-surface border border-border-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-45 disabled:cursor-not-allowed disabled:active:scale-100 shrink-0"
                    >
                      <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                      <span>
                        {isClearing
                          ? t("settings.storageClearing")
                          : t("settings.storageClearBtn")}
                      </span>
                    </button>
                  </div>
                );
              })}
            </div>
          </div>

          {error && (
            <div className="animate-fluent-item px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12.5px]">
              {localizeBackendError(error)}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="px-5 py-3.5 border-t border-border-subtle bg-sidebar flex items-center justify-between gap-3">
          <span className="text-[12px] font-mono text-text-secondary select-all">
            {appVersion
              ? `${t("app.name")} v${appVersion} • © 2026 Lunqasd1337`
              : `${t("app.name")} • © 2026 Lunqasd1337`}
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={requestClose}
              disabled={isUpdateLocked}
              className="px-4 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98] disabled:opacity-40 disabled:active:scale-100"
            >
              {t("settings.cancel")}
            </button>
            <button
              type="button"
              onClick={handleSave}
              disabled={isBusy || saving || isUpdateLocked}
              className="px-4 py-1.5 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:active:scale-100"
            >
              {t("settings.save")}
            </button>
          </div>
        </div>
    </FluentModal>
  );
}
