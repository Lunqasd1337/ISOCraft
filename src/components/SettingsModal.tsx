import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import {
  AlertTriangle,
  ArrowUpCircle,
  CheckCircle2,
  Download,
  ExternalLink,
  FolderOpen,
  RefreshCw,
  X,
  XCircle,
} from "lucide-react";
import type { AppSettings, EnvironmentStatus, ToolStatus } from "../types/studio";

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
  isBuilding?: boolean;
  onRefreshEnv: () => Promise<void>;
  onSave: (updated: AppSettings) => Promise<void>;
  onClose: () => void;
}

export function SettingsModal({
  settings,
  envStatus,
  isBuilding = false,
  onRefreshEnv,
  onSave,
  onClose,
}: SettingsModalProps) {
  const { t } = useTranslation();
  const [workspaceDir, setWorkspaceDir] = useState(settings.workspaceDir);
  const [oscdimgPath, setOscdimgPath] = useState(settings.oscdimgPath ?? "");
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshTick, setRefreshTick] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);

  const pendingUpdateRef = useRef<Update | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [updateCheckTick, setUpdateCheckTick] = useState(0);
  const [availableUpdate, setAvailableUpdate] = useState<{
    version: string;
    date?: string;
    body?: string;
  } | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<{
    percent: number | null;
    downloadedBytes: number;
  }>({ percent: null, downloadedBytes: 0 });
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [isClosing, setIsClosing] = useState(false);
  const closeTimerRef = useRef<number | null>(null);

  const isUpdateLocked =
    updateStatus === "downloading" || updateStatus === "installing";

  const requestClose = useCallback(() => {
    if (isUpdateLocked || isClosing) return;
    setIsClosing(true);
    closeTimerRef.current = window.setTimeout(() => {
      onClose();
    }, 140);
  }, [isUpdateLocked, isClosing, onClose]);

  useEffect(() => {
    return () => {
      if (closeTimerRef.current !== null) {
        window.clearTimeout(closeTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        requestClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [requestClose]);

  useEffect(() => {
    let active = true;
    getVersion()
      .then((ver) => {
        if (active) setAppVersion(ver);
      })
      .catch(() => {});
    return () => {
      active = false;
      pendingUpdateRef.current?.close().catch(() => {});
      pendingUpdateRef.current = null;
    };
  }, []);

  const handleBrowseWorkspace = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: workspaceDir || undefined,
    });
    if (typeof selected === "string" && selected.trim()) {
      setWorkspaceDir(selected);
    }
  };

  const handleBrowseOscdimg = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "oscdimg.exe", extensions: ["exe"] }],
    });
    if (typeof selected === "string" && selected.trim()) {
      setOscdimgPath(selected);
    }
  };

  const handleRefresh = async () => {
    setRefreshTick((t) => t + 1);
    setRefreshing(true);
    try {
      await onRefreshEnv();
    } finally {
      setRefreshing(false);
    }
  };

  const handleCheckUpdates = async () => {
    setUpdateCheckTick((v) => v + 1);
    setUpdateStatus("checking");
    setUpdateError(null);
    setAvailableUpdate(null);

    await pendingUpdateRef.current?.close().catch(() => {});
    pendingUpdateRef.current = null;

    try {
      const update = await check();
      if (update) {
        pendingUpdateRef.current = update;
        setAvailableUpdate({
          version: update.version,
          date: update.date,
          body: update.body,
        });
        setUpdateStatus("available");
      } else {
        setUpdateStatus("up-to-date");
      }
    } catch (err) {
      setUpdateError(err instanceof Error ? err.message : String(err));
      setUpdateStatus("error");
    }
  };

  const handleInstallUpdate = async () => {
    const update = pendingUpdateRef.current;
    if (!update || isBuilding) return;

    setUpdateStatus("downloading");
    setUpdateError(null);
    setDownloadProgress({ percent: 0, downloadedBytes: 0 });

    let downloaded = 0;
    let total: number | null = null;

    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? null;
            setDownloadProgress({
              percent: total ? 0 : null,
              downloadedBytes: 0,
            });
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            setDownloadProgress({
              percent:
                total && total > 0
                  ? Math.min(100, Math.round((downloaded / total) * 100))
                  : null,
              downloadedBytes: downloaded,
            });
            break;
          case "Finished":
            setDownloadProgress({ percent: 100, downloadedBytes: downloaded });
            setUpdateStatus("installing");
            break;
        }
      });

      pendingUpdateRef.current = null;
      await relaunch().catch(() => {});
    } catch (err) {
      setUpdateError(err instanceof Error ? err.message : String(err));
      setUpdateStatus("error");
    }
  };

  const isUpdatingBusy =
    updateStatus === "checking" ||
    updateStatus === "downloading" ||
    updateStatus === "installing";

  const handleSave = async () => {
    setError(null);
    setSaving(true);
    try {
      await onSave({
        ...settings,
        workspaceDir: workspaceDir.trim(),
        oscdimgPath: oscdimgPath.trim() ? oscdimgPath.trim() : null,
      });
      await onRefreshEnv();
      requestClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
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
      className="flex items-center justify-between gap-3 py-2 px-3 rounded-md bg-subtle border border-border-subtle"
    >
      <div className="min-w-0">
        <div className="text-[13px] font-medium font-mono text-text-primary">
          {name}
        </div>
        {tool?.path && (
          <div className="text-[11px] font-mono text-text-secondary truncate">
            {tool.path}
          </div>
        )}
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
  );

  return createPortal(
    <div
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
      className={`${isClosing ? "animate-fluent-backdrop-exit" : "animate-fluent-backdrop"} fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4`}
    >
      <div
        className={`${isClosing ? "animate-fluent-modal-exit" : "animate-fluent-modal"} w-full max-w-[600px] bg-surface border border-border-subtle rounded-lg shadow-xl overflow-hidden flex flex-col max-h-[85vh]`}
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
            className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors disabled:opacity-40"
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
                onChange={(e) => setWorkspaceDir(e.target.value)}
                className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary focus:outline-none focus:border-accent"
              />
              <button
                type="button"
                onClick={handleBrowseWorkspace}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors shrink-0"
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
                placeholder="C:\Program Files (x86)\Windows Kits\10\...\oscdimg.exe"
                onChange={(e) => setOscdimgPath(e.target.value)}
                className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary focus:outline-none focus:border-accent"
              />
              <button
                type="button"
                onClick={handleBrowseOscdimg}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors shrink-0"
              >
                {t("settings.browse")}
              </button>
              {oscdimgPath && (
                <button
                  type="button"
                  onClick={() => setOscdimgPath("")}
                  className="px-2.5 py-1.5 rounded-md text-[12px] text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors shrink-0"
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
                className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.97]"
              >
                <RefreshCw
                  key={refreshTick}
                  className={`w-3.5 h-3.5 ${
                    refreshing
                      ? "animate-spin"
                      : refreshTick > 0
                      ? "animate-fluent-icon-spin"
                      : ""
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
                  className="inline-flex items-center gap-1 px-2 py-1 rounded text-[12px] font-medium text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
                >
                  <ExternalLink className="w-3.5 h-3.5" strokeWidth={1.6} />
                  {t("settings.updatesOpenReleases")}
                </button>
                <button
                  type="button"
                  onClick={handleCheckUpdates}
                  disabled={isUpdatingBusy}
                  className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.97] disabled:opacity-50"
                >
                  <RefreshCw
                    key={updateCheckTick}
                    className={`w-3.5 h-3.5 ${
                      updateStatus === "checking"
                        ? "animate-spin"
                        : updateCheckTick > 0
                        ? "animate-fluent-icon-spin"
                        : ""
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
                    version: appVersion ?? "0.2.0",
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
                    disabled={isBuilding}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 shrink-0"
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
                {isBuilding && (
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
                  <div
                    className="h-full bg-accent transition-all duration-150"
                    style={{
                      width: `${
                        updateStatus === "installing"
                          ? 100
                          : downloadProgress.percent ?? 100
                      }%`,
                    }}
                  />
                </div>
              </div>
            )}

            {updateStatus === "error" && updateError && (
              <div className="animate-fluent-item flex items-start gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                <XCircle className="w-4 h-4 shrink-0 mt-0.5" strokeWidth={1.75} />
                <span className="break-words">
                  {t("settings.updatesError", { error: updateError })}
                </span>
              </div>
            )}
          </div>

          {error && (
            <div className="animate-fluent-item px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12.5px]">
              {error}
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
              className="px-4 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-colors disabled:opacity-40"
            >
              {t("settings.cancel")}
            </button>
            <button
              type="button"
              onClick={handleSave}
              disabled={saving || isUpdateLocked}
              className="px-4 py-1.5 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50"
            >
              {t("settings.save")}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
