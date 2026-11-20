import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { Titlebar } from "./components/Titlebar";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { LogDrawer } from "./components/LogDrawer";
import { SettingsModal } from "./components/SettingsModal";
import { CloseGuardModal } from "./components/CloseGuardModal";
import { ImageScreen } from "./components/screens/ImageScreen";
import { AppsScreen } from "./components/screens/AppsScreen";
import { FeaturesScreen } from "./components/screens/FeaturesScreen";
import { UpdatesScreen } from "./components/screens/UpdatesScreen";
import { DriversScreen } from "./components/screens/DriversScreen";
import { RegistryScreen } from "./components/screens/RegistryScreen";
import { FilesScreen } from "./components/screens/FilesScreen";
import { BuildScreen } from "./components/screens/BuildScreen";
import { usePresetWorkflow } from "./hooks/usePresetWorkflow";
import type {
  AddAppxRequest,
  AppLanguage,
  AppSettings,
  AppTheme,
  AppsQueueState,
  BuildProgressEvent,
  DriversQueueState,
  EnvironmentStatus,
  FeaturesQueueState,
  FilesQueueState,
  ImageCatalog,
  ImageInspectionResult,
  LogEntry,
  OutputOptions,
  RegistryQueueState,
  SectionId,
  UpdatesQueueState,
} from "./types/studio";

const THEME_CYCLE: AppTheme[] = ["system", "light", "dark"];

const EMPTY_APPS_QUEUE: AppsQueueState = {
  removeFamilies: [],
  addPackages: [],
};

const EMPTY_FEATURES_QUEUE: FeaturesQueueState = {
  enableFeatures: [],
  disableFeatures: [],
  addCapabilities: [],
  removeCapabilities: [],
  enableAll: true,
  limitAccess: true,
  customSxsPath: null,
  featureOverrides: {},
  capabilityOverrides: {},
};

const EMPTY_UPDATES_QUEUE: UpdatesQueueState = {
  items: [],
  runComponentCleanup: false,
  cleanupResetBase: false,
};

const EMPTY_DRIVERS_QUEUE: DriversQueueState = {
  items: [],
};

const EMPTY_REGISTRY_QUEUE: RegistryQueueState = {
  regFiles: [],
};

const EMPTY_FILES_QUEUE: FilesQueueState = {
  items: [],
};

function applyDomTheme(theme: AppTheme) {
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const shouldDark = theme === "dark" || (theme === "system" && prefersDark);
  document.documentElement.classList.toggle("dark", shouldDark);
}

