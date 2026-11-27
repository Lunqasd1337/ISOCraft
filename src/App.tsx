import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { Titlebar } from "./components/Titlebar";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { LogDrawer } from "./components/LogDrawer";
import { CloseGuardModal } from "./components/CloseGuardModal";
import {
  ConfirmResetModal,
  type PendingResetAction,
} from "./components/ConfirmResetModal";
import { ScreenTransition } from "./components/FluentMotion";
import { localizeBackendError } from "./utils/backendErrors";
import { prefersReducedMotion } from "./utils/motion";
import { getBaseName, validateDestinationClient } from "./utils/destinationValidation";
import { ImageScreen } from "./components/screens/ImageScreen";
import { AppsScreen } from "./components/screens/AppsScreen";
import { FeaturesScreen } from "./components/screens/FeaturesScreen";
import { ServicesScreen } from "./components/screens/ServicesScreen";
import { UpdatesScreen } from "./components/screens/UpdatesScreen";
import { DriversScreen } from "./components/screens/DriversScreen";
import { RegistryScreen } from "./components/screens/RegistryScreen";
import { FilesScreen } from "./components/screens/FilesScreen";
import { PostSetupScreen } from "./components/screens/PostSetupScreen";
import { BuildScreen } from "./components/screens/BuildScreen";
import { usePresetWorkflow } from "./hooks/usePresetWorkflow";
import { destroyLogStore, initLogStore } from "./hooks/useLogStore";

