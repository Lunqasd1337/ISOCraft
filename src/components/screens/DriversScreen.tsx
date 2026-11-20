import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  Cpu,
  FileCode,
  Folder,
  FolderPlus,
  Plus,
  Trash2,
  X,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import type {
  DriverQueueItem,
  DriverScanResult,
  DriversQueueState,
} from "../../types/studio";

interface DriversScreenProps {
  hasValidImage: boolean;
  canInjectBootWim: boolean;
  driversQueue: DriversQueueState;
  onChangeDriversQueue: (next: DriversQueueState) => void;
}

export function DriversScreen({
  hasValidImage,
  canInjectBootWim,
  driversQueue,
  onChangeDriversQueue,
}: DriversScreenProps) {
  const { t } = useTranslation();
  const [scanError, setScanError] = useState<string | null>(null);

  const scanAndAddPaths = async (paths: string[]) => {
    setScanError(null);
    const added: DriverQueueItem[] = [];
    for (const p of paths) {
      try {
        const res = await invoke<DriverScanResult>("scan_driver_folder", { path: p });
        added.push({
          ...res,
          recurse: res.isDirectory,
          forceUnsigned: false,
          injectToInstallWim: true,
          injectToBootWim: false,
        });
      } catch (err) {
        setScanError(t("screens.drivers.scanError", { error: String(err) }));
      }
    }
    if (added.length > 0) {
      const existingMap = new Map(
        driversQueue.items.map((item) => [item.path.toLowerCase(), item])
      );
      for (const item of added) {
        if (!existingMap.has(item.path.toLowerCase())) {
          existingMap.set(item.path.toLowerCase(), item);
        }
      }
      onChangeDriversQueue({
        items: Array.from(existingMap.values()),
      });
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

  const handleAddFiles = async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Driver INF", extensions: ["inf"] }],
    });
    if (Array.isArray(selected) && selected.length > 0) {
      await scanAndAddPaths(selected);
    }
  };

  const updateItem = (path: string, patch: Partial<DriverQueueItem>) => {
    onChangeDriversQueue({
      items: driversQueue.items.map((item) =>
        item.path === path ? { ...item, ...patch } : item
      ),
    });
  };

  const removeItem = (path: string) => {
    onChangeDriversQueue({
      items: driversQueue.items.filter((item) => item.path !== path),
    });
  };

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.drivers.title")}
        subtitle={t("screens.drivers.subtitle")}
        actions={
          <>
            <button
              type="button"
              disabled={!hasValidImage}
              onClick={handleAddFolder}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.drivers.addFolderBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage}
              onClick={handleAddFiles}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Plus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.drivers.addFileBtn")}</span>
            </button>
          </>
        }
      />

      {!hasValidImage ? (
        <EmptyStateCard
          icon={Cpu}
          title={t("screens.drivers.emptyTitle")}
          description={t("screens.drivers.emptyNeedImage")}
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

          {driversQueue.items.length === 0 ? (
            <EmptyStateCard
              icon={Cpu}
              title={t("screens.drivers.emptyTitle")}
              description={t("screens.drivers.emptyDesc")}
              action={
                <div className="flex flex-wrap items-center justify-center gap-2">
                  <button
                    type="button"
                    onClick={handleAddFolder}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors"
                  >
                    <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.drivers.addFolderBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleAddFiles}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors"
                  >
                    <Plus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.drivers.addFileBtn")}</span>
                  </button>
                </div>
              }
            />
          ) : (
            <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {driversQueue.items.map((item) => {
                const baseName = item.path.split(/[\\/]/).pop() || item.path;
                return (
                  <div key={item.path} className="px-4 py-3 space-y-2.5">
                    <div className="flex items-start justify-between gap-4">
                      <div className="flex items-start gap-3 min-w-0">
                        {item.isDirectory ? (
                          <Folder
                            className="w-4 h-4 text-accent shrink-0 mt-0.5"
                            strokeWidth={1.7}
                          />
                        ) : (
                          <FileCode
                            className="w-4 h-4 text-accent shrink-0 mt-0.5"
                            strokeWidth={1.7}
                          />
                        )}
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="text-[13.5px] font-semibold text-text-primary truncate">
                              {baseName}
                            </span>
                            <span className="px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-success-bg text-status-success shrink-0">
                              {t("screens.drivers.foundInfBadge", {
                                count: item.infCount,
                              })}
                            </span>
                          </div>
                          <div className="text-[11.5px] font-mono text-text-secondary truncate mt-0.5">
                            {item.path}
                          </div>
                          {item.sampleInfs.length > 0 && item.isDirectory && (
                            <div className="text-[11px] font-mono text-text-secondary truncate mt-0.5">
                              {item.sampleInfs.join(", ")}
                              {item.infCount > item.sampleInfs.length ? ", ..." : ""}
                            </div>
                          )}
                        </div>
                      </div>

                      <button
                        type="button"
                        onClick={() => removeItem(item.path)}
                        title={t("screens.drivers.remove")}
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-colors shrink-0"
                      >
                        <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                        <span>{t("screens.drivers.remove")}</span>
                      </button>
                    </div>

                    <div className="pl-7 flex flex-wrap items-center gap-x-5 gap-y-2 text-[12.5px]">
                      <label className="flex items-center gap-2 cursor-pointer text-text-primary">
                        <input
                          type="checkbox"
                          checked={item.injectToInstallWim}
                          onChange={(e) =>
                            updateItem(item.path, {
                              injectToInstallWim: e.target.checked,
                            })
                          }
                          className="accent-accent"
                        />
                        <span className="font-mono text-[12px]">
                          {t("screens.drivers.targetInstallWim")}
                        </span>
                      </label>

                      <label
                        title={
                          canInjectBootWim
                            ? t("screens.drivers.bootWimHint")
                            : t("screens.drivers.bootWimUnavailable")
                        }
                        className={`flex items-center gap-2 ${
                          canInjectBootWim
                            ? "cursor-pointer text-text-primary"
                            : "cursor-not-allowed opacity-50 text-text-secondary"
                        }`}
                      >
                        <input
                          type="checkbox"
                          disabled={!canInjectBootWim}
                          checked={canInjectBootWim && item.injectToBootWim}
                          onChange={(e) =>
                            updateItem(item.path, {
                              injectToBootWim: e.target.checked,
                            })
                          }
                          className="accent-accent"
                        />
                        <span className="font-mono text-[12px]">
                          {t("screens.drivers.targetBootWim")}
                        </span>
                      </label>

                      <label className="flex items-center gap-2 cursor-pointer text-text-primary">
                        <input
                          type="checkbox"
                          checked={item.forceUnsigned}
                          onChange={(e) =>
                            updateItem(item.path, {
                              forceUnsigned: e.target.checked,
                            })
                          }
                          className="accent-accent"
                        />
                        <span>{t("screens.drivers.forceUnsigned")}</span>
                      </label>

                      {item.isDirectory && (
                        <label className="flex items-center gap-2 cursor-pointer text-text-primary">
                          <input
                            type="checkbox"
                            checked={item.recurse}
                            onChange={(e) =>
                              updateItem(item.path, { recurse: e.target.checked })
                            }
                            className="accent-accent"
                          />
                          <span>{t("screens.drivers.recurse")}</span>
                        </label>
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