export function App() {
  const { t, i18n } = useTranslation();
  const [activeSection, setActiveSection] = useState<SectionId>("image");
  const [settings, setSettings] = useState<AppSettings>({
    workspaceDir: "",
    oscdimgPath: null,
    theme: "system",
    language: "ru",
  });
  const [envStatus, setEnvStatus] = useState<EnvironmentStatus | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [isLogOpen, setIsLogOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [showCloseGuard, setShowCloseGuard] = useState(false);
  const [recovering, setRecovering] = useState(false);

  // Stage 2: Image Inspection & Build State
  const [selectedImagePath, setSelectedImagePath] = useState<string | null>(null);
  const [inspection, setInspection] = useState<ImageInspectionResult | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);

  // Stage 3: Edition Catalog + Apps/Features Queue
  const [catalog, setCatalog] = useState<ImageCatalog | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [appsQueue, setAppsQueue] = useState<AppsQueueState>(EMPTY_APPS_QUEUE);
  const [featuresQueue, setFeaturesQueue] =
    useState<FeaturesQueueState>(EMPTY_FEATURES_QUEUE);
  const [updatesQueue, setUpdatesQueue] =
    useState<UpdatesQueueState>(EMPTY_UPDATES_QUEUE);
  const [driversQueue, setDriversQueue] =
    useState<DriversQueueState>(EMPTY_DRIVERS_QUEUE);
  const [registryQueue, setRegistryQueue] =
    useState<RegistryQueueState>(EMPTY_REGISTRY_QUEUE);
  const [filesQueue, setFilesQueue] =
    useState<FilesQueueState>(EMPTY_FILES_QUEUE);

  const [output, setOutput] = useState<OutputOptions>({
    target: "iso",
    installFormat: "wim",
    splitSizeMb: 3800,
    outputPath: "",
    autounattendPath: null,
    addGenericEiCfg: false,
    uefiNoPrompt: true,
    volumeLabel: "WIN11_CUSTOM",
  });

  const [isBuilding, setIsBuilding] = useState(false);
  const [buildProgress, setBuildProgress] = useState<BuildProgressEvent | null>(null);
  const [buildResultMessage, setBuildResultMessage] = useState<{
    ok: boolean;
    text: string;
  } | null>(null);
  const {
    missingPresetPaths,
    presetBanner,
    dismissPresetBanner,
    resetPresetState,
    handleSavePreset,
    handleLoadPreset,
    handleRelinkMissingPath,
    handleRemoveMissingPath,
  } = usePresetWorkflow({
    inspection,
    selectedIndex,
    catalog,
    appsQueue,
    setAppsQueue,
    featuresQueue,
    setFeaturesQueue,
    updatesQueue,
    setUpdatesQueue,
    driversQueue,
    setDriversQueue,
    registryQueue,
    setRegistryQueue,
    filesQueue,
    setFilesQueue,
    output,
    setOutput,
  });
  const catalogSeqRef = useRef(0);

  const refreshEnvironment = useCallback(async () => {
    try {
      const status = await invoke<EnvironmentStatus>("check_environment");
      setEnvStatus(status);
    } catch {
      // ignore if outside tauri
    }
  }, []);

  const loadCatalogForEdition = useCallback(
    async (insp: ImageInspectionResult, idx: number, forceRefresh = false) => {
      const reqId = ++catalogSeqRef.current;
      setCatalogLoading(true);
      setCatalogError(null);
      setCatalog(null);
      try {
        const res = await invoke<ImageCatalog>("load_image_catalog", {
          sourcePath: insp.sourcePath,
          sourceKind: insp.sourceKind,
          index: idx,
          forceRefresh,
        });
        if (catalogSeqRef.current !== reqId) return;
        setCatalog(res);
      } catch (err) {
        if (catalogSeqRef.current !== reqId) return;
        setCatalogError(String(err));
      } finally {
        if (catalogSeqRef.current === reqId) {
          setCatalogLoading(false);
        }
      }
    },
    []
  );

  useEffect(() => {
    let active = true;

    const init = async () => {
      try {
        const [loadedSettings, initialLogs] = await Promise.all([
          invoke<AppSettings>("get_settings"),
          invoke<LogEntry[]>("get_logs"),
        ]);
        if (!active) return;

        setSettings(loadedSettings);
        setLogs(initialLogs);

        const lang = loadedSettings.language === "en" ? "en" : "ru";
        await i18n.changeLanguage(lang);
        applyDomTheme(loadedSettings.theme);

        const status = await invoke<EnvironmentStatus>("check_environment");
        if (active) setEnvStatus(status);
      } catch {
        applyDomTheme("system");
      }
    };

    init();

    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onMediaChange = () => {
      setSettings((curr) => {
        if (curr.theme === "system") applyDomTheme("system");
        return curr;
      });
    };
    media.addEventListener("change", onMediaChange);

    const unlistenLog = listen<LogEntry>("app-log", (event) => {
      setLogs((prev) => [...prev.slice(-1999), event.payload]);
    });

    const unlistenClose = listen("close-requested-while-busy", () => {
      setShowCloseGuard(true);
    });

    const unlistenProgress = listen<BuildProgressEvent>("build-progress", (event) => {
      setBuildProgress(event.payload);
    });

    return () => {
      active = false;
      media.removeEventListener("change", onMediaChange);
      unlistenLog.then((fn) => fn()).catch(() => {});
      unlistenClose.then((fn) => fn()).catch(() => {});
      unlistenProgress.then((fn) => fn()).catch(() => {});
    };
  }, [i18n]);

  const handleLanguageChange = async (lang: AppLanguage) => {
    await i18n.changeLanguage(lang);
    const updated = { ...settings, language: lang };
    setSettings(updated);
    try {
      await invoke("save_settings", { newSettings: updated });
    } catch {
      // ignore outside tauri
    }
  };

  const handleCycleTheme = async () => {
    const idx = THEME_CYCLE.indexOf(settings.theme);
    const nextTheme = THEME_CYCLE[(idx + 1) % THEME_CYCLE.length];
    const updated = { ...settings, theme: nextTheme };
    setSettings(updated);
    applyDomTheme(nextTheme);
    try {
      await invoke("set_app_theme", { theme: nextTheme });
    } catch {
      // ignore outside tauri
    }
  };

  const handleSaveSettings = async (updated: AppSettings) => {
    const saved = await invoke<AppSettings>("save_settings", {
      newSettings: updated,
    });
    setSettings(saved);
    applyDomTheme(saved.theme);
  };

  const handleRecoverWorkspace = async () => {
    setRecovering(true);
    try {
      await invoke("recover_workspace");
      await refreshEnvironment();
    } finally {
      setRecovering(false);
    }
  };

  const handleForceClose = async () => {
    try {
      await invoke("force_cancel_and_close");
    } catch {
      setShowCloseGuard(false);
    }
  };

  const handleSelectImagePath = async (path: string) => {
    catalogSeqRef.current += 1;
    setSelectedImagePath(path);
    setInspecting(true);
    setInspectError(null);
    setInspection(null);
    setSelectedIndex(null);
    setCatalog(null);
    setCatalogError(null);
    setAppsQueue(EMPTY_APPS_QUEUE);
    setFeaturesQueue(EMPTY_FEATURES_QUEUE);
    setUpdatesQueue(EMPTY_UPDATES_QUEUE);
    setDriversQueue(EMPTY_DRIVERS_QUEUE);
    setRegistryQueue(EMPTY_REGISTRY_QUEUE);
    setFilesQueue(EMPTY_FILES_QUEUE);
    resetPresetState();
    setBuildResultMessage(null);

    try {
      const res = await invoke<ImageInspectionResult>("inspect_image", { path });
      setInspection(res);
      const firstSupported = res.editions.find((e) => e.isSupported);
      const idx = firstSupported ? firstSupported.index : null;
      setSelectedIndex(idx);
      setOutput((prev) => ({
        ...prev,
        target: res.sourceKind === "iso" ? "iso" : "image",
        outputPath: "",
      }));
      if (idx !== null && res.isValidWin11ClientX64) {
        loadCatalogForEdition(res, idx, false);
      }
    } catch (err) {
      setInspectError(String(err));
    } finally {
      setInspecting(false);
    }
  };

  const handleSelectEditionIndex = (idx: number) => {
    if (idx === selectedIndex) return;
    setSelectedIndex(idx);
    setAppsQueue(EMPTY_APPS_QUEUE);
    setFeaturesQueue(EMPTY_FEATURES_QUEUE);
    if (inspection && inspection.isValidWin11ClientX64) {
      loadCatalogForEdition(inspection, idx, false);
    }
  };

  const handleToggleRemoveFamily = (family: string) => {
    setAppsQueue((prev) => {
      const exists = prev.removeFamilies.includes(family);
      return {
        ...prev,
        removeFamilies: exists
          ? prev.removeFamilies.filter((f) => f !== family)
          : [...prev.removeFamilies, family],
      };
    });
  };

  const handleAddAppxPackage = (req: AddAppxRequest) => {
    setAppsQueue((prev) => ({
      ...prev,
      addPackages: [
        ...prev.addPackages.filter((p) => p.packagePath !== req.packagePath),
        req,
      ],
    }));
  };

  const handleRemoveAddedPackage = (packagePath: string) => {
    setAppsQueue((prev) => ({
      ...prev,
      addPackages: prev.addPackages.filter((p) => p.packagePath !== packagePath),
    }));
  };

  const handleStartBuild = async () => {
    if (!inspection || !inspection.isValidWin11ClientX64 || selectedIndex === null) {
      return;
    }

    let outPath = output.outputPath.trim();
    if (!outPath) {
      const ext =
        output.target === "iso"
          ? "iso"
          : output.installFormat === "esd"
          ? "esd"
          : output.installFormat === "swm"
          ? "swm"
          : "wim";
      const chosen = await save({
        defaultPath:
          output.target === "iso" ? "Win11_Custom_x64.iso" : `install.${ext}`,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      });
      if (!chosen) return;
      outPath = chosen;
      setOutput((prev) => ({ ...prev, outputPath: chosen }));
    }

    setActiveSection("build");
    setIsBuilding(true);
    setBuildResultMessage(null);
    setBuildProgress({
      step: 1,
      totalSteps: 9,
      phaseKey: "step_prepare",
      percent: 2,
      message: "...",
    });

    try {
      const savedPath = await invoke<string>("start_build", {
        config: {
          sourcePath: inspection.sourcePath,
          sourceKind: inspection.sourceKind,
          selectedIndex,
          target: output.target,
          installFormat: output.installFormat,
          splitSizeMb: output.splitSizeMb,
          outputPath: outPath,
          autounattendPath: output.autounattendPath,
          addGenericEiCfg: output.addGenericEiCfg,
          uefiNoPrompt: output.uefiNoPrompt,
          volumeLabel: output.volumeLabel,
          apps: {
            removeFamilies: appsQueue.removeFamilies,
            addPackages: appsQueue.addPackages,
          },
          features: {
            enable: featuresQueue.enableFeatures.map((name) => ({
              name,
              enableAll:
                featuresQueue.featureOverrides?.[name]?.enableAll ??
                featuresQueue.enableAll,
              customSxsPath:
                featuresQueue.featureOverrides?.[name]?.customSxsPath ??
                featuresQueue.customSxsPath,
            })),
            disable: featuresQueue.disableFeatures,
            capabilitiesAdd: featuresQueue.addCapabilities.map((name) => ({
              name,
              limitAccess:
                featuresQueue.capabilityOverrides?.[name]?.limitAccess ??
                featuresQueue.limitAccess,
              customSxsPath:
                featuresQueue.capabilityOverrides?.[name]?.customSxsPath ??
                featuresQueue.customSxsPath,
            })),
            capabilitiesRemove: featuresQueue.removeCapabilities,
            defaultSxsPath: featuresQueue.customSxsPath,
          },
          updates: {
            items: updatesQueue.items.map((u) => ({
              path: u.path,
              isDirectory: u.isDirectory,
            })),
            runComponentCleanup: updatesQueue.runComponentCleanup,
            cleanupResetBase: updatesQueue.cleanupResetBase,
          },
          drivers: {
            items: driversQueue.items.map((d) => ({
              path: d.path,
              isDirectory: d.isDirectory,
              recurse: d.recurse,
              forceUnsigned: d.forceUnsigned,
              injectToInstallWim: d.injectToInstallWim,
              injectToBootWim:
                output.target === "iso" && Boolean(inspection.hasBootWim)
                  ? d.injectToBootWim
                  : false,
            })),
          },
          registry: {
            regFiles: registryQueue.regFiles.map((r) => ({
              path: r.path,
              enabled: r.enabled,
            })),
          },
          files: {
            items: filesQueue.items
              .filter((f) => f.target !== "isoRoot" || output.target === "iso")
              .map((f) => ({
                sourcePath: f.sourcePath,
                isDirectory: f.isDirectory,
                target: f.target,
                destinationPath: f.destinationPath,
                conflictPolicy: f.conflictPolicy,
              })),
          },
        },
      });
      setBuildResultMessage({
        ok: true,
        text: t("screens.build.buildDone", { path: savedPath }),
      });
    } catch (err) {
      setBuildResultMessage({
        ok: false,
        text: t("screens.build.buildError", { error: String(err) }),
      });
    } finally {
      setIsBuilding(false);
      await refreshEnvironment();
    }
  };

  const handleCancelBuild = async () => {
    try {
      await invoke("cancel_build");
    } finally {
      setIsBuilding(false);
      await refreshEnvironment();
    }
  };

  const handlePrimaryBuildClick = () => {
    if (activeSection !== "build") {
      setActiveSection("build");
      return;
    }
    handleStartBuild();
  };

  const handleOutputChange = (next: OutputOptions) => {
    if (next.target !== "iso") {
      setDriversQueue((prev) =>
        prev.items.some((d) => d.injectToBootWim)
          ? {
              items: prev.items.map((d) => ({ ...d, injectToBootWim: false })),
            }
          : prev
      );
    }
    setOutput(next);
  };

  const sidebarBadges = useMemo<Partial<Record<SectionId, string>>>(() => {
    const badges: Partial<Record<SectionId, string>> = {};
    const rem = appsQueue.removeFamilies.length;
    const add = appsQueue.addPackages.length;
    if (rem > 0 && add > 0) badges.apps = `-${rem} / +${add}`;
    else if (rem > 0) badges.apps = `-${rem}`;
    else if (add > 0) badges.apps = `+${add}`;

    const featTotal =
      featuresQueue.enableFeatures.length +
      featuresQueue.disableFeatures.length +
      featuresQueue.addCapabilities.length +
      featuresQueue.removeCapabilities.length;
    if (featTotal > 0) badges.features = String(featTotal);

    if (updatesQueue.items.length > 0) {
      badges.updates = String(updatesQueue.items.length);
    }
    if (driversQueue.items.length > 0) {
      badges.drivers = String(driversQueue.items.length);
    }
    const enabledRegsCount = registryQueue.regFiles.filter((r) => r.enabled).length;
    if (enabledRegsCount > 0) {
      badges.registry = String(enabledRegsCount);
    }

    const activeFilesCount = filesQueue.items.filter(
      (f) => f.target !== "isoRoot" || output.target === "iso"
    ).length;
    if (activeFilesCount > 0) {
      badges.files = String(activeFilesCount);
    }

    return badges;
  }, [appsQueue, driversQueue, featuresQueue, filesQueue, output.target, registryQueue, updatesQueue]);

  const totalQueuedCount =
    appsQueue.removeFamilies.length +
    appsQueue.addPackages.length +
    featuresQueue.enableFeatures.length +
    featuresQueue.disableFeatures.length +
    featuresQueue.addCapabilities.length +
    featuresQueue.removeCapabilities.length +
    updatesQueue.items.length +
    (updatesQueue.runComponentCleanup ? 1 : 0) +
    driversQueue.items.length +
    registryQueue.regFiles.filter((r) => r.enabled).length +
    filesQueue.items.filter((f) => f.target !== "isoRoot" || output.target === "iso")
      .length;

  const sidebarBadgesWithBuild = useMemo<Partial<Record<SectionId, string>>>(() => {
    if (totalQueuedCount <= 0) return sidebarBadges;
    return { ...sidebarBadges, build: String(totalQueuedCount) };
  }, [sidebarBadges, totalQueuedCount]);

  const imageBaseName = selectedImagePath
    ? selectedImagePath.split(/[\\/]/).pop() || selectedImagePath
    : null;

  const selectedEdition =
    inspection && selectedIndex !== null
      ? inspection.editions.find((e) => e.index === selectedIndex) ?? null
      : null;

  const canBuild = Boolean(
    inspection?.isValidWin11ClientX64 && selectedIndex !== null && !inspecting
  );

  return (
    <div className="h-screen w-screen flex flex-col bg-canvas text-text-primary overflow-hidden border border-border-subtle">
      <Titlebar
        language={settings.language === "en" ? "en" : "ru"}
        onChangeLanguage={handleLanguageChange}
        theme={settings.theme}
        onCycleTheme={handleCycleTheme}
        onOpenSettings={() => setIsSettingsOpen(true)}
      />

      <div className="flex flex-1 min-h-0">
        <Sidebar
          activeSection={activeSection}
          onSelectSection={setActiveSection}
          badges={sidebarBadgesWithBuild}
        />

        <main className="flex-1 overflow-y-auto px-8 py-6">
          <div className="max-w-[920px] mx-auto space-y-4">
            {envStatus && !envStatus.isAdmin && (
              <div className="animate-fluent-item flex items-center gap-2.5 px-4 py-2.5 rounded-lg bg-status-warning-bg text-status-warning text-[12.5px] font-medium">
                <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span>{t("app.noAdminWarning")}</span>
              </div>
            )}

            {envStatus?.hasDirtyWorkspace && (
              <div className="animate-fluent-item flex items-center justify-between gap-4 px-4 py-3 rounded-lg bg-status-warning-bg text-status-warning">
                <div className="flex items-start gap-2.5 min-w-0">
                  <AlertTriangle
                    className="w-4 h-4 shrink-0 mt-0.5"
                    strokeWidth={1.75}
                  />
                  <div className="min-w-0">
                    <div className="text-[13px] font-semibold">
                      {t("recovery.bannerTitle")}
                    </div>
                    <div className="text-[12px] opacity-90">
                      {t("recovery.bannerDesc", {
                        details: envStatus.dirtyWorkspaceDetails ?? "",
                      })}
                    </div>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={handleRecoverWorkspace}
                  disabled={recovering}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-semibold bg-surface text-text-primary border border-border-subtle hover:bg-subtle shrink-0"
                >
                  <RefreshCw
                    className={`w-3.5 h-3.5 ${recovering ? "animate-spin" : ""}`}
                    strokeWidth={1.7}
                  />
                  <span>{t("recovery.recoverBtn")}</span>
                </button>
              </div>
            )}

            <div key={activeSection} className="animate-fluent-screen">
              {activeSection === "image" && (
              <ImageScreen
                selectedImagePath={selectedImagePath}
                inspection={inspection}
                inspecting={inspecting}
                inspectError={inspectError}
                selectedIndex={selectedIndex}
                catalog={catalog}
                catalogLoading={catalogLoading}
                catalogError={catalogError}
                envStatus={envStatus}
                onSelectImagePath={handleSelectImagePath}
                onSelectEditionIndex={handleSelectEditionIndex}
                onReloadCatalog={() => {
                  if (inspection && selectedIndex !== null) {
                    loadCatalogForEdition(inspection, selectedIndex, true);
                  }
                }}
              />
            )}
            {activeSection === "apps" && (
              <AppsScreen
                catalog={catalog}
                catalogLoading={catalogLoading}
                appsQueue={appsQueue}
                onToggleRemoveFamily={handleToggleRemoveFamily}
                onAddPackage={handleAddAppxPackage}
                onRemoveAddedPackage={handleRemoveAddedPackage}
              />
            )}
            {activeSection === "features" && (
              <FeaturesScreen
                inspection={inspection}
                catalog={catalog}
                catalogLoading={catalogLoading}
                featuresQueue={featuresQueue}
                onChangeFeaturesQueue={setFeaturesQueue}
              />
            )}
            {activeSection === "updates" && (
              <UpdatesScreen
                hasValidImage={Boolean(
                  inspection?.isValidWin11ClientX64 && selectedIndex !== null
                )}
                editionBuildNumber={selectedEdition?.buildNumber ?? null}
                workspaceDir={settings.workspaceDir}
                updatesQueue={updatesQueue}
                onChangeUpdatesQueue={setUpdatesQueue}
              />
            )}
            {activeSection === "drivers" && (
              <DriversScreen
                hasValidImage={Boolean(
                  inspection?.isValidWin11ClientX64 && selectedIndex !== null
                )}
                canInjectBootWim={Boolean(
                  inspection?.hasBootWim && output.target === "iso"
                )}
                driversQueue={driversQueue}
                onChangeDriversQueue={setDriversQueue}
              />
            )}
            {activeSection === "registry" && (
              <RegistryScreen
                hasValidImage={Boolean(
                  inspection?.isValidWin11ClientX64 && selectedIndex !== null
                )}
                registryQueue={registryQueue}
                onChangeRegistryQueue={setRegistryQueue}
              />
            )}
            {activeSection === "files" && (
              <FilesScreen
                hasValidImage={Boolean(
                  inspection?.isValidWin11ClientX64 && selectedIndex !== null
                )}
                canTargetIso={Boolean(
                  inspection?.sourceKind === "iso" && output.target === "iso"
                )}
                filesQueue={filesQueue}
                onChangeFilesQueue={setFilesQueue}
              />
            )}
            {activeSection === "build" && (
              <BuildScreen
                inspection={inspection}
                selectedIndex={selectedIndex}
                appsQueue={appsQueue}
                featuresQueue={featuresQueue}
                updatesQueue={updatesQueue}
                driversQueue={driversQueue}
                registryQueue={registryQueue}
                filesQueue={filesQueue}
                onChangeUpdatesQueue={setUpdatesQueue}
                output={output}
                onChangeOutput={handleOutputChange}
                onNavigate={setActiveSection}
                isBuilding={isBuilding}
                buildProgress={buildProgress}
                buildResultMessage={buildResultMessage}
                missingPresetPaths={missingPresetPaths}
                presetBanner={presetBanner}
                onDismissPresetBanner={dismissPresetBanner}
                onLoadPreset={handleLoadPreset}
                onSavePreset={handleSavePreset}
                onRelinkMissingPath={handleRelinkMissingPath}
                onRemoveMissingPath={handleRemoveMissingPath}
                onCancelBuild={handleCancelBuild}
              />
            )}
            </div>
          </div>
        </main>
      </div>

      {isLogOpen && (
        <LogDrawer logs={logs} onClose={() => setIsLogOpen(false)} />
      )}

      <StatusBar
        imageLabel={imageBaseName}
        editionLabel={selectedEdition?.name ?? null}
        queuedCount={totalQueuedCount}
        freeSpaceGb={envStatus ? envStatus.freeSpaceGb : null}
        lowSpaceWarning={envStatus ? envStatus.lowSpaceWarning : false}
        isLogOpen={isLogOpen}
        onToggleLog={() => setIsLogOpen((v) => !v)}
        onPrimaryBuildClick={handlePrimaryBuildClick}
        buildDisabled={!canBuild}
        isBuilding={isBuilding}
        buildPercent={buildProgress?.percent ?? 0}
      />

      {isSettingsOpen && (
        <SettingsModal
          settings={settings}
          envStatus={envStatus}
          isBuilding={isBuilding}
          onRefreshEnv={refreshEnvironment}
          onSave={handleSaveSettings}
          onClose={() => setIsSettingsOpen(false)}
        />
      )}

      {showCloseGuard && (
        <CloseGuardModal
          onKeepRunning={() => setShowCloseGuard(false)}
          onForceStop={handleForceClose}
        />
      )}
    </div>
  );
}

export default App;
