import { Suspense, lazy, useState, type Dispatch, type SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import { formatBytes } from "../../utils/format";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  FileArchive,
  Folder,
  FolderPlus,
  Globe,
  Info,
  Plus,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { FluentReveal } from "../FluentMotion";
import { localizeBackendError } from "../../utils/backendErrors";

const CatalogModal = lazy(() =>
  import("./CatalogModal").then((m) => ({ default: m.CatalogModal }))
);
import type {
  UpdateQueueItem,
  UpdateScanInfo,
  UpdatesQueueState,
} from "../../types/studio";

interface UpdatesScreenProps {
  hasValidImage: boolean;
  editionBuildNumber?: number | null;
  workspaceDir?: string;
  isBuilding?: boolean;
  updatesQueue: UpdatesQueueState;
  onChangeUpdatesQueue: Dispatch<SetStateAction<UpdatesQueueState>>;
}

export function UpdatesScreen({
  hasValidImage,
  editionBuildNumber = null,
  workspaceDir = "",
  isBuilding = false,
  updatesQueue,
  onChangeUpdatesQueue,
}: UpdatesScreenProps) {
  const { t } = useTranslation();
  const [scanError, setScanError] = useState<string | null>(null);
  const [isCatalogOpen, setIsCatalogOpen] = useState(false);

  const mergeUpdateItems = (added: UpdateQueueItem[]) => {
    if (added.length === 0) return;
    onChangeUpdatesQueue((prev) => {
      const existingMap = new Map(
        prev.items.map((item) => [item.path.toLowerCase(), item])
      );
      for (const item of added) {
        existingMap.set(item.path.toLowerCase(), item);
      }
      return {
        ...prev,
        items: Array.from(existingMap.values()),
      };
    });
  };

  const scanAndAddPaths = async (paths: string[]) => {
    if (isBuilding) return;
    setScanError(null);
    const added: UpdateQueueItem[] = [];
    for (const p of paths) {
      try {
        const info = await invoke<UpdateScanInfo>("scan_update_path", { path: p });
        added.push(info);
      } catch (err) {
        setScanError(String(err));
      }
    }
    mergeUpdateItems(added);
  };

  const handleAddFiles = async () => {
    if (isBuilding) return;
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: t("dialogs.windowsUpdates"), extensions: ["msu", "cab"] }],
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

  const handleRemoveItem = (path: string) => {
    if (isBuilding) return;
    onChangeUpdatesQueue((prev) => ({
      ...prev,
      items: prev.items.filter((i) => i.path !== path),
    }));
  };

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.updates.title")}
        subtitle={t("screens.updates.subtitle")}
        actions={
          <>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={() => setIsCatalogOpen(true)}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <Globe className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.updates.catalogBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleAddFiles}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <Plus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.updates.addFilesBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleAddFolder}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.updates.addFolderBtn")}</span>
            </button>
          </>
        }
      />

      {!hasValidImage ? (
        <EmptyStateCard
          icon={RefreshCw}
          title={t("screens.updates.emptyTitle")}
          description={t("screens.updates.emptyNeedImage")}
        />
      ) : (
        <>
          <div className="flex items-start gap-2.5 px-4 py-3 rounded-lg bg-surface border border-border-subtle text-[12.5px] text-text-secondary">
            <Info className="w-4 h-4 text-accent shrink-0 mt-0.5" strokeWidth={1.6} />
            <span className="leading-relaxed">{t("screens.updates.checkpointInfo")}</span>
          </div>

          {scanError && (
            <FluentReveal className="flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px]">
              <div className="flex items-center gap-2 min-w-0">
                <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span className="break-words">
                  {t("screens.updates.scanError", {
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

          {updatesQueue.items.length === 0 ? (
            <EmptyStateCard
              icon={RefreshCw}
              title={t("screens.updates.emptyTitle")}
              description={t("screens.updates.emptyDesc")}
              action={
                <div className="flex flex-wrap items-center justify-center gap-2">
                  <button
                    type="button"
                    onClick={() => setIsCatalogOpen(true)}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                  >
                    <Globe className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.updates.catalogBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleAddFiles}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                  >
                    <Plus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.updates.addFilesBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleAddFolder}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98]"
                  >
                    <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.updates.addFolderBtn")}</span>
                  </button>
                </div>
              }
            />
          ) : (
            <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {updatesQueue.items.map((item) => {
                const baseName = item.path.split(/[\\/]/).pop() || item.path;
                const badgeLabel =
                  item.kind === "folder"
                    ? `${t("screens.updates.kindFolder")} • ${t("screens.updates.packagesCount", { count: item.packageCount })}`
                    : item.kind.toUpperCase();

                return (
                  <FluentReveal
                    key={item.path}
                    className="px-4 py-3 flex items-center justify-between gap-4 hover:bg-subtle transition-colors"
                  >
                    <div className="flex items-center gap-3 min-w-0">
                      {item.isDirectory ? (
                        <Folder
                          className="w-4 h-4 text-accent shrink-0"
                          strokeWidth={1.7}
                        />
                      ) : (
                        <FileArchive
                          className="w-4 h-4 text-accent shrink-0"
                          strokeWidth={1.7}
                        />
                      )}
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-[13.5px] font-semibold text-text-primary truncate">
                            {baseName}
                          </span>
                          <span className="px-1.5 py-0.5 rounded text-[11px] font-medium bg-subtle text-text-secondary shrink-0">
                            {badgeLabel}
                          </span>
                        </div>
                        <div className="text-[11.5px] font-mono text-text-secondary truncate mt-0.5">
                          {item.path}
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center gap-3 shrink-0">
                      <span className="text-[12px] font-mono text-text-secondary">
                        {formatBytes(item.totalBytes)}
                      </span>
                      <button
                        type="button"
                        disabled={isBuilding}
                        onClick={() => handleRemoveItem(item.path)}
                        title={t("screens.updates.remove")}
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
                      >
                        <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                        <span>{t("screens.updates.remove")}</span>
                      </button>
                    </div>
                  </FluentReveal>
                );
              })}
            </div>
          )}
        </>
      )}

      {isCatalogOpen && (
        <Suspense fallback={null}>
          <CatalogModal
            editionBuildNumber={editionBuildNumber}
            workspaceDir={workspaceDir}
            isBuilding={isBuilding}
            queuedPaths={updatesQueue.items.map((i) => i.path)}
            onDownloaded={mergeUpdateItems}
            onClose={() => setIsCatalogOpen(false)}
          />
        </Suspense>
      )}
    </div>
  );
}
