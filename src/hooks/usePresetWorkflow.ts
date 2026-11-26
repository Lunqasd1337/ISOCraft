import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { localizeBackendError } from "../utils/backendErrors";
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
  PostSetupFileScanResult,
  PostSetupQueueState,
  PresetImportResult,
  RegFilePreview,
  RegQueueItem,
  RegistryQueueState,
  StudioPreset,
  UpdateQueueItem,
  UpdateScanInfo,
  UpdatesQueueState,
} from "../types/studio";

export type PresetBannerState =
  | { ok: true; kind: "saved" | "loaded"; path: string }
  | { ok: true; kind: "loadedWithMissing"; path: string; count: number }
  | { ok: false; kind: "error"; error: string };

interface UsePresetWorkflowParams {
  inspection: ImageInspectionResult | null;
  selectedIndex: number | null;
  catalog: ImageCatalog | null;
  isBusy?: boolean;
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
  postSetupQueue: PostSetupQueueState;
  setPostSetupQueue: Dispatch<SetStateAction<PostSetupQueueState>>;
  output: OutputOptions;
  setOutput: Dispatch<SetStateAction<OutputOptions>>;
}

const UNSAFE_KEYS = new Set(["__proto__", "constructor", "prototype"]);

function safeOwnValue<T>(
  record: Record<string, T> | undefined,
  key: string
): T | undefined {
  if (!record || UNSAFE_KEYS.has(key) || !Object.hasOwn(record, key)) {
    return undefined;
  }
  return record[key];
}

