import { useState } from "react";
import { useTranslation } from "react-i18next";
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
  filesQueue: FilesQueueState;
  onChangeFilesQueue: (next: FilesQueueState) => void;
}

const INSTALL_PRESETS = [
  "Windows\\Setup\\Scripts",
  "Users\\Default\\Desktop",
  "ProgramData",
  "Tools",
];

const ISO_PRESETS = ["sources\\$OEM$", "Soft"];

const PROTECTED_ISO_FILES = new Set([
  "sources\\install.wim",
  "sources\\install.esd",
  "sources\\boot.wim",
  "boot\\etfsboot.com",
  "efi\\microsoft\\boot\\efisys.bin",
  "efi\\microsoft\\boot\\efisys_noprompt.bin",
  "bootmgr",
  "bootmgr.efi",
]);

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

function getBaseName(fullPath: string): string {
  const parts = fullPath.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? fullPath;
}

function defaultDestinationForScan(scan: FileScanResult): string {
  if (scan.isDirectory) {
    return "";
  }
  const base = getBaseName(scan.path).toLowerCase();
  if (base.endsWith(".cmd") || base.endsWith(".bat") || base.endsWith(".ps1")) {
    return "Windows\\Setup\\Scripts";
  }
  return "Tools";
}

function validateDestinationClient(
  target: FileTarget,
  rawDest: string,
  baseName: string
): { errorKey: "invalidDestTraversal" | "invalidDestProtected" | null; cleanDir: string } {
  const unified = rawDest.trim().replace(/\//g, "\\");
  if (unified.startsWith("\\\\") || unified.includes(":") || /[<>"|?*\u0000-\u001f]/.test(unified)) {
    return { errorKey: "invalidDestTraversal", cleanDir: unified };
  }

  const stripped = unified.replace(/^\\+|\\+$/g, "");
  if (stripped.length > 0) {
    const segments = stripped.split("\\");
    for (const seg of segments) {
      if (seg === "" || seg === "." || seg === ".." || seg.replace(/[. ]+$/g, "") === "") {
        return { errorKey: "invalidDestTraversal", cleanDir: stripped };
      }
    }
  }

  const fullRel = stripped ? `${stripped}\\${baseName}` : baseName;
  const norm = fullRel.toLowerCase();

  if (target === "installWim") {
    if (
      norm === "windows\\system32\\config" ||
      norm.startsWith("windows\\system32\\config\\") ||
      norm === "users\\default\\ntuser.dat"
    ) {
      return { errorKey: "invalidDestProtected", cleanDir: stripped };
    }
  } else {
    if (
      PROTECTED_ISO_FILES.has(norm) ||
      /^sources\\install[^\\]*\.swm$/i.test(norm)
    ) {
      return { errorKey: "invalidDestProtected", cleanDir: stripped };
    }
  }

  return { errorKey: null, cleanDir: stripped };
}

export function FilesScreen({
  hasValidImage,
  canTargetIso,
  filesQueue,
  onChangeFilesQueue,
}: FilesScreenProps) {
  const { t } = useTranslation();
  const [scanError, setScanError] = useState<string | null>(null);

  const scanAndAddPaths = async (paths: string[]) => {
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
        setScanError(t("screens.files.scanError", { error: String(err) }));
      }
    }

    if (added.length > 0) {
      onChangeFilesQueue({
        items: [...filesQueue.items, ...added],
      });
    }
  };

  const handleAddFiles = async () => {
    const selected = await open({
      multiple: true,
    });
    if (Array.isArray(selected) && selected.length > 0) {
      await scanAndAddPaths(selected);
    }
  };

  const handleAddFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (typeof selected === "string" && selected.trim()) {
      await scanAndAddPaths([selected]);
    }
  };

  const updateItem = (id: string, patch: Partial<FileQueueItem>) => {
    onChangeFilesQueue({
      items: filesQueue.items.map((item) =>
        item.id === id ? { ...item, ...patch } : item
      ),
    });
  };

  const handleTargetChange = (item: FileQueueItem, nextTarget: FileTarget) => {
    if (item.target === nextTarget) return;
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
    onChangeFilesQueue({
      items: filesQueue.items.filter((item) => item.id !== id),
    });
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
              disabled={!hasValidImage}
              onClick={handleAddFiles}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Plus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.files.addFilesBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage}
              onClick={handleAddFolder}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
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
            <div className="flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px]">
              <div className="flex items-center gap-2 min-w-0">
                <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span className="break-words">{scanError}</span>
              </div>
              <button
                type="button"
                onClick={() => setScanError(null)}
                className="p-1 rounded hover:bg-subtle shrink-0"
              >
                <X className="w-3.5 h-3.5" strokeWidth={1.7} />
              </button>
            </div>
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
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors"
                  >
                    <Plus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.files.addFilesBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleAddFolder}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors"
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
                  <div key={item.id} className="px-4 py-3.5 space-y-3">
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
                        onClick={() => removeItem(item.id)}
                        title={t("screens.files.remove")}
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-colors shrink-0"
                      >
                        <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                        <span>{t("screens.files.remove")}</span>
                      </button>
                    </div>

                    {/* Controls: Target + Conflict Policy */}
                    <div className="pl-7 grid grid-cols-1 md:grid-cols-2 gap-3">
                      <div className="space-y-1">
                        <div className="inline-flex rounded-md border border-border-subtle bg-canvas p-0.5 w-full">
                          <button
                            type="button"
                            onClick={() => handleTargetChange(item, "installWim")}
                            className={`flex-1 py-1 px-2.5 rounded-[4px] text-[12px] font-medium transition-colors ${
                              item.target === "installWim"
                                ? "bg-surface text-text-primary shadow-sm"
                                : "text-text-secondary hover:text-text-primary"
                            }`}
                          >
                            {t("screens.files.targetInstallWim")}
                          </button>
                          <button
                            type="button"
                            disabled={!canTargetIso && item.target !== "isoRoot"}
                            title={
                              canTargetIso
                                ? undefined
                                : t("screens.files.isoRootUnavailable")
                            }
                            onClick={() => handleTargetChange(item, "isoRoot")}
                            className={`flex-1 py-1 px-2.5 rounded-[4px] text-[12px] font-medium transition-colors disabled:opacity-45 disabled:cursor-not-allowed ${
                              item.target === "isoRoot"
                                ? "bg-surface text-text-primary shadow-sm"
                                : "text-text-secondary hover:text-text-primary"
                            }`}
                          >
                            {t("screens.files.targetIsoRoot")}
                          </button>
                        </div>
                      </div>

                      <div className="flex items-center gap-2">
                        <span className="text-[12px] text-text-secondary shrink-0">
                          {t("screens.files.conflictLabel")}:
                        </span>
                        <div className="inline-flex rounded-md border border-border-subtle bg-canvas p-0.5 flex-1">
                          {(["overwrite", "skip"] as ConflictPolicy[]).map((pol) => (
                            <button
                              key={pol}
                              type="button"
                              onClick={() => updateItem(item.id, { conflictPolicy: pol })}
                              className={`flex-1 py-1 px-2.5 rounded-[4px] text-[12px] font-medium transition-colors ${
                                item.conflictPolicy === pol
                                  ? "bg-surface text-text-primary shadow-sm"
                                  : "text-text-secondary hover:text-text-primary"
                              }`}
                            >
                              {pol === "overwrite"
                                ? t("screens.files.policyOverwrite")
                                : t("screens.files.policySkip")}
                            </button>
                          ))}
                        </div>
                      </div>
                    </div>

                    {/* Destination folder input & quick presets */}
                    <div className="pl-7 space-y-2">
                      <div className="flex items-center gap-2">
                        <span className="text-[12px] text-text-secondary shrink-0">
                          {t("screens.files.destLabel")}:
                        </span>
                        <div className="flex-1 flex items-center rounded-md bg-canvas border border-border-subtle focus-within:border-accent overflow-hidden">
                          <span className="px-2.5 py-1.5 bg-subtle border-r border-border-subtle text-[12px] font-mono text-text-secondary shrink-0">
                            {prefix}
                          </span>
                          <input
                            type="text"
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
                            className="px-2 py-0.5 rounded text-[11px] font-mono bg-canvas border border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
                          >
                            {preset}
                          </button>
                        ))}
                        <button
                          type="button"
                          onClick={() => updateItem(item.id, { destinationPath: "" })}
                          className="px-2 py-0.5 rounded text-[11px] bg-canvas border border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
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
                        <div className="flex items-center gap-2 px-3 py-2 rounded-md bg-status-danger-bg text-status-danger text-[12px]">
                          <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                          <span>{t(`screens.files.${validation.errorKey}`)}</span>
                        </div>
                      )}

                      {item.target === "isoRoot" && !canTargetIso && (
                        <div className="flex items-center gap-2 px-3 py-2 rounded-md bg-status-warning-bg text-status-warning text-[12px]">
                          <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                          <span>{t("screens.files.isoRootSkippedWarning")}</span>
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}
