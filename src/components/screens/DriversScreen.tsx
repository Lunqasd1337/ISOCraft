import {
  Suspense,
  lazy,
  useDeferredValue,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  Cpu,
  FileCode,
  Folder,
  FolderPlus,
  HardDriveDownload,
  Loader2,
  Plus,
  RotateCcw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { FluentReveal, FluentSegmentedControl } from "../FluentMotion";
import { localizeBackendError } from "../../utils/backendErrors";
import { driverCategoryLabelKey } from "../../utils/driverCategories";
import type {
  DriverQueueItem,
  DriverScanResult,
  DriversQueueState,
  HostDriverExportResult,
  ImageCatalog,
} from "../../types/studio";

const ExportHostDriversModal = lazy(() =>
  import("./ExportHostDriversModal").then((m) => ({
    default: m.ExportHostDriversModal,
  }))
);

type DriversTab = "add" | "installed";
type InstalledFilter =
  | "all"
  | "storage"
  | "network"
  | "display"
  | "audio"
  | "usb"
  | "other"
  | "removing";

interface DriversScreenProps {
  hasValidImage: boolean;
  canInjectBootWim: boolean;
  catalog: ImageCatalog | null;
  catalogLoading: boolean;
  workspaceDir: string;
  isBuilding?: boolean;
  driversQueue: DriversQueueState;
  onChangeDriversQueue: Dispatch<SetStateAction<DriversQueueState>>;
}

export function DriversScreen({
  hasValidImage,
  canInjectBootWim,
  catalog,
  catalogLoading,
  workspaceDir,
  isBuilding = false,
  driversQueue,
  onChangeDriversQueue,
}: DriversScreenProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<DriversTab>("add");
  const [scanError, setScanError] = useState<string | null>(null);
  const [exportSuccessBanner, setExportSuccessBanner] = useState<string | null>(null);
  const [isHostExportOpen, setIsHostExportOpen] = useState(false);

  const [installedSearch, setInstalledSearch] = useState("");
  const deferredInstalledSearch = useDeferredValue(installedSearch);
  const [installedFilter, setInstalledFilter] = useState<InstalledFilter>("all");

  const removeSet = useMemo(
    () => new Set(driversQueue.removePublishedNames.map((n) => n.toLowerCase())),
    [driversQueue.removePublishedNames]
  );

  const scanAndAddPaths = async (paths: string[]) => {
    if (isBuilding) return;
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
        setScanError(String(err));
      }
    }
    if (added.length > 0) {
      onChangeDriversQueue((prev) => {
        const existingMap = new Map(
          prev.items.map((item) => [item.path.toLowerCase(), item])
        );
        for (const item of added) {
          if (!existingMap.has(item.path.toLowerCase())) {
            existingMap.set(item.path.toLowerCase(), item);
          }
        }
        return {
          ...prev,
          items: Array.from(existingMap.values()),
        };
      });
    }
  };

  const handleHostDriversExported = (
    res: HostDriverExportResult,
    injectStorageToBootWim: boolean
  ) => {
    const newItems: DriverQueueItem[] = [];
    if (res.storageScan) {
      newItems.push({
        ...res.storageScan,
        recurse: true,
        forceUnsigned: false,
        injectToInstallWim: true,
        injectToBootWim: injectStorageToBootWim,
      });
    }
    if (res.otherScan) {
      newItems.push({
        ...res.otherScan,
        recurse: true,
        forceUnsigned: false,
        injectToInstallWim: true,
        injectToBootWim: false,
      });
    }
    if (res.combinedScan) {
      newItems.push({
        ...res.combinedScan,
        recurse: true,
        forceUnsigned: false,
        injectToInstallWim: true,
        injectToBootWim: false,
      });
    }

    if (newItems.length > 0) {
      onChangeDriversQueue((prev) => {
        const existingMap = new Map(
          prev.items.map((item) => [item.path.toLowerCase(), item])
        );
        for (const item of newItems) {
          existingMap.set(item.path.toLowerCase(), item);
        }
        return {
          ...prev,
          items: Array.from(existingMap.values()),
        };
      });
    }

    setExportSuccessBanner(
      t("screens.drivers.hostExportSuccess", {
        count: res.exportedCount,
        path: res.destinationDir,
      })
    );
    setTab("add");
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

  const handleAddFiles = async () => {
    if (isBuilding) return;
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: t("dialogs.driverInf"), extensions: ["inf"] }],
      });
      if (Array.isArray(selected) && selected.length > 0) {
        await scanAndAddPaths(selected);
      }
    } catch (err) {
      setScanError(String(err));
    }
  };

  const updateItem = (path: string, patch: Partial<DriverQueueItem>) => {
    if (isBuilding) return;
    onChangeDriversQueue((prev) => ({
      ...prev,
      items: prev.items.map((item) =>
        item.path === path ? { ...item, ...patch } : item
      ),
    }));
  };

  const removeItem = (path: string) => {
    if (isBuilding) return;
    onChangeDriversQueue((prev) => ({
      ...prev,
      items: prev.items.filter((item) => item.path !== path),
    }));
  };

  const toggleRemoveInstalledDriver = (publishedName: string) => {
    if (isBuilding) return;
    const key = publishedName.toLowerCase();
    onChangeDriversQueue((prev) => {
      const exists = prev.removePublishedNames.some((n) => n.toLowerCase() === key);
      return {
        ...prev,
        removePublishedNames: exists
          ? prev.removePublishedNames.filter((n) => n.toLowerCase() !== key)
          : [...prev.removePublishedNames, publishedName],
      };
    });
  };

  const handleResetRemoveList = () => {
    if (isBuilding) return;
    onChangeDriversQueue((prev) => ({
      ...prev,
      removePublishedNames: [],
    }));
  };

  const filteredInstalledDrivers = useMemo(() => {
    if (!catalog) return [];
    const q = deferredInstalledSearch.trim().toLowerCase();
    return catalog.drivers.filter((d) => {
      if (installedFilter === "removing") {
        if (!removeSet.has(d.publishedName.toLowerCase())) return false;
      } else if (installedFilter !== "all") {
        if (installedFilter === "other") {
          if (["storage", "network", "display", "audio", "usb"].includes(d.category)) {
            return false;
          }
        } else if (d.category !== installedFilter) {
          return false;
        }
      }
      if (!q) return true;
      return (
        d.originalFileName.toLowerCase().includes(q) ||
        d.publishedName.toLowerCase().includes(q) ||
        d.providerName.toLowerCase().includes(q) ||
        d.className.toLowerCase().includes(q)
      );
    });
  }, [catalog, deferredInstalledSearch, installedFilter, removeSet]);

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.drivers.title")}
        subtitle={t("screens.drivers.subtitle")}
        actions={
          <>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={() => setIsHostExportOpen(true)}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <HardDriveDownload className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.drivers.exportHostBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleAddFiles}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <Plus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.drivers.addFileBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleAddFolder}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.drivers.addFolderBtn")}</span>
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
          <div className="flex flex-wrap items-center justify-between gap-3">
            <FluentSegmentedControl<DriversTab>
              value={tab}
              onChange={setTab}
              variant="accent"
              size="md"
              options={[
                {
                  value: "add",
                  label: `${t("screens.drivers.tabAdd")} (${driversQueue.items.length})`,
                },
                {
                  value: "installed",
                  label: `${t("screens.drivers.tabInstalled")} (${catalog?.drivers.length ?? 0})`,
                },
              ]}
            />

            {tab === "installed" && driversQueue.removePublishedNames.length > 0 && (
              <button
                type="button"
                disabled={isBuilding}
                onClick={handleResetRemoveList}
                className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50"
              >
                <RotateCcw className="w-3.5 h-3.5" strokeWidth={1.6} />
                <span>{t("screens.drivers.resetRemoveBtn")}</span>
              </button>
            )}
          </div>

          {exportSuccessBanner && (
            <FluentReveal className="flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-success-bg text-status-success text-[12.5px]">
              <div className="flex items-center gap-2 min-w-0">
                <CheckCircle2 className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span className="break-all">{exportSuccessBanner}</span>
              </div>
              <button
                type="button"
                onClick={() => setExportSuccessBanner(null)}
                className="p-1 rounded hover:bg-subtle transition-colors shrink-0"
              >
                <X className="w-3.5 h-3.5" strokeWidth={1.7} />
              </button>
            </FluentReveal>
          )}

          {scanError && (
            <FluentReveal className="flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px]">
              <div className="flex items-center gap-2 min-w-0">
                <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span className="break-words">
                  {t("screens.drivers.scanError", {
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

          {tab === "add" ? (
            driversQueue.items.length === 0 ? (
              <EmptyStateCard
                icon={Cpu}
                title={t("screens.drivers.emptyTitle")}
                description={t("screens.drivers.emptyDesc")}
                action={
                  <div className="flex flex-wrap items-center justify-center gap-2">
                    <button
                      type="button"
                      onClick={() => setIsHostExportOpen(true)}
                      className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                    >
                      <HardDriveDownload className="w-4 h-4" strokeWidth={1.6} />
                      <span>{t("screens.drivers.exportHostBtn")}</span>
                    </button>
                    <button
                      type="button"
                      onClick={handleAddFiles}
                      className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                    >
                      <Plus className="w-4 h-4" strokeWidth={1.6} />
                      <span>{t("screens.drivers.addFileBtn")}</span>
                    </button>
                    <button
                      type="button"
                      onClick={handleAddFolder}
                      className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98]"
                    >
                      <FolderPlus className="w-4 h-4" strokeWidth={1.6} />
                      <span>{t("screens.drivers.addFolderBtn")}</span>
                    </button>
                  </div>
                }
              />
            ) : (
              <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
                {driversQueue.items.map((item) => {
                  const baseName = item.path.split(/[\\/]/).pop() || item.path;
                  return (
                    <FluentReveal key={item.path} className="px-4 py-3 space-y-2.5">
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
                          disabled={isBuilding}
                          onClick={() => removeItem(item.path)}
                          title={t("screens.drivers.remove")}
                          className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100 shrink-0"
                        >
                          <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                          <span>{t("screens.drivers.remove")}</span>
                        </button>
                      </div>

                      <div className="pl-7 flex flex-wrap items-center gap-x-5 gap-y-2 text-[12.5px]">
                        <label className="flex items-center gap-2 cursor-pointer text-text-primary">
                          <input
                            type="checkbox"
                            disabled={isBuilding}
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
                            canInjectBootWim && !isBuilding
                              ? "cursor-pointer text-text-primary"
                              : "cursor-not-allowed opacity-50 text-text-secondary"
                          }`}
                        >
                          <input
                            type="checkbox"
                            disabled={!canInjectBootWim || isBuilding}
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
                            disabled={isBuilding}
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
                              disabled={isBuilding}
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
                    </FluentReveal>
                  );
                })}
              </div>
            )
          ) : catalogLoading && !catalog ? (
            <div className="bg-surface border border-border-subtle rounded-lg px-6 py-12 flex flex-col items-center gap-3 text-center">
              <Loader2 className="w-6 h-6 animate-spin text-accent" strokeWidth={1.7} />
              <div className="text-[13px] text-text-secondary">
                {t("screens.drivers.installedLoading")}
              </div>
            </div>
          ) : !catalog || catalog.drivers.length === 0 ? (
            <EmptyStateCard
              icon={Cpu}
              title={t("screens.drivers.installedEmptyTitle")}
              description={t("screens.drivers.installedEmptyDesc")}
            />
          ) : (
            <div className="space-y-3">
              {/* Search & Filter */}
              <div className="flex flex-wrap items-center gap-2.5">
                <div className="relative flex-1 min-w-[220px]">
                  <Search
                    className="w-4 h-4 text-text-secondary absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none"
                    strokeWidth={1.6}
                  />
                  <input
                    type="text"
                    value={installedSearch}
                    onChange={(e) => setInstalledSearch(e.target.value)}
                    placeholder={t("screens.drivers.installedSearchPlaceholder")}
                    className="w-full pl-9 pr-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[13px] text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
                  />
                </div>
                <FluentSegmentedControl<InstalledFilter>
                  value={installedFilter}
                  onChange={setInstalledFilter}
                  variant="accent"
                  size="sm"
                  options={[
                    { value: "all", label: t("screens.drivers.categories.all") },
                    { value: "storage", label: t("screens.drivers.categories.storage") },
                    { value: "network", label: t("screens.drivers.categories.network") },
                    { value: "display", label: t("screens.drivers.categories.display") },
                    { value: "audio", label: t("screens.drivers.categories.audio") },
                    { value: "usb", label: t("screens.drivers.categories.usb") },
                    { value: "other", label: t("screens.drivers.categories.other") },
                    {
                      value: "removing",
                      label: t("screens.drivers.filterRemoving", {
                        count: driversQueue.removePublishedNames.length,
                      }),
                    },
                  ]}
                />
              </div>

              {filteredInstalledDrivers.length === 0 ? (
                <div className="bg-surface border border-border-subtle rounded-lg px-5 py-8 text-center text-[13px] text-text-secondary">
                  {t("screens.drivers.installedNoMatches")}
                </div>
              ) : (
                <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
                  {filteredInstalledDrivers.map((drv) => {
                    const marked = removeSet.has(drv.publishedName.toLowerCase());
                    return (
                      <FluentReveal
                        key={drv.publishedName}
                        className={`px-4 py-3 flex items-center justify-between gap-4 transition-colors ${
                          marked ? "bg-status-danger-bg" : "hover:bg-subtle"
                        }`}
                      >
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="text-[13.5px] font-semibold text-text-primary truncate">
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
                            {marked && (
                              <span className="px-1.5 py-0.5 rounded text-[11px] font-semibold bg-status-danger-bg text-status-danger shrink-0">
                                {t("screens.drivers.markedForRemoval")}
                              </span>
                            )}
                          </div>
                          <div className="text-[11.5px] text-text-secondary flex flex-wrap items-center gap-x-3 mt-0.5">
                            {drv.providerName && <span>{drv.providerName}</span>}
                            {drv.version && <span className="font-mono">v{drv.version}</span>}
                            {drv.date && <span className="font-mono">{drv.date}</span>}
                          </div>
                        </div>

                        <button
                          type="button"
                          disabled={isBuilding}
                          onClick={() => toggleRemoveInstalledDriver(drv.publishedName)}
                          className={`inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed shrink-0 ${
                            marked
                              ? "bg-surface border border-border-subtle text-text-primary hover:bg-subtle"
                              : "bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary"
                          }`}
                        >
                          {marked ? (
                            <>
                              <RotateCcw className="w-3.5 h-3.5" strokeWidth={1.6} />
                              <span>{t("screens.drivers.keepDriverBtn")}</span>
                            </>
                          ) : (
                            <>
                              <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                              <span>{t("screens.drivers.removeDriverBtn")}</span>
                            </>
                          )}
                        </button>
                      </FluentReveal>
                    );
                  })}
                </div>
              )}
            </div>
          )}
        </>
      )}

      {isHostExportOpen && (
        <Suspense fallback={null}>
          <ExportHostDriversModal
            workspaceDir={workspaceDir}
            canInjectBootWim={canInjectBootWim}
            onExported={handleHostDriversExported}
            onClose={() => setIsHostExportOpen(false)}
          />
        </Suspense>
      )}
    </div>
  );
}