export function usePresetWorkflow({
  inspection,
  selectedIndex,
  catalog,
  isBusy = false,
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
  postSetupQueue,
  setPostSetupQueue,
  output,
  setOutput,
}: UsePresetWorkflowParams) {
  const { t } = useTranslation();
  const [missingPresetPaths, setMissingPresetPaths] = useState<MissingPresetPath[]>([]);
  const [presetBanner, setPresetBanner] = useState<PresetBannerState | null>(null);
  const [presetLoading, setPresetLoading] = useState(false);
  const presetSeqRef = useRef(0);
  const inspectionRef = useRef(inspection);
  inspectionRef.current = inspection;

  const selectedEdition =
    inspection?.editions.find((e) => e.index === selectedIndex) ?? null;

  useEffect(() => {
    setMissingPresetPaths((prev) => {
      if (prev.length === 0) return prev;
      const pkgSet = new Set(appsQueue.addPackages.map((p) => p.packagePath.toLowerCase()));
      const updSet = new Set(updatesQueue.items.map((u) => u.path.toLowerCase()));
      const drvSet = new Set(driversQueue.items.map((d) => d.path.toLowerCase()));
      const regSet = new Set(registryQueue.regFiles.map((r) => r.path.toLowerCase()));
      const fileSet = new Set(filesQueue.items.map((f) => f.sourcePath.toLowerCase()));
      const postSetupSet = new Set(
        postSetupQueue.items
          .map((i) => i.sourcePath?.toLowerCase() ?? "")
          .filter(Boolean)
      );
      const sxsSet = new Set<string>();
      if (featuresQueue.customSxsPath) {
        sxsSet.add(featuresQueue.customSxsPath.toLowerCase());
      }
      for (const v of Object.values(featuresQueue.featureOverrides ?? {})) {
        if (v.customSxsPath) sxsSet.add(v.customSxsPath.toLowerCase());
      }
      for (const v of Object.values(featuresQueue.capabilityOverrides ?? {})) {
        if (v.customSxsPath) sxsSet.add(v.customSxsPath.toLowerCase());
      }
      const unattendLower = output.autounattendPath?.toLowerCase() ?? null;

      const filtered = prev.filter((m) => {
        const pLow = m.path.toLowerCase();
        switch (m.kind) {
          case "appxPackage":
            return pkgSet.has(pLow);
          case "appxDependency":
            return appsQueue.addPackages.some(
              (pkg) =>
                (!m.parentPath ||
                  pkg.packagePath.toLowerCase() === m.parentPath.toLowerCase()) &&
                pkg.dependencyPaths.some((d) => d.toLowerCase() === pLow)
            );
          case "appxLicense":
            return appsQueue.addPackages.some(
              (pkg) =>
                (!m.parentPath ||
                  pkg.packagePath.toLowerCase() === m.parentPath.toLowerCase()) &&
                !pkg.skipLicense &&
                pkg.licensePath?.toLowerCase() === pLow
            );
          case "featuresSxs":
            return sxsSet.has(pLow);
          case "updateItem":
            return updSet.has(pLow);
          case "driverItem":
            return drvSet.has(pLow);
          case "regFile":
            return regSet.has(pLow);
          case "customFile":
            return fileSet.has(pLow);
          case "postSetupFile":
            return postSetupSet.has(pLow);
          case "autounattendXml":
            return unattendLower === pLow;
          default:
            return true;
        }
      });
      return filtered.length === prev.length ? prev : filtered;
    });
  }, [
    appsQueue,
    featuresQueue,
    updatesQueue,
    driversQueue,
    registryQueue,
    filesQueue,
    postSetupQueue,
    output.autounattendPath,
  ]);

  const dismissPresetBanner = useCallback(() => {
    setPresetBanner(null);
  }, []);

  const resetPresetState = useCallback(() => {
    presetSeqRef.current += 1;
    setPresetLoading(false);
    setMissingPresetPaths([]);
    setPresetBanner(null);
  }, []);

  const handleSavePreset = async () => {
    if (!inspection || selectedIndex === null || isBusy || presetLoading) return;
    setPresetBanner(null);
    let chosen: string | null = null;
    try {
      const picked = await save({
        defaultPath: "isocraft-preset.json",
        filters: [{ name: t("dialogs.jsonPreset"), extensions: ["json"] }],
      });
      if (typeof picked === "string" && picked.trim()) {
        chosen = picked;
      }
    } catch (err) {
      setPresetBanner({
        ok: false,
        kind: "error",
        error: localizeBackendError(String(err)),
      });
      return;
    }
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
        enable: featuresQueue.enableFeatures.map((name) => {
          const ov = safeOwnValue(featuresQueue.featureOverrides, name);
          return {
            name,
            enableAll: ov?.enableAll ?? featuresQueue.enableAll,
            customSxsPath: ov?.customSxsPath ?? featuresQueue.customSxsPath,
          };
        }),
        disable: featuresQueue.disableFeatures,
        capabilitiesAdd: featuresQueue.addCapabilities.map((name) => {
          const ov = safeOwnValue(featuresQueue.capabilityOverrides, name);
          return {
            name,
            limitAccess: ov?.limitAccess ?? featuresQueue.limitAccess,
            customSxsPath: ov?.customSxsPath ?? featuresQueue.customSxsPath,
          };
        }),
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
        removePublishedNames: driversQueue.removePublishedNames,
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
      postSetup: {
        ...postSetupQueue,
        firstLogonMode:
          output.target === "iso" ? postSetupQueue.firstLogonMode : "runOnce",
      },
      output: {
        target: output.target,
        installFormat: output.installFormat,
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
        kind: "saved",
        path: savedPath,
      });
    } catch (err) {
      setPresetBanner({
        ok: false,
        kind: "error",
        error: localizeBackendError(String(err)),
      });
    }
  };

  const handleLoadPreset = async () => {
    if (isBusy || presetLoading) return;
    setPresetBanner(null);
    let chosen: string | null = null;
    try {
      const picked = await open({
        multiple: false,
        filters: [{ name: t("dialogs.jsonPreset"), extensions: ["json"] }],
      });
      if (typeof picked === "string" && picked.trim()) {
        chosen = picked;
      }
    } catch (err) {
      setPresetBanner({
        ok: false,
        kind: "error",
        error: localizeBackendError(String(err)),
      });
      return;
    }
    if (!chosen) return;

    const seq = ++presetSeqRef.current;
    setPresetLoading(true);
    try {
      const res = await invoke<PresetImportResult>("import_preset", { path: chosen });
      if (seq !== presetSeqRef.current) return;
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
      const seenPkg = new Set<string>();
      const addPackages = p.apps.addPackages.filter((pkg) => {
        const key = pkg.packagePath.trim().toLowerCase();
        if (!key || seenPkg.has(key)) return false;
        seenPkg.add(key);
        return true;
      });

      const resolvedSxs =
        p.features.defaultSxsPath ??
        p.features.enable.find((e) => Boolean(e.customSxsPath))?.customSxsPath ??
        p.features.capabilitiesAdd.find((c) => Boolean(c.customSxsPath))?.customSxsPath ??
        null;
      const featureOverrides: Record<
        string,
        { enableAll: boolean; customSxsPath: string | null }
      > = Object.create(null);
      for (const f of p.features.enable) {
        if (!UNSAFE_KEYS.has(f.name)) {
          featureOverrides[f.name] = {
            enableAll: f.enableAll,
            customSxsPath: f.customSxsPath ?? resolvedSxs,
          };
        }
      }
      const capabilityOverrides: Record<
        string,
        { limitAccess: boolean; customSxsPath: string | null }
      > = Object.create(null);
      for (const c of p.features.capabilitiesAdd) {
        if (!UNSAFE_KEYS.has(c.name)) {
          capabilityOverrides[c.name] = {
            limitAccess: c.limitAccess,
            customSxsPath: c.customSxsPath ?? resolvedSxs,
          };
        }
      }

      const mapWithConcurrency = async <T, R>(
        items: readonly T[],
        limit: number,
        fn: (item: T) => Promise<R>
      ): Promise<R[]> => {
        if (items.length === 0) return [];
        const results = new Array<R>(items.length);
        let nextIndex = 0;
        const workerCount = Math.min(limit, items.length);
        await Promise.all(
          Array.from({ length: workerCount }, async () => {
            while (nextIndex < items.length) {
              if (seq !== presetSeqRef.current) return;
              const idx = nextIndex;
              nextIndex += 1;
              results[idx] = await fn(items[idx]);
            }
          })
        );
        return results;
      };

      const loadedUpdates = await mapWithConcurrency(
        p.updates.items,
        4,
        async (u): Promise<UpdateQueueItem> => {
          if (!isMissing("updateItem", u.path)) {
            try {
              return await invoke<UpdateScanInfo>("scan_update_path", {
                path: u.path,
              });
            } catch {
              // fall through to placeholder
            }
          }
          return {
            path: u.path,
            isDirectory: u.isDirectory,
            kind: u.isDirectory
              ? "folder"
              : u.path.toLowerCase().endsWith(".cab")
              ? "cab"
              : "msu",
            packageCount: 0,
            totalBytes: 0,
          };
        }
      );
      if (seq !== presetSeqRef.current) return;

      const latestInspectionBeforeDrivers = inspectionRef.current;
      const canIsoNow = latestInspectionBeforeDrivers?.sourceKind === "iso";
      const clampedTarget = p.output.target === "iso" && canIsoNow ? "iso" : "image";

      const loadedDrivers = await mapWithConcurrency(
        p.drivers.items,
        4,
        async (d): Promise<DriverQueueItem> => {
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
          return {
            path: d.path,
            isDirectory: scan ? scan.isDirectory : inferDir,
            infCount: scan?.infCount ?? 0,
            sampleInfs: scan?.sampleInfs ?? [],
            totalBytes: scan?.totalBytes ?? 0,
            recurse: d.recurse,
            forceUnsigned: d.forceUnsigned,
            injectToInstallWim: d.injectToInstallWim,
            injectToBootWim:
              clampedTarget === "iso" && Boolean(latestInspectionBeforeDrivers?.hasBootWim)
                ? d.injectToBootWim
                : false,
          };
        }
      );
      if (seq !== presetSeqRef.current) return;

      const loadedRegs = await mapWithConcurrency(
        p.registry.regFiles,
        4,
        async (r): Promise<RegQueueItem> => {
          const missingReg = isMissing("regFile", r.path);
          let preview: RegFilePreview | null = null;
          let readError: string | null = null;
          if (!missingReg) {
            try {
              preview = await invoke<RegFilePreview>("preview_reg_file", {
                path: r.path,
              });
            } catch (err) {
              readError = localizeBackendError(String(err));
              preview = null;
            }
          }
          return {
            path: r.path,
            enabled: r.enabled,
            missingFile: missingReg,
            preview: preview ?? {
              path: r.path,
              valid: false,
              totalSet: 0,
              totalDelete: 0,
              affectedHives: [],
              operations: [],
              warnings: [],
              errors: readError ? [readError] : [],
            },
          };
        }
      );
      if (seq !== presetSeqRef.current) return;

      const loadedFiles = await mapWithConcurrency(
        p.files?.items ?? [],
        4,
        async (f): Promise<FileQueueItem> => {
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
          return {
            id,
            sourcePath: f.sourcePath,
            isDirectory: scan ? scan.isDirectory : f.isDirectory,
            fileCount: scan?.fileCount ?? 0,
            totalBytes: scan?.totalBytes ?? 0,
            target: f.target,
            destinationPath: f.destinationPath,
            conflictPolicy: f.conflictPolicy,
          };
        }
      );

      if (seq !== presetSeqRef.current) return;

      const latestInspection = inspectionRef.current;
      const latestCanIso = latestInspection?.sourceKind === "iso";
      const finalTarget = p.output.target === "iso" && latestCanIso ? "iso" : "image";

      setAppsQueue({
        removeFamilies,
        addPackages,
      });
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
      setUpdatesQueue({
        items: loadedUpdates,
        runComponentCleanup: p.updates.runComponentCleanup,
        cleanupResetBase: p.updates.cleanupResetBase,
      });
      setDriversQueue({
        items: loadedDrivers.map((d) => ({
          ...d,
          injectToBootWim:
            finalTarget === "iso" && Boolean(latestInspection?.hasBootWim)
              ? d.injectToBootWim
              : false,
        })),
        removePublishedNames: Array.from(
          new Set(
            (p.drivers.removePublishedNames ?? [])
              .map((n) => n.trim())
              .filter(Boolean)
          )
        ),
      });
      setRegistryQueue({ regFiles: loadedRegs });
      setFilesQueue({ items: loadedFiles });
      setPostSetupQueue({
        items: (p.postSetup?.items ?? []).map((item, idx) => ({
          ...item,
          id:
            item.id && item.id.trim()
              ? item.id
              : `${item.kind}-${idx}-${Date.now()}`,
        })),
        firstLogonMode:
          finalTarget === "iso" && p.postSetup?.firstLogonMode === "autounattend"
            ? "autounattend"
            : "runOnce",
        cleanupTempFiles: p.postSetup?.cleanupTempFiles ?? true,
        waitForNetwork: p.postSetup?.waitForNetwork ?? true,
      });

      setOutput((prev) => {
        let nextOutPath = prev.outputPath;
        if (nextOutPath) {
          const ext = finalTarget === "iso" ? "iso" : p.output.installFormat;
          nextOutPath = nextOutPath.replace(/\.(iso|wim|esd)$/i, `.${ext}`);
        }
        return {
          target: finalTarget,
          installFormat: p.output.installFormat,
          outputPath: nextOutPath,
          autounattendPath: p.output.autounattendPath,
          addGenericEiCfg: p.output.addGenericEiCfg,
          uefiNoPrompt: p.output.uefiNoPrompt,
          volumeLabel: p.output.volumeLabel,
        };
      });

      setMissingPresetPaths(res.missingPaths);
      setPresetBanner(
        res.missingPaths.length > 0
          ? {
              ok: true,
              kind: "loadedWithMissing",
              path: chosen,
              count: res.missingPaths.length,
            }
          : {
              ok: true,
              kind: "loaded",
              path: chosen,
            }
      );
    } catch (err) {
      if (seq === presetSeqRef.current) {
        setPresetBanner({
          ok: false,
          kind: "error",
          error: localizeBackendError(String(err)),
        });
      }
    } finally {
      if (seq === presetSeqRef.current) {
        setPresetLoading(false);
      }
    }
  };

  const handleRelinkMissingPath = async (missing: MissingPresetPath) => {
    if (isBusy || presetLoading) return;
    setPresetBanner(null);
    let filters: { name: string; extensions: string[] }[] | undefined;
    if (!missing.expectedDirectory) {
      switch (missing.kind) {
        case "appxPackage":
        case "appxDependency":
          filters = [
            { name: t("dialogs.appxPackages"), extensions: ["appx", "appxbundle", "msix", "msixbundle"] },
          ];
          break;
        case "appxLicense":
        case "autounattendXml":
          filters = [{ name: t("dialogs.xmlFiles"), extensions: ["xml"] }];
          break;
        case "updateItem":
          filters = [{ name: t("dialogs.windowsUpdates"), extensions: ["msu", "cab"] }];
          break;
        case "driverItem":
          filters = [{ name: t("dialogs.driverInf"), extensions: ["inf"] }];
          break;
        case "regFile":
          filters = [{ name: t("dialogs.registryFiles"), extensions: ["reg"] }];
          break;
        case "postSetupFile":
          filters = [
            {
              name: t("dialogs.postSetupFiles"),
              extensions: ["exe", "msi", "ps1", "cmd", "bat"],
            },
          ];
          break;
        default:
          filters = undefined;
      }
    }

    let pickedPath: string | null = null;
    try {
      const picked = await open({
        directory: missing.expectedDirectory,
        multiple: false,
        filters,
      });
      if (typeof picked === "string" && picked.trim()) {
        pickedPath = picked;
      }
    } catch (err) {
      setPresetBanner({
        ok: false,
        kind: "error",
        error: localizeBackendError(String(err)),
      });
      return;
    }
    if (!pickedPath) return;

    const picked = pickedPath;
    const oldLower = missing.path.toLowerCase();
    const parentLower = missing.parentPath?.toLowerCase() ?? null;
    const seq = presetSeqRef.current;

    try {
      switch (missing.kind) {
        case "appxPackage": {
          setAppsQueue((prev) => {
            const seen = new Set<string>();
            const nextPkgs = [];
            for (const pkg of prev.addPackages) {
              const updated =
                pkg.packagePath.toLowerCase() === oldLower
                  ? { ...pkg, packagePath: picked }
                  : pkg;
              const k = updated.packagePath.toLowerCase();
              if (!seen.has(k)) {
                seen.add(k);
                nextPkgs.push(updated);
              }
            }
            return { ...prev, addPackages: nextPkgs };
          });
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
              const seen = new Set<string>();
              const nextDeps: string[] = [];
              for (const d of pkg.dependencyPaths) {
                const candidate = d.toLowerCase() === oldLower ? picked : d;
                const k = candidate.toLowerCase();
                if (!seen.has(k)) {
                  seen.add(k);
                  nextDeps.push(candidate);
                }
              }
              return { ...pkg, dependencyPaths: nextDeps };
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
            const nextFeat: Record<string, { enableAll: boolean; customSxsPath: string | null }> =
              Object.assign(Object.create(null), prev.featureOverrides ?? {});
            for (const [k, v] of Object.entries(nextFeat)) {
              if (!UNSAFE_KEYS.has(k) && v.customSxsPath?.toLowerCase() === oldLower) {
                nextFeat[k] = { ...v, customSxsPath: picked };
              }
            }
            const nextCap: Record<string, { limitAccess: boolean; customSxsPath: string | null }> =
              Object.assign(Object.create(null), prev.capabilityOverrides ?? {});
            for (const [k, v] of Object.entries(nextCap)) {
              if (!UNSAFE_KEYS.has(k) && v.customSxsPath?.toLowerCase() === oldLower) {
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
          if (seq !== presetSeqRef.current) return;
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
          if (seq !== presetSeqRef.current) return;
          setDriversQueue((prev) => {
            const seen = new Set<string>();
            const next: DriverQueueItem[] = [];
            for (const d of prev.items) {
              const candidate =
                d.path.toLowerCase() === oldLower
                  ? {
                      ...d,
                      path: scan.path,
                      isDirectory: scan.isDirectory,
                      infCount: scan.infCount,
                      sampleInfs: scan.sampleInfs,
                      totalBytes: scan.totalBytes,
                    }
                  : d;
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
        case "regFile": {
          const preview = await invoke<RegFilePreview>("preview_reg_file", {
            path: picked,
          });
          if (seq !== presetSeqRef.current) return;
          setRegistryQueue((prev) => {
            const seen = new Set<string>();
            const next: RegQueueItem[] = [];
            for (const r of prev.regFiles) {
              const candidate =
                r.path.toLowerCase() === oldLower
                  ? { ...r, path: preview.path, missingFile: false, preview }
                  : r;
              const k = candidate.path.toLowerCase();
              if (!seen.has(k)) {
                seen.add(k);
                next.push(candidate);
              }
            }
            return { regFiles: next };
          });
          break;
        }
        case "customFile": {
          const scan = await invoke<FileScanResult>("scan_custom_file_path", {
            path: picked,
          });
          if (seq !== presetSeqRef.current) return;
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
        case "postSetupFile": {
          const scan = await invoke<PostSetupFileScanResult>("scan_post_setup_file", {
            path: picked,
          });
          if (seq !== presetSeqRef.current) return;
          setPostSetupQueue((prev) => ({
            ...prev,
            items: prev.items.map((item) =>
              item.sourcePath?.toLowerCase() === oldLower
                ? {
                    ...item,
                    kind: scan.kind,
                    sourcePath: scan.path,
                    displayName: scan.fileName,
                    phase:
                      scan.kind === "installer" || item.phase === "firstLogon"
                        ? item.phase
                        : "firstLogon",
                  }
                : item
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
        kind: "error",
        error: localizeBackendError(String(err)),
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
          ...prev,
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
      case "postSetupFile":
        setPostSetupQueue((prev) => ({
          ...prev,
          items: prev.items.filter(
            (i) => i.sourcePath?.toLowerCase() !== oldLower
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
    presetLoading,
    dismissPresetBanner,
    resetPresetState,
    handleSavePreset,
    handleLoadPreset,
    handleRelinkMissingPath,
    handleRemoveMissingPath,
  };
}