const SettingsModal = lazy(() =>
  import("./components/SettingsModal").then((m) => ({
    default: m.SettingsModal,
  }))
);
import {
  detectSystemLanguage,
  persistCachedLanguage,
  resolveInitialLanguage,
} from "./i18n";
import type {
  AddAppxRequest,
  AppLanguage,
  AppSettings,
  AppTheme,
  AppUpdateInfo,
  AppsQueueState,
  BuildProgressEvent,
  BuildResultState,
  DriversQueueState,
  EnvironmentStatus,
  FeaturesQueueState,
  FilesQueueState,
  ImageCatalog,
  ImageInspectionResult,
  OutputOptions,
  PostSetupQueueState,
  RegistryQueueState,
  SectionId,
  ServicesQueueState,
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

const EMPTY_SERVICES_QUEUE: ServicesQueueState = {
  changes: [],
};

const EMPTY_UPDATES_QUEUE: UpdatesQueueState = {
  items: [],
  runComponentCleanup: false,
  cleanupResetBase: false,
};

const EMPTY_DRIVERS_QUEUE: DriversQueueState = {
  items: [],
  removePublishedNames: [],
};

const EMPTY_REGISTRY_QUEUE: RegistryQueueState = {
  regFiles: [],
};

const EMPTY_FILES_QUEUE: FilesQueueState = {
  items: [],
};

const EMPTY_POST_SETUP_QUEUE: PostSetupQueueState = {
  items: [],
  firstLogonMode: "runOnce",
  cleanupTempFiles: true,
  waitForNetwork: true,
};

let themeTransitionTimer: number | null = null;

function applyDomTheme(theme: AppTheme, animate = false) {
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const shouldDark = theme === "dark" || (theme === "system" && prefersDark);
  const root = document.documentElement;
  const changed = root.classList.contains("dark") !== shouldDark;

  if (animate && changed && !prefersReducedMotion()) {
    root.classList.add("theme-transitioning");
    if (themeTransitionTimer !== null) {
      window.clearTimeout(themeTransitionTimer);
    }
    themeTransitionTimer = window.setTimeout(() => {
      themeTransitionTimer = null;
      root.classList.remove("theme-transitioning");
    }, 200);
  }

  root.classList.toggle("dark", shouldDark);
}

export function App() {
  const { t, i18n } = useTranslation();
  const [activeSection, setActiveSection] = useState<SectionId>("image");
  const [settings, setSettings] = useState<AppSettings>(() => ({
    workspaceDir: "",
    oscdimgPath: null,
    theme: "system",
    language: resolveInitialLanguage(),
  }));
  const settingsLoadedRef = useRef(false);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const [envStatus, setEnvStatus] = useState<EnvironmentStatus | null>(null);
  const [isLogOpen, setIsLogOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [showCloseGuard, setShowCloseGuard] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const [recoveryError, setRecoveryError] = useState<string | null>(null);

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
  const [servicesQueue, setServicesQueue] =
    useState<ServicesQueueState>(EMPTY_SERVICES_QUEUE);
  const [updatesQueue, setUpdatesQueue] =
    useState<UpdatesQueueState>(EMPTY_UPDATES_QUEUE);
  const [driversQueue, setDriversQueue] =
    useState<DriversQueueState>(EMPTY_DRIVERS_QUEUE);
  const [registryQueue, setRegistryQueue] =
    useState<RegistryQueueState>(EMPTY_REGISTRY_QUEUE);
  const [filesQueue, setFilesQueue] =
    useState<FilesQueueState>(EMPTY_FILES_QUEUE);
  const [postSetupQueue, setPostSetupQueue] =
    useState<PostSetupQueueState>(EMPTY_POST_SETUP_QUEUE);

  const [output, setOutput] = useState<OutputOptions>({
    target: "iso",
    installFormat: "wim",
    outputPath: "",
    autounattendPath: null,
    addGenericEiCfg: false,
    uefiNoPrompt: true,
    volumeLabel: "WIN11_CUSTOM",
  });

  const [isBuilding, setIsBuilding] = useState(false);
  const [cancellingBuild, setCancellingBuild] = useState(false);
  const [cancelError, setCancelError] = useState<string | null>(null);
  const [buildStartedAt, setBuildStartedAt] = useState<number | null>(null);
  const [buildProgress, setBuildProgress] = useState<BuildProgressEvent | null>(null);
  const [buildResultMessage, setBuildResultMessage] =
    useState<BuildResultState | null>(null);
  const [availableAppVersion, setAvailableAppVersion] = useState<string | null>(
    null
  );
  const [pendingResetAction, setPendingResetAction] =
    useState<PendingResetAction | null>(null);

  const isGlobalBusy =
    isBuilding || inspecting || catalogLoading || recovering || cancellingBuild;

  const {
    missingPresetPaths,
    presetBanner,
    presetLoading,
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
    isBusy: isGlobalBusy,
    appsQueue,
    setAppsQueue,
    featuresQueue,
    setFeaturesQueue,
    servicesQueue,
    setServicesQueue,
    updatesQueue,
    setUpdatesQueue,
    driversQueue,
    setDriversQueue,
    registryQueue,
    setRegistryQueue,
    filesQueue,
    setFilesQueue,
    postSetupQueue,
    setPostSetupQueue,
    output,
    setOutput,
  });
  const catalogSeqRef = useRef(0);
  const inspectSeqRef = useRef(0);
  const startingBuildRef = useRef(false);

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const update = await invoke<AppUpdateInfo | null>("check_app_update");
          if (active && update?.version) {
            setAvailableAppVersion(update.version);
          }
        } catch {
          // Ignore updater network/offline errors on background check
        }
      })();
    }, 2500);

    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, []);

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

    void initLogStore();

    const init = async () => {
      try {
        const loadedSettings = await invoke<AppSettings>("get_settings");
        if (!active) return;

        const lang: AppLanguage =
          loadedSettings.language === "ru" || loadedSettings.language === "en"
            ? loadedSettings.language
            : detectSystemLanguage();

        settingsLoadedRef.current = true;
        setSettings({ ...loadedSettings, language: lang });

        await i18n.changeLanguage(lang);
        persistCachedLanguage(lang);
        applyDomTheme(loadedSettings.theme, false);

        const status = await invoke<EnvironmentStatus>("check_environment");
        if (active) setEnvStatus(status);
      } catch {
        applyDomTheme("system", false);
      }
    };

    init();

    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onMediaChange = () => {
      setSettings((curr) => {
        if (curr.theme === "system") applyDomTheme("system", true);
        return curr;
      });
    };
    media.addEventListener("change", onMediaChange);

    const registerSafe = (
      promise: Promise<UnlistenFn>,
      setter: (fn: UnlistenFn) => void
    ) => {
      promise
        .then((fn) => {
          if (!active) {
            fn();
          } else {
            setter(fn);
          }
        })
        .catch(() => {});
    };

    let unlistenCloseFn: UnlistenFn | null = null;
    let unlistenProgressFn: UnlistenFn | null = null;

    registerSafe(
      listen("close-requested-while-busy", () => {
        if (!active) return;
        setShowCloseGuard(true);
      }),
      (fn) => {
        unlistenCloseFn = fn;
      }
    );

    registerSafe(
      listen<BuildProgressEvent>("build-progress", (event) => {
        if (!active) return;
        setBuildProgress(event.payload);
      }),
      (fn) => {
        unlistenProgressFn = fn;
      }
    );

    return () => {
      active = false;
      media.removeEventListener("change", onMediaChange);
      destroyLogStore();
      unlistenCloseFn?.();
      unlistenProgressFn?.();
    };
  }, [i18n]);

  const handleLanguageChange = async (lang: AppLanguage) => {
    await i18n.changeLanguage(lang);
    persistCachedLanguage(lang);
    const updated: AppSettings = { ...settingsRef.current, language: lang };
    setSettings((prev) => ({ ...prev, language: lang }));
    if (!settingsLoadedRef.current || !updated.workspaceDir.trim()) {
      return;
    }
    try {
      await invoke("save_settings", { newSettings: updated });
    } catch {
      // ignore outside tauri
    }
  };

  const handleCycleTheme = async () => {
    const cur = settingsRef.current;
    const idx = THEME_CYCLE.indexOf(cur.theme);
    const nextTheme = THEME_CYCLE[(idx + 1) % THEME_CYCLE.length];
    setSettings((prev) => ({ ...prev, theme: nextTheme }));
    applyDomTheme(nextTheme, true);
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
    applyDomTheme(saved.theme, true);
  };

  const handleRecoverWorkspace = async () => {
    if (recovering || isBuilding) return;
    setRecovering(true);
    setRecoveryError(null);
    try {
      await invoke("recover_workspace");
      await refreshEnvironment();
    } catch (err) {
      setRecoveryError(String(err));
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

  const applySelectImagePath = async (path: string) => {
    const inspSeq = ++inspectSeqRef.current;
    catalogSeqRef.current += 1;
    setCatalogLoading(false);
    setSelectedImagePath(path);
    setInspecting(true);
    setInspectError(null);
    setInspection(null);
    setSelectedIndex(null);
    setCatalog(null);
    setCatalogError(null);
    setAppsQueue(EMPTY_APPS_QUEUE);
    setFeaturesQueue(EMPTY_FEATURES_QUEUE);
    setServicesQueue(EMPTY_SERVICES_QUEUE);
    setUpdatesQueue(EMPTY_UPDATES_QUEUE);
    setDriversQueue(EMPTY_DRIVERS_QUEUE);
    setRegistryQueue(EMPTY_REGISTRY_QUEUE);
    setFilesQueue(EMPTY_FILES_QUEUE);
    setPostSetupQueue(EMPTY_POST_SETUP_QUEUE);
    setOutput({
      target: "iso",
      installFormat: "wim",
      outputPath: "",
      autounattendPath: null,
      addGenericEiCfg: false,
      uefiNoPrompt: true,
      volumeLabel: "WIN11_CUSTOM",
    });
    resetPresetState();
    setBuildResultMessage(null);
    setBuildProgress(null);
    setBuildStartedAt(null);
    setCancelError(null);

    try {
      const res = await invoke<ImageInspectionResult>("inspect_image", { path });
      if (inspectSeqRef.current !== inspSeq) return;
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
      if (inspectSeqRef.current !== inspSeq) return;
      setInspectError(String(err));
    } finally {
      if (inspectSeqRef.current === inspSeq) {
        setInspecting(false);
      }
    }
  };

  const applySelectEditionIndex = (idx: number) => {
    if (idx === selectedIndex) return;
    setSelectedIndex(idx);
    setAppsQueue(EMPTY_APPS_QUEUE);
    setFeaturesQueue(EMPTY_FEATURES_QUEUE);
    setServicesQueue(EMPTY_SERVICES_QUEUE);
    setDriversQueue((prev) =>
      prev.removePublishedNames.length > 0
        ? { ...prev, removePublishedNames: [] }
        : prev
    );
    setBuildResultMessage(null);
    setBuildProgress(null);
    setBuildStartedAt(null);
    if (inspection && inspection.isValidWin11ClientX64) {
      loadCatalogForEdition(inspection, idx, false);
    }
  };

  const handleSelectImagePath = (path: string) => {
    if (isBuilding || recovering || cancellingBuild) return;
    if (totalQueuedCount > 0) {
      setPendingResetAction({ kind: "image", path });
      return;
    }
    void applySelectImagePath(path);
  };

  const handleSelectEditionIndex = (idx: number) => {
    if (idx === selectedIndex || isBuilding || recovering || cancellingBuild) return;
    const editionQueuedCount =
      appsQueue.removeFamilies.length +
      appsQueue.addPackages.length +
      featuresQueue.enableFeatures.length +
      featuresQueue.disableFeatures.length +
      featuresQueue.addCapabilities.length +
      featuresQueue.removeCapabilities.length +
      servicesQueue.changes.length +
      driversQueue.removePublishedNames.length;
    if (editionQueuedCount > 0) {
      setPendingResetAction({ kind: "edition", index: idx });
      return;
    }
    applySelectEditionIndex(idx);
  };

  const handleConfirmPendingReset = () => {
    const action = pendingResetAction;
    setPendingResetAction(null);
    if (!action) return;
    if (action.kind === "image") {
      void applySelectImagePath(action.path);
    } else {
      applySelectEditionIndex(action.index);
    }
  };

  const handleToggleRemoveFamily = (family: string) => {
    const famLower = family.toLowerCase();
    setAppsQueue((prev) => {
      const exists = prev.removeFamilies.some(
        (f) => f.toLowerCase() === famLower
      );
      return {
        ...prev,
        removeFamilies: exists
          ? prev.removeFamilies.filter((f) => f.toLowerCase() !== famLower)
          : [...prev.removeFamilies, family],
      };
    });
  };

  const handleBulkRemoveFamilies = (families: string[]) => {
    if (families.length === 0) return;
    setAppsQueue((prev) => {
      const seen = new Set(prev.removeFamilies.map((f) => f.toLowerCase()));
      const next = [...prev.removeFamilies];
      for (const fam of families) {
        const key = fam.toLowerCase();
        if (!seen.has(key)) {
          seen.add(key);
          next.push(fam);
        }
      }
      return {
        ...prev,
        removeFamilies: next,
      };
    });
  };

  const handleResetAppsQueue = () => {
    setAppsQueue(EMPTY_APPS_QUEUE);
  };

  const handleAddAppxPackage = (req: AddAppxRequest) => {
    const pkgLower = req.packagePath.toLowerCase();
    setAppsQueue((prev) => ({
      ...prev,
      addPackages: [
        ...prev.addPackages.filter((p) => p.packagePath.toLowerCase() !== pkgLower),
        req,
      ],
    }));
  };

  const handleRemoveAddedPackage = (packagePath: string) => {
    const pkgLower = packagePath.toLowerCase();
    setAppsQueue((prev) => ({
      ...prev,
      addPackages: prev.addPackages.filter(
        (p) => p.packagePath.toLowerCase() !== pkgLower
      ),
    }));
  };

  const hasInvalidFileDestinations = useMemo(
    () =>
      filesQueue.items
        .filter((f) => f.target !== "isoRoot" || output.target === "iso")
        .some(
          (f) =>
            validateDestinationClient(
              f.target,
              f.destinationPath,
              getBaseName(f.sourcePath)
            ).errorKey !== null
        ),
    [filesQueue.items, output.target]
  );

  const canBuild = Boolean(
    inspection?.isValidWin11ClientX64 &&
      selectedIndex !== null &&
      !inspecting &&
      !catalogLoading &&
      !recovering &&
      !cancellingBuild &&
      !presetLoading &&
      !envStatus?.hasDirtyWorkspace &&
      !hasInvalidFileDestinations
  );

  const handleStartBuild = async () => {
    if (
      startingBuildRef.current ||
      !inspection ||
      !inspection.isValidWin11ClientX64 ||
      selectedIndex === null ||
      isBuilding ||
      cancellingBuild ||
      inspecting ||
      recovering ||
      presetLoading ||
      envStatus?.hasDirtyWorkspace ||
      hasInvalidFileDestinations
    ) {
      return;
    }

    startingBuildRef.current = true;
    try {
      let outPath = output.outputPath.trim();
      if (!outPath) {
        const ext =
          output.target === "iso"
            ? "iso"
            : output.installFormat === "esd"
            ? "esd"
            : "wim";
        let chosen: string | null = null;
        try {
          const picked = await save({
            defaultPath:
              output.target === "iso" ? "Win11_Custom_x64.iso" : `install.${ext}`,
            filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
          });
          if (typeof picked === "string" && picked.trim()) {
            chosen = picked;
          }
        } catch (err) {
          setBuildResultMessage({
            ok: false,
            error: String(err),
          });
          return;
        }
        if (!chosen) return;
        outPath = chosen;
        setOutput((prev) => ({ ...prev, outputPath: chosen }));
      }

      setActiveSection("build");
      setIsBuilding(true);
      setCancelError(null);
      setBuildResultMessage(null);
      setBuildStartedAt(Date.now());
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
              enable: featuresQueue.enableFeatures.map((name) => {
                const ov =
                  featuresQueue.featureOverrides &&
                  Object.hasOwn(featuresQueue.featureOverrides, name)
                    ? featuresQueue.featureOverrides[name]
                    : undefined;
                return {
                  name,
                  enableAll: ov?.enableAll ?? featuresQueue.enableAll,
                  customSxsPath: ov?.customSxsPath ?? featuresQueue.customSxsPath,
                };
              }),
              disable: featuresQueue.disableFeatures,
              capabilitiesAdd: featuresQueue.addCapabilities.map((name) => {
                const ov =
                  featuresQueue.capabilityOverrides &&
                  Object.hasOwn(featuresQueue.capabilityOverrides, name)
                    ? featuresQueue.capabilityOverrides[name]
                    : undefined;
                return {
                  name,
                  limitAccess: ov?.limitAccess ?? featuresQueue.limitAccess,
                  customSxsPath: ov?.customSxsPath ?? featuresQueue.customSxsPath,
                };
              }),
              capabilitiesRemove: featuresQueue.removeCapabilities,
              defaultSxsPath: featuresQueue.customSxsPath,
            },
            services: {
              changes: servicesQueue.changes,
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
              removePublishedNames: driversQueue.removePublishedNames,
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
            postSetup: {
              ...postSetupQueue,
              firstLogonMode:
                output.target === "iso" ? postSetupQueue.firstLogonMode : "runOnce",
            },
          },
        });
        setBuildResultMessage({
          ok: true,
          path: savedPath,
        });
      } catch (err) {
        setBuildResultMessage({
          ok: false,
          error: String(err),
        });
      } finally {
        setIsBuilding(false);
        setBuildStartedAt(null);
        await refreshEnvironment();
      }
    } finally {
      startingBuildRef.current = false;
    }
  };

  const handleCancelBuild = async () => {
    if (cancellingBuild) return;
    setCancellingBuild(true);
    setCancelError(null);
    try {
      await invoke("cancel_build");
      setIsBuilding(false);
      setBuildStartedAt(null);
    } catch (err) {
      setCancelError(String(err));
    } finally {
      setCancellingBuild(false);
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
              ...prev,
              items: prev.items.map((d) => ({ ...d, injectToBootWim: false })),
            }
          : prev
      );
      setPostSetupQueue((prev) =>
        prev.firstLogonMode === "autounattend"
          ? { ...prev, firstLogonMode: "runOnce" }
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

    if (servicesQueue.changes.length > 0) {
      badges.services = String(servicesQueue.changes.length);
    }

    if (updatesQueue.items.length > 0) {
      badges.updates = String(updatesQueue.items.length);
    }
    const drvRem = driversQueue.removePublishedNames.length;
    const drvAdd = driversQueue.items.length;
    if (drvRem > 0 && drvAdd > 0) badges.drivers = `-${drvRem} / +${drvAdd}`;
    else if (drvRem > 0) badges.drivers = `-${drvRem}`;
    else if (drvAdd > 0) badges.drivers = `+${drvAdd}`;
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

    const activePostSetupCount = postSetupQueue.items.filter((i) => i.enabled).length;
    if (activePostSetupCount > 0) {
      badges.postSetup = String(activePostSetupCount);
    }

    return badges;
  }, [appsQueue, driversQueue, featuresQueue, filesQueue, output.target, postSetupQueue, registryQueue, servicesQueue.changes.length, updatesQueue]);

  const totalQueuedCount =
    appsQueue.removeFamilies.length +
    appsQueue.addPackages.length +
    featuresQueue.enableFeatures.length +
    featuresQueue.disableFeatures.length +
    featuresQueue.addCapabilities.length +
    featuresQueue.removeCapabilities.length +
    servicesQueue.changes.length +
    updatesQueue.items.length +
    (updatesQueue.runComponentCleanup ? 1 : 0) +
    driversQueue.items.length +
    driversQueue.removePublishedNames.length +
    registryQueue.regFiles.filter((r) => r.enabled).length +
    filesQueue.items.filter((f) => f.target !== "isoRoot" || output.target === "iso")
      .length +
    postSetupQueue.items.filter((i) => i.enabled).length;

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

  const editionResetCount =
    appsQueue.removeFamilies.length +
    appsQueue.addPackages.length +
    featuresQueue.enableFeatures.length +
    featuresQueue.disableFeatures.length +
    featuresQueue.addCapabilities.length +
    featuresQueue.removeCapabilities.length +
    servicesQueue.changes.length +
    driversQueue.removePublishedNames.length;

  return (
    <div className="h-screen w-screen flex flex-col bg-canvas text-text-primary overflow-hidden border border-border-subtle">
      <Titlebar
        language={settings.language === "en" ? "en" : "ru"}
        onChangeLanguage={handleLanguageChange}
        theme={settings.theme}
        onCycleTheme={handleCycleTheme}
        availableAppVersion={availableAppVersion}
        isBusy={isGlobalBusy}
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
              <div className="animate-fluent-item flex flex-col gap-2 px-4 py-3 rounded-lg bg-status-warning-bg text-status-warning">
                <div className="flex items-center justify-between gap-4">
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
                          mounts: envStatus.dirtyMountsCount,
                          hives: envStatus.dirtyHivesCount,
                        })}
                      </div>
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={handleRecoverWorkspace}
                    disabled={recovering || isBuilding}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-semibold bg-surface text-text-primary border border-border-subtle hover:bg-subtle transition-all duration-150 active:scale-[0.98] disabled:opacity-60 disabled:active:scale-100 shrink-0"
                  >
                    <RefreshCw
                      className={`w-3.5 h-3.5 ${recovering ? "animate-spin" : ""}`}
                      strokeWidth={1.7}
                    />
                    <span>{t("recovery.recoverBtn")}</span>
                  </button>
                </div>
                {recoveryError && (
                  <div className="text-[12px] text-status-danger bg-status-danger-bg rounded px-2.5 py-1.5">
                    {localizeBackendError(recoveryError)}
                  </div>
                )}
              </div>
            )}

            <ScreenTransition key={activeSection}>
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
                isBuilding={isBuilding || recovering || cancellingBuild}
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
                isBuilding={isBuilding || recovering || cancellingBuild}
                appsQueue={appsQueue}
                onToggleRemoveFamily={handleToggleRemoveFamily}
                onBulkRemoveFamilies={handleBulkRemoveFamilies}
                onResetQueue={handleResetAppsQueue}
                onAddPackage={handleAddAppxPackage}
                onRemoveAddedPackage={handleRemoveAddedPackage}
              />
            )}
            {activeSection === "features" && (
              <FeaturesScreen
                inspection={inspection}
                catalog={catalog}
                catalogLoading={catalogLoading}
                isBuilding={isBuilding || recovering || cancellingBuild}
                featuresQueue={featuresQueue}
                onChangeFeaturesQueue={setFeaturesQueue}
              />
            )}
            {activeSection === "services" && (
              <ServicesScreen
                catalog={catalog}
                catalogLoading={catalogLoading}
                isBuilding={isBuilding || recovering || cancellingBuild}
                servicesQueue={servicesQueue}
                onChangeServicesQueue={setServicesQueue}
              />
            )}
            {activeSection === "updates" && (
              <UpdatesScreen
                hasValidImage={Boolean(
                  inspection?.isValidWin11ClientX64 && selectedIndex !== null
                )}
                editionBuildNumber={selectedEdition?.buildNumber ?? null}
                workspaceDir={settings.workspaceDir}
                isBuilding={isBuilding || recovering || cancellingBuild}
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
                catalog={catalog}
                catalogLoading={catalogLoading}
                workspaceDir={settings.workspaceDir}
                isBuilding={isBuilding || recovering || cancellingBuild}
                driversQueue={driversQueue}
                onChangeDriversQueue={setDriversQueue}
              />
            )}
            {activeSection === "registry" && (
              <RegistryScreen
                hasValidImage={Boolean(
                  inspection?.isValidWin11ClientX64 && selectedIndex !== null
                )}
                isBuilding={isBuilding || recovering || cancellingBuild}
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
                isBuilding={isBuilding || recovering || cancellingBuild}
                filesQueue={filesQueue}
                onChangeFilesQueue={setFilesQueue}
              />
            )}
            {activeSection === "postSetup" && (
              <PostSetupScreen
                hasValidImage={Boolean(
                  inspection?.isValidWin11ClientX64 && selectedIndex !== null
                )}
                canTargetIso={Boolean(
                  inspection?.sourceKind === "iso" && output.target === "iso"
                )}
                autounattendPath={output.autounattendPath}
                outputTarget={output.target}
                isBuilding={isBuilding || recovering || cancellingBuild}
                postSetupQueue={postSetupQueue}
                onChangePostSetupQueue={setPostSetupQueue}
              />
            )}
            {activeSection === "build" && (
              <BuildScreen
                inspection={inspection}
                selectedIndex={selectedIndex}
                appsQueue={appsQueue}
                featuresQueue={featuresQueue}
                servicesQueue={servicesQueue}
                updatesQueue={updatesQueue}
                driversQueue={driversQueue}
                registryQueue={registryQueue}
                filesQueue={filesQueue}
                postSetupQueue={postSetupQueue}
                onChangeUpdatesQueue={setUpdatesQueue}
                output={output}
                onChangeOutput={handleOutputChange}
                onNavigate={setActiveSection}
                isBuilding={isBuilding}
                cancellingBuild={cancellingBuild}
                cancelError={cancelError}
                buildStartedAt={buildStartedAt}
                presetLoading={presetLoading}
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
            </ScreenTransition>
          </div>
        </main>
      </div>

      <LogDrawer
        isOpen={isLogOpen}
        onClose={() => setIsLogOpen(false)}
      />

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
        <Suspense fallback={null}>
          <SettingsModal
            settings={settings}
            envStatus={envStatus}
            isBusy={isGlobalBusy || presetLoading}
            onRefreshEnv={refreshEnvironment}
            onSave={handleSaveSettings}
            onClose={() => setIsSettingsOpen(false)}
          />
        </Suspense>
      )}

      {pendingResetAction && (
        <ConfirmResetModal
          action={pendingResetAction}
          queuedCount={
            pendingResetAction.kind === "edition"
              ? editionResetCount
              : totalQueuedCount
          }
          onCancel={() => setPendingResetAction(null)}
          onConfirm={handleConfirmPendingReset}
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
