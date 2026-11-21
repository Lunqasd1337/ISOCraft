import { useCallback, useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import type {
  AppsQueueState,
  DriverQueueItem,
  DriverScanResult,
  DriversQueueState,
  FeaturesQueueState,
  FileQueueItem,
  FileScanResult,
  FilesQueueState,
  ImageCatalog,
  ImageInspectionResult,
  MissingPresetPath,
  OutputOptions,
  PresetImportResult,
  RegFilePreview,
  RegQueueItem,
  RegistryQueueState,
  StudioPreset,
  UpdateQueueItem,
  UpdateScanInfo,
  UpdatesQueueState,
} from "../types/studio";

export interface PresetBannerState {
  ok: boolean;
  text: string;
}

interface UsePresetWorkflowParams {
  inspection: ImageInspectionResult | null;
  selectedIndex: number | null;
  catalog: ImageCatalog | null;
  appsQueue: AppsQueueState;
  setAppsQueue: Dispatch<SetStateAction<AppsQueueState>>;
  featuresQueue: FeaturesQueueState;
  setFeaturesQueue: Dispatch<SetStateAction<FeaturesQueueState>>;
  updatesQueue: UpdatesQueueState;
  setUpdatesQueue: Dispatch<SetStateAction<UpdatesQueueState>>;
  driversQueue: DriversQueueState;
  setDriversQueue: Dispatch<SetStateAction<DriversQueueState>>;
  registryQueue: RegistryQueueState;
  setRegistryQueue: Dispatch<SetStateAction<RegistryQueueState>>;
  filesQueue: FilesQueueState;
  setFilesQueue: Dispatch<SetStateAction<FilesQueueState>>;
  output: OutputOptions;
  setOutput: Dispatch<SetStateAction<OutputOptions>>;
}

export function usePresetWorkflow({
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
}: UsePresetWorkflowParams) {
  const { t } = useTranslation();
  const [missingPresetPaths, setMissingPresetPaths] = useState<MissingPresetPath[]>([]);
  const [presetBanner, setPresetBanner] = useState<PresetBannerState | null>(null);

  const selectedEdition =
    inspection?.editions.find((e) => e.index === selectedIndex) ?? null;

  const dismissPresetBanner = useCallback(() => {
    setPresetBanner(null);
  }, []);

  const resetPresetState = useCallback(() => {
    setMissingPresetPaths([]);
    setPresetBanner(null);
  }, []);
  const handleSavePreset = async () => {
    if (!inspection || selectedIndex === null) return;
    setPresetBanner(null);
    const chosen = await save({
      defaultPath: "isocraft-preset.json",
      filters: [{ name: "JSON Preset", extensions: ["json"] }],
    });
    if (!chosen) return;

    const familyDisplayMap = new Map(
      catalog?.appxPackages.map((p) => [p.packageFamilyName.toLowerCase(), p.displayName]) ?? []
    );

    const preset: StudioPreset = {
      schemaVersion: "1.0",
      meta: {
        name: selectedEdition ? `ISOCraft - ${selectedEdition.name}` : "ISOCraft Preset",
        description: "",
        createdAt: new Date().toISOString(),
        targetArch: "x64",
        targetEditionHint: selectedEdition?.name ?? null,
      },
      apps: {
        removePackages: appsQueue.removeFamilies.map((fam) => ({
          packageFamilyName: fam,
          displayNameHint: familyDisplayMap.get(fam.toLowerCase()) ?? null,
        })),
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
        items: filesQueue.items.map((f) => ({
          sourcePath: f.sourcePath,
          isDirectory: f.isDirectory,
          target: f.target,
          destinationPath: f.destinationPath,
          conflictPolicy: f.conflictPolicy,
        })),
      },
      output: {
        target: output.target,
        installFormat: output.installFormat,
        splitSizeMb: output.splitSizeMb,
        autounattendPath: output.autounattendPath,
        addGenericEiCfg: output.addGenericEiCfg,
        uefiNoPrompt: output.uefiNoPrompt,
        volumeLabel: output.volumeLabel,
      },
    };

    try {
      const savedPath = await invoke<string>("export_preset", {
        path: chosen,
        preset,
      });
      setPresetBanner({
        ok: true,
        text: t("screens.build.presetSaved", { path: savedPath }),
      });
    } catch (err) {
      setPresetBanner({
        ok: false,
        text: t("screens.build.presetError", { error: String(err) }),
      });
    }
  };

  const handleLoadPreset = async () => {
    setPresetBanner(null);
    const chosen = await open({
      multiple: false,
      filters: [{ name: "JSON Preset", extensions: ["json"] }],
    });
    if (typeof chosen !== "string" || !chosen.trim()) return;

    try {
      const res = await invoke<PresetImportResult>("import_preset", { path: chosen });
      const p = res.preset;
      const isMissing = (kind: MissingPresetPath["kind"], targetPath: string) =>
        res.missingPaths.some(
          (m) => m.kind === kind && m.path.toLowerCase() === targetPath.toLowerCase()
        );

      const seenFam = new Set<string>();
      const removeFamilies: string[] = [];
      for (const entry of p.apps.removePackages) {
        const fam = (
          typeof entry === "string" ? entry : entry.packageFamilyName
        ).trim();
        if (fam && !seenFam.has(fam.toLowerCase())) {
          seenFam.add(fam.toLowerCase());
          removeFamilies.push(fam);
        }
      }
      setAppsQueue({
        removeFamilies,
        addPackages: p.apps.addPackages,
      });

      const resolvedSxs =
        p.features.defaultSxsPath ??
        p.features.enable.find((e) => Boolean(e.customSxsPath))?.customSxsPath ??
        p.features.capabilitiesAdd.find((c) => Boolean(c.customSxsPath))?.customSxsPath ??
        null;
      const featureOverrides: Record<
        string,
        { enableAll: boolean; customSxsPath: string | null }
      > = {};
      for (const f of p.features.enable) {
        featureOverrides[f.name] = {
          enableAll: f.enableAll,
          customSxsPath: f.customSxsPath ?? resolvedSxs,
        };
      }
      const capabilityOverrides: Record<
        string,
        { limitAccess: boolean; customSxsPath: string | null }
      > = {};
      for (const c of p.features.capabilitiesAdd) {
        capabilityOverrides[c.name] = {
          limitAccess: c.limitAccess,
          customSxsPath: c.customSxsPath ?? resolvedSxs,
        };
      }
      setFeaturesQueue({
        enableFeatures: p.features.enable.map((f) => f.name),
        disableFeatures: p.features.disable,
        addCapabilities: p.features.capabilitiesAdd.map((c) => c.name),
        removeCapabilities: p.features.capabilitiesRemove,
        enableAll: p.features.enable[0]?.enableAll ?? true,
        limitAccess: p.features.capabilitiesAdd[0]?.limitAccess ?? true,
        customSxsPath: resolvedSxs,
        featureOverrides,
        capabilityOverrides,
      });

      const loadedUpdates: UpdateQueueItem[] = [];
      for (const u of p.updates.items) {
        if (!isMissing("updateItem", u.path)) {
          try {
            const info = await invoke<UpdateScanInfo>("scan_update_path", {
              path: u.path,
            });
            loadedUpdates.push(info);
            continue;
          } catch {
            // fall through to placeholder
          }
        }
        loadedUpdates.push({
          path: u.path,
          isDirectory: u.isDirectory,
          kind: u.isDirectory
            ? "folder"
            : u.path.toLowerCase().endsWith(".cab")
            ? "cab"
            : "msu",
          packageCount: 0,
          totalBytes: 0,
        });
      }
      setUpdatesQueue({
        items: loadedUpdates,
        runComponentCleanup: p.updates.runComponentCleanup,
        cleanupResetBase: p.updates.cleanupResetBase,
      });

      const canIso = inspection?.sourceKind === "iso";
      const clampedTarget = p.output.target === "iso" && canIso ? "iso" : "image";

      const loadedDrivers: DriverQueueItem[] = [];
      for (const d of p.drivers.items) {
        const inferDir = d.isDirectory ?? !d.path.toLowerCase().endsWith(".inf");
        let scan: DriverScanResult | null = null;
        if (!isMissing("driverItem", d.path)) {
          try {
            scan = await invoke<DriverScanResult>("scan_driver_folder", {
              path: d.path,
            });
          } catch {
            scan = null;
          }
        }
        loadedDrivers.push({
          path: d.path,
          isDirectory: scan ? scan.isDirectory : inferDir,
          infCount: scan?.infCount ?? 0,
          sampleInfs: scan?.sampleInfs ?? [],
          totalBytes: scan?.totalBytes ?? 0,
          recurse: d.recurse,
          forceUnsigned: d.forceUnsigned,
          injectToInstallWim: d.injectToInstallWim,
          injectToBootWim:
            clampedTarget === "iso" && Boolean(inspection?.hasBootWim)
              ? d.injectToBootWim
              : false,
        });
      }
      setDriversQueue({ items: loadedDrivers });

      const loadedRegs: RegQueueItem[] = [];
      for (const r of p.registry.regFiles) {
        let preview: RegFilePreview | null = null;
        if (!isMissing("regFile", r.path)) {
          try {
            preview = await invoke<RegFilePreview>("preview_reg_file", {
              path: r.path,
            });
          } catch {
            preview = null;
          }
        }
        loadedRegs.push({
          path: r.path,
          enabled: r.enabled,
          preview: preview ?? {
            path: r.path,
            valid: false,
            totalSet: 0,
            totalDelete: 0,
            affectedHives: [],
            operations: [],
            warnings: [],
            errors: [t("screens.registry.fileNotFound")],
          },
        });
      }
      setRegistryQueue({ regFiles: loadedRegs });

      const loadedFiles: FileQueueItem[] = [];
      for (const f of p.files?.items ?? []) {
        let scan: FileScanResult | null = null;
        if (!isMissing("customFile", f.sourcePath)) {
          try {
            scan = await invoke<FileScanResult>("scan_custom_file_path", {
              path: f.sourcePath,
            });
          } catch {
            scan = null;
          }
        }
        const id =
          typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
            ? crypto.randomUUID()
            : `${f.sourcePath}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
        loadedFiles.push({
          id,
          sourcePath: f.sourcePath,
          isDirectory: scan ? scan.isDirectory : f.isDirectory,
          fileCount: scan?.fileCount ?? 0,
          totalBytes: scan?.totalBytes ?? 0,
          target: f.target,
          destinationPath: f.destinationPath,
          conflictPolicy: f.conflictPolicy,
        });
      }
      setFilesQueue({ items: loadedFiles });

      setOutput((prev) => {
        let nextOutPath = prev.outputPath;
        if (nextOutPath) {
          const ext = clampedTarget === "iso" ? "iso" : p.output.installFormat;
          nextOutPath = nextOutPath.replace(/\.(iso|wim|esd|swm)$/i, `.${ext}`);
        }
        return {
          target: clampedTarget,
          installFormat: p.output.installFormat,
          splitSizeMb: p.output.splitSizeMb,
          outputPath: nextOutPath,
          autounattendPath: p.output.autounattendPath,
          addGenericEiCfg: p.output.addGenericEiCfg,
          uefiNoPrompt: p.output.uefiNoPrompt,
          volumeLabel: p.output.volumeLabel,
        };
      });

      setMissingPresetPaths(res.missingPaths);
      setPresetBanner({
        ok: true,
        text:
          res.missingPaths.length > 0
            ? t("screens.build.presetLoadedWithMissing", {
                path: chosen,
                count: res.missingPaths.length,
              })
            : t("screens.build.presetLoaded", { path: chosen }),
      });
    } catch (err) {
      setPresetBanner({
        ok: false,
        text: t("screens.build.presetError", { error: String(err) }),
      });
    }
  };

  const handleRelinkMissingPath = async (missing: MissingPresetPath) => {
    setPresetBanner(null);
    let filters: { name: string; extensions: string[] }[] | undefined;
    if (!missing.expectedDirectory) {
      switch (missing.kind) {
        case "appxPackage":
        case "appxDependency":
          filters = [
            { name: "AppX / MSIX", extensions: ["appx", "appxbundle", "msix", "msixbundle"] },
          ];
          break;
        case "appxLicense":
        case "autounattendXml":
          filters = [{ name: "XML", extensions: ["xml"] }];
          break;
        case "updateItem":
          filters = [{ name: "Windows Updates", extensions: ["msu", "cab"] }];
          break;
        case "driverItem":
          filters = [{ name: "Driver INF", extensions: ["inf"] }];
          break;
        case "regFile":
          filters = [{ name: "Registry File", extensions: ["reg"] }];
          break;
        default:
          filters = undefined;
      }
    }

    const picked = await open({
      directory: missing.expectedDirectory,
      multiple: false,
      filters,
    });
    if (typeof picked !== "string" || !picked.trim()) return;

    const oldLower = missing.path.toLowerCase();
    const parentLower = missing.parentPath?.toLowerCase() ?? null;

    try {
      switch (missing.kind) {
        case "appxPackage": {
          setAppsQueue((prev) => ({
            ...prev,
            addPackages: prev.addPackages.map((pkg) =>
              pkg.packagePath.toLowerCase() === oldLower
                ? { ...pkg, packagePath: picked }
                : pkg
            ),
          }));
          setMissingPresetPaths((prev) =>
            prev
              .filter((m) => m.id !== missing.id)
              .map((m) =>
                m.parentPath && m.parentPath.toLowerCase() === oldLower
                  ? { ...m, parentPath: picked }
                  : m
              )
          );
          return;
        }
        case "appxDependency": {
          setAppsQueue((prev) => ({
            ...prev,
            addPackages: prev.addPackages.map((pkg) => {
              if (parentLower && pkg.packagePath.toLowerCase() !== parentLower) {
                return pkg;
              }
              return {
                ...pkg,
                dependencyPaths: pkg.dependencyPaths.map((d) =>
                  d.toLowerCase() === oldLower ? picked : d
                ),
              };
            }),
          }));
          break;
        }
        case "appxLicense": {
          setAppsQueue((prev) => ({
            ...prev,
            addPackages: prev.addPackages.map((pkg) => {
              if (parentLower && pkg.packagePath.toLowerCase() !== parentLower) {
                return pkg;
              }
              if (pkg.licensePath?.toLowerCase() === oldLower) {
                return { ...pkg, licensePath: picked, skipLicense: false };
              }
              return pkg;
            }),
          }));
          break;
        }
        case "featuresSxs": {
          setFeaturesQueue((prev) => {
            const nextFeat = { ...(prev.featureOverrides ?? {}) };
            for (const [k, v] of Object.entries(nextFeat)) {
              if (v.customSxsPath?.toLowerCase() === oldLower) {
                nextFeat[k] = { ...v, customSxsPath: picked };
              }
            }
            const nextCap = { ...(prev.capabilityOverrides ?? {}) };
            for (const [k, v] of Object.entries(nextCap)) {
              if (v.customSxsPath?.toLowerCase() === oldLower) {
                nextCap[k] = { ...v, customSxsPath: picked };
              }
            }
            return {
              ...prev,
              customSxsPath:
                !prev.customSxsPath || prev.customSxsPath.toLowerCase() === oldLower
                  ? picked
                  : prev.customSxsPath,
              featureOverrides: nextFeat,
              capabilityOverrides: nextCap,
            };
          });
          break;
        }
        case "updateItem": {
          const info = await invoke<UpdateScanInfo>("scan_update_path", {
            path: picked,
          });
          setUpdatesQueue((prev) => {
            const next: UpdateQueueItem[] = [];
            const seen = new Set<string>();
            for (const item of prev.items) {
              const candidate =
                item.path.toLowerCase() === oldLower ? info : item;
              const k = candidate.path.toLowerCase();
              if (!seen.has(k)) {
                seen.add(k);
                next.push(candidate);
              }
            }
            return { ...prev, items: next };
          });
          break;
        }
        case "driverItem": {
          const scan = await invoke<DriverScanResult>("scan_driver_folder", {
            path: picked,
          });
          setDriversQueue((prev) => ({
            items: prev.items.map((d) =>
              d.path.toLowerCase() === oldLower
                ? {
                    ...d,
                    path: scan.path,
                    isDirectory: scan.isDirectory,
                    infCount: scan.infCount,
                    sampleInfs: scan.sampleInfs,
                    totalBytes: scan.totalBytes,
                  }
                : d
            ),
          }));
          break;
        }
        case "regFile": {
          const preview = await invoke<RegFilePreview>("preview_reg_file", {
            path: picked,
          });
          setRegistryQueue((prev) => ({
            regFiles: prev.regFiles.map((r) =>
              r.path.toLowerCase() === oldLower
                ? { ...r, path: preview.path, preview }
                : r
            ),
          }));
          break;
        }
        case "customFile": {
          const scan = await invoke<FileScanResult>("scan_custom_file_path", {
            path: picked,
          });
          setFilesQueue((prev) => ({
            items: prev.items.map((f) =>
              f.sourcePath.toLowerCase() === oldLower
                ? {
                    ...f,
                    sourcePath: scan.path,
                    isDirectory: scan.isDirectory,
                    fileCount: scan.fileCount,
                    totalBytes: scan.totalBytes,
                  }
                : f
            ),
          }));
          break;
        }
        case "autounattendXml": {
          setOutput((prev) => ({ ...prev, autounattendPath: picked }));
          break;
        }
      }

      setMissingPresetPaths((prev) => prev.filter((m) => m.id !== missing.id));
    } catch (err) {
      setPresetBanner({
        ok: false,
        text: t("screens.build.presetError", { error: String(err) }),
      });
    }
  };

  const handleRemoveMissingPath = (missing: MissingPresetPath) => {
    const oldLower = missing.path.toLowerCase();
    const parentLower = missing.parentPath?.toLowerCase() ?? null;

    switch (missing.kind) {
      case "appxPackage":
        setAppsQueue((prev) => ({
          ...prev,
          addPackages: prev.addPackages.filter(
            (pkg) => pkg.packagePath.toLowerCase() !== oldLower
          ),
        }));
        setMissingPresetPaths((prev) =>
          prev.filter(
            (m) =>
              m.id !== missing.id &&
              (!m.parentPath || m.parentPath.toLowerCase() !== oldLower)
          )
        );
        return;
      case "appxDependency":
        setAppsQueue((prev) => ({
          ...prev,
          addPackages: prev.addPackages.map((pkg) => {
            if (parentLower && pkg.packagePath.toLowerCase() !== parentLower) {
              return pkg;
            }
            return {
              ...pkg,
              dependencyPaths: pkg.dependencyPaths.filter(
                (d) => d.toLowerCase() !== oldLower
              ),
            };
          }),
        }));
        break;
      case "appxLicense":
        setAppsQueue((prev) => ({
          ...prev,
          addPackages: prev.addPackages.map((pkg) => {
            if (parentLower && pkg.packagePath.toLowerCase() !== parentLower) {
              return pkg;
            }
            if (pkg.licensePath?.toLowerCase() === oldLower) {
              return { ...pkg, licensePath: null, skipLicense: true };
            }
            return pkg;
          }),
        }));
        break;
      case "featuresSxs":
        setFeaturesQueue((prev) => {
          const nextFeat = { ...(prev.featureOverrides ?? {}) };
          for (const [k, v] of Object.entries(nextFeat)) {
            if (v.customSxsPath?.toLowerCase() === oldLower) {
              nextFeat[k] = { ...v, customSxsPath: null };
            }
          }
          const nextCap = { ...(prev.capabilityOverrides ?? {}) };
          for (const [k, v] of Object.entries(nextCap)) {
            if (v.customSxsPath?.toLowerCase() === oldLower) {
              nextCap[k] = { ...v, customSxsPath: null };
            }
          }
          return {
            ...prev,
            customSxsPath:
              prev.customSxsPath?.toLowerCase() === oldLower
                ? null
                : prev.customSxsPath,
            featureOverrides: nextFeat,
            capabilityOverrides: nextCap,
          };
        });
        break;
      case "updateItem":
        setUpdatesQueue((prev) => ({
          ...prev,
          items: prev.items.filter((u) => u.path.toLowerCase() !== oldLower),
        }));
        break;
      case "driverItem":
        setDriversQueue((prev) => ({
          items: prev.items.filter((d) => d.path.toLowerCase() !== oldLower),
        }));
        break;
      case "regFile":
        setRegistryQueue((prev) => ({
          regFiles: prev.regFiles.filter((r) => r.path.toLowerCase() !== oldLower),
        }));
        break;
      case "customFile":
        setFilesQueue((prev) => ({
          items: prev.items.filter(
            (f) => f.sourcePath.toLowerCase() !== oldLower
          ),
        }));
        break;
      case "autounattendXml":
        setOutput((prev) => ({ ...prev, autounattendPath: null }));
        break;
    }

    setMissingPresetPaths((prev) => prev.filter((m) => m.id !== missing.id));
  };

  return {
    missingPresetPaths,
    presetBanner,
    dismissPresetBanner,
    resetPresetState,
    handleSavePreset,
    handleLoadPreset,
    handleRelinkMissingPath,
    handleRemoveMissingPath,
  };
}
