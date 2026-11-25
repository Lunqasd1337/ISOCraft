import { useState, type Dispatch, type SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import { formatBytes } from "../../utils/format";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  File,
  Folder,
  FolderPlus,
  Plus,
  Trash2,
  X,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { FluentReveal, FluentSegmentedControl } from "../FluentMotion";
import { localizeBackendError } from "../../utils/backendErrors";
import {
  INSTALL_PRESETS,
  ISO_PRESETS,
  defaultDestinationForScan,
  getBaseName,
  validateDestinationClient,
} from "../../utils/destinationValidation";
import type {
  ConflictPolicy,
  FileQueueItem,
  FileScanResult,
  FileTarget,
  FilesQueueState,
} from "../../types/studio";

interface FilesScreenProps {
  hasValidImage: boolean;
  canTargetIso: boolean;
  isBuilding?: boolean;
  filesQueue: FilesQueueState;
  onChangeFilesQueue: Dispatch<SetStateAction<FilesQueueState>>;
}

export function FilesScreen({
  hasValidImage,
  canTargetIso,
  isBuilding = false,
  filesQueue,
  onChangeFilesQueue,
}: FilesScreenProps) {
  const { t } = useTranslation();
  const [scanError, setScanError] = useState<string | null>(null);

  const scanAndAddPaths = async (paths: string[]) => {
    if (isBuilding) return;
    setScanError(null);
    const added: FileQueueItem[] = [];

    for (const p of paths) {
      try {
        const res = await invoke<FileScanResult>("scan_custom_file_path", { path: p });
        const id =
          typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
            ? crypto.randomUUID()
            : `${p}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
        added.push({
          id,
          sourcePath: res.path,
          isDirectory: res.isDirectory,
          fileCount: res.fileCount,
          totalBytes: res.totalBytes,
          target: "installWim",
          destinationPath: defaultDestinationForScan(res),
          conflictPolicy: "overwrite",
        });
      } catch (err) {
        setScanError(String(err));
      }
    }

    if (added.length > 0) {
      onChangeFilesQueue((prev) => ({
        items: [...prev.items, ...added],
      }));
    }
  };

  const handleAddFiles = async () => {
    if (isBuilding) return;
    try {
      const selected = await open({
        multiple: true,
      });
      if (Array.isArray(selected) && selected.length > 0) {
        await scanAndAddPaths(selected);
      }
    } catch (err) {
      setScanError(String(err));
    }
  };

  const handleAddFolder = async () => {
    if (isBuilding) return;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (typeof selected === "string" && selected.trim()) {
        await scanAndAddPaths([selected]);
      }
    } catch (err) {
      setScanError(String(err));
    }
  };

  const updateItem = (id: string, patch: Partial<FileQueueItem>) => {
    if (isBuilding) return;
    onChangeFilesQueue((prev) => ({
      items: prev.items.map((item) =>
        item.id === id ? { ...item, ...patch } : item
      ),
    }));
  };

  const handleTargetChange = (item: FileQueueItem, nextTarget: FileTarget) => {
    if (isBuilding || item.target === nextTarget) return;
    const currentNorm = item.destinationPath.trim().replace(/\//g, "\\").replace(/^\\+|\\+$/g, "").toLowerCase();
    const oppositePresets = (nextTarget === "isoRoot" ? INSTALL_PRESETS : ISO_PRESETS).map((p) =>
      p.toLowerCase()
    );
    const shouldClear = oppositePresets.includes(currentNorm);
    updateItem(item.id, {
      target: nextTarget,
      destinationPath: shouldClear ? "" : item.destinationPath,
    });
  };

  const removeItem = (id: string) => {
    if (isBuilding) return;
    onChangeFilesQueue((prev) => ({
      items: prev.items.filter((item) => item.id !== id),
    }));
  };

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.files.title")}
        subtitle={t("screens.files.subtitle")}
        actions={
          <>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleAddFiles}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <Plus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.files.addFilesBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleAddFolder}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.files.addFolderBtn")}</span>
            </button>
          </>
        }
      />

      {!hasValidImage ? (
        <EmptyStateCard
          icon={FolderPlus}
          title={t("screens.files.emptyTitle")}
          description={t("screens.files.emptyNeedImage")}
        />
      ) : (
        <>
          {scanError && (
            <FluentReveal className="flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px]">
              <div className="flex items-center gap-2 min-w-0">
                <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span className="break-words">
                  {t("screens.files.scanError", {
                    error: localizeBackendError(scanError),
                  })}
                </span>
              </div>
              <button
                type="button"
                onClick={() => setScanError(null)}
                className="p-1 rounded hover:bg-subtle transition-colors shrink-0"
              >
                <X className="w-3.5 h-3.5" strokeWidth={1.7} />
              </button>
            </FluentReveal>
          )}

          {filesQueue.items.length === 0 ? (
            <EmptyStateCard
              icon={FolderPlus}
              title={t("screens.files.emptyTitle")}
              description={t("screens.files.emptyDesc")}
              action={
                <div className="flex flex-wrap items-center justify-center gap-2">
                  <button
                    type="button"
                    onClick={handleAddFiles}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98]"
                  >
                    <Plus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.files.addFilesBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleAddFolder}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                  >
                    <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.files.addFolderBtn")}</span>
                  </button>
                </div>
              }
            />
          ) : (
            <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {filesQueue.items.map((item) => {
                const baseName = getBaseName(item.sourcePath);
                const validation = validateDestinationClient(
                  item.target,
                  item.destinationPath,
                  baseName
                );
                const prefix = item.target === "installWim" ? "C:\\" : "<ISO>\\";
                const relJoined = validation.cleanDir
                  ? `${validation.cleanDir}\\${baseName}`
                  : baseName;
                const previewFull = `${prefix}${relJoined}${item.isDirectory ? "\\" : ""}`;
                const presets = item.target === "installWim" ? INSTALL_PRESETS : ISO_PRESETS;

                return (
                  <FluentReveal key={item.id} className="px-4 py-3.5 space-y-3">
                    {/* Top row: Icon, name, badges, source path, remove */}
                    <div className="flex items-start justify-between gap-4">
                      <div className="flex items-start gap-3 min-w-0">
                        {item.isDirectory ? (
                          <Folder
                            className="w-4 h-4 text-accent shrink-0 mt-0.5"
                            strokeWidth={1.7}
                          />
                        ) : (
                          <File
                            className="w-4 h-4 text-accent shrink-0 mt-0.5"
                            strokeWidth={1.7}
                          />
                        )}
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="text-[13.5px] font-semibold text-text-primary truncate">
                              {baseName}
                            </span>
                            {item.isDirectory && (
                              <span className="px-1.5 py-0.5 rounded text-[11px] font-medium bg-subtle text-text-secondary shrink-0">
                                {t("screens.files.fileCountBadge", {
                                  count: item.fileCount,
                                })}
                              </span>
                            )}
                            <span className="text-[11.5px] text-text-secondary shrink-0">
                              {formatBytes(item.totalBytes)}
                            </span>
                          </div>
                          <div className="text-[11.5px] font-mono text-text-secondary truncate mt-0.5">
                            {item.sourcePath}
                          </div>
                        </div>
                      </div>

                      <button
                        type="button"
                        disabled={isBuilding}
                        onClick={() => removeItem(item.id)}
                        title={t("screens.files.remove")}
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100 shrink-0"
                      >
                        <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                        <span>{t("screens.files.remove")}</span>
                      </button>
                    </div>

                    {/* Controls: Target + Conflict Policy */}
                    <div className="pl-7 grid grid-cols-1 md:grid-cols-2 gap-3">
                      <div className="space-y-1">
                        <FluentSegmentedControl
                          value={item.target}
                          onChange={(nextTarget) => handleTargetChange(item, nextTarget)}
                          variant="surface"
                          size="sm"
                          fullWidth
                          options={[
                            {
                              value: "installWim",
                              label: t("screens.files.targetInstallWim"),
                            },
                            {
                              value: "isoRoot",
                              label: t("screens.files.targetIsoRoot"),
                              disabled: !canTargetIso && item.target !== "isoRoot",
                              title: canTargetIso
                                ? undefined
                                : t("screens.files.isoRootUnavailable"),
                            },
                          ]}
                        />
                      </div>

                      <div className="flex items-center gap-2">
                        <span className="text-[12px] text-text-secondary shrink-0">
                          {t("screens.files.conflictLabel")}:
                        </span>
                        <FluentSegmentedControl<ConflictPolicy>
                          value={item.conflictPolicy}
                          onChange={(pol) => updateItem(item.id, { conflictPolicy: pol })}
                          variant="surface"
                          size="sm"
                          fullWidth
                          className="flex-1"
                          options={[
                            {
                              value: "overwrite",
                              label: t("screens.files.policyOverwrite"),
                            },
                            {
                              value: "skip",
                              label: t("screens.files.policySkip"),
                            },
                          ]}
                        />
                      </div>
                    </div>

                    {/* Destination folder input & quick presets */}
                    <div className="pl-7 space-y-2">
                      <div className="flex items-center gap-2">
                        <span className="text-[12px] text-text-secondary shrink-0">
                          {t("screens.files.destLabel")}:
                        </span>
                        <div className="flex-1 flex items-center rounded-md bg-canvas border border-border-subtle transition-colors duration-150 focus-within:border-accent overflow-hidden">
                          <span className="px-2.5 py-1.5 bg-subtle border-r border-border-subtle text-[12px] font-mono text-text-secondary shrink-0">
                            {prefix}
                          </span>
                          <input
                            type="text"
                            disabled={isBuilding}
                            value={item.destinationPath}
                            onChange={(e) =>
                              updateItem(item.id, { destinationPath: e.target.value })
                            }
                            placeholder={
                              item.target === "installWim"
                                ? t("screens.files.destPlaceholderInstall")
                                : t("screens.files.destPlaceholderIso")
                            }
                            className="flex-1 px-2.5 py-1.5 bg-transparent text-[12px] font-mono text-text-primary focus:outline-none"
                          />
                        </div>
                      </div>

                      <div className="flex flex-wrap items-center gap-1.5">
                        {presets.map((preset) => (
                          <button
                            key={preset}
                            type="button"
                            onClick={() => updateItem(item.id, { destinationPath: preset })}
                            className="px-2 py-0.5 rounded text-[11px] font-mono bg-canvas border border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.97]"
                          >
                            {preset}
                          </button>
                        ))}
                        <button
                          type="button"
                          onClick={() => updateItem(item.id, { destinationPath: "" })}
                          className="px-2 py-0.5 rounded text-[11px] bg-canvas border border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.97]"
                        >
                          {t("screens.files.presetRoot")} ({prefix})
                        </button>
                      </div>

                      <div className="flex items-center gap-2 text-[11.5px] text-text-secondary">
                        <span>{t("screens.files.resultPreviewLabel")}</span>
                        <span className="font-mono text-text-primary break-all">
                          {previewFull}
                        </span>
                      </div>

                      {validation.errorKey && (
                        <FluentReveal className="flex items-center gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                          <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                          <span>
                            {validation.errorKey === "invalidDestTraversal"
                              ? t("screens.files.invalidDestTraversal")
                              : t("screens.files.invalidDestProtected")}
                          </span>
                        </FluentReveal>
                      )}

                      {item.target === "isoRoot" && !canTargetIso && (
                        <FluentReveal className="flex items-center gap-2 px-3 py-2 rounded-md bg-status-warning-bg text-status-warning text-[12px]">
                          <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                          <span>{t("screens.files.isoRootSkippedWarning")}</span>
                        </FluentReveal>
                      )}
                    </div>
                  </FluentReveal>
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}
