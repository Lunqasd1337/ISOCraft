import { useState } from "react";
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
import { CatalogModal } from "./CatalogModal";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import type {
  UpdateQueueItem,
  UpdateScanInfo,
  UpdatesQueueState,
} from "../../types/studio";

interface UpdatesScreenProps {
  hasValidImage: boolean;
  editionBuildNumber?: number | null;
  workspaceDir?: string;
  updatesQueue: UpdatesQueueState;
  onChangeUpdatesQueue: (next: UpdatesQueueState) => void;
}

export function UpdatesScreen({
  hasValidImage,
  editionBuildNumber = null,
  workspaceDir = "",
  updatesQueue,
  onChangeUpdatesQueue,
}: UpdatesScreenProps) {
  const { t } = useTranslation();
  const [scanError, setScanError] = useState<string | null>(null);
  const [isCatalogOpen, setIsCatalogOpen] = useState(false);

  const mergeUpdateItems = (added: UpdateQueueItem[]) => {
    if (added.length === 0) return;
    const existingMap = new Map(
      updatesQueue.items.map((item) => [item.path.toLowerCase(), item])
    );
    for (const item of added) {
      existingMap.set(item.path.toLowerCase(), item);
    }
    onChangeUpdatesQueue({
      ...updatesQueue,
      items: Array.from(existingMap.values()),
    });
  };

  const scanAndAddPaths = async (paths: string[]) => {
    setScanError(null);
    const added: UpdateQueueItem[] = [];
    for (const p of paths) {
      try {
        const info = await invoke<UpdateScanInfo>("scan_update_path", { path: p });
        added.push(info);
      } catch (err) {
        setScanError(t("screens.updates.scanError", { error: String(err) }));
      }
    }
    mergeUpdateItems(added);
  };

  const handleAddFiles = async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Windows Updates", extensions: ["msu", "cab"] }],
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

  const handleRemoveItem = (path: string) => {
    onChangeUpdatesQueue({
      ...updatesQueue,
      items: updatesQueue.items.filter((i) => i.path !== path),
    });
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
              disabled={!hasValidImage}
              onClick={() => setIsCatalogOpen(true)}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Globe className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.updates.catalogBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage}
              onClick={handleAddFiles}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Plus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.updates.addFilesBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage}
              onClick={handleAddFolder}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
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
            <div className="animate-fluent-item flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px]">
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
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors"
                  >
                    <Globe className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.updates.catalogBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleAddFiles}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors"
                  >
                    <Plus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.updates.addFilesBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleAddFolder}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors"
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
                  <div
                    key={item.path}
                    className="animate-fluent-item px-4 py-3 flex items-center justify-between gap-4 hover:bg-subtle transition-colors"
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
                        onClick={() => handleRemoveItem(item.path)}
                        title={t("screens.updates.remove")}
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-colors"
                      >
                        <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                        <span>{t("screens.updates.remove")}</span>
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </>
      )}

      {isCatalogOpen && (
        <CatalogModal
          editionBuildNumber={editionBuildNumber}
          workspaceDir={workspaceDir}
          queuedPaths={updatesQueue.items.map((i) => i.path)}
          onDownloaded={mergeUpdateItems}
          onClose={() => setIsCatalogOpen(false)}
        />
      )}
    </div>
  );
}
