import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  Cog,
  Cpu,
  Download,
  FileCode2,
  FolderOpen,
  FolderPlus,
  LayoutGrid,
  PackageCheck,
  RefreshCw,
  SlidersHorizontal,
  Terminal,
  Upload,
  X,
  XCircle,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { FluentReveal, FluentSegmentedControl } from "../FluentMotion";
import { MissingPresetPathsCard } from "./build/MissingPresetPathsCard";
import { BuildSummaryCard, type BuildSummaryRow } from "./build/BuildSummaryCard";
import { BuildProgressCard, type DiagStatus } from "./build/BuildProgressCard";
import type { PresetBannerState } from "../../hooks/usePresetWorkflow";
import { localizeBackendError, localizeXmlError } from "../../utils/backendErrors";
import type {
  AppsQueueState,
  AutounattendValidation,
  BuildProgressEvent,
  BuildResultState,
  DriversQueueState,
  FeaturesQueueState,
  FilesQueueState,
  ImageInspectionResult,
  InstallOutputFormat,
  MissingPresetPath,
  OutputOptions,
  PostSetupQueueState,
  RegistryQueueState,
  SectionId,
  ServicesQueueState,
  UpdatesQueueState,
} from "../../types/studio";

interface BuildScreenProps {
  inspection: ImageInspectionResult | null;
  selectedIndex: number | null;
  appsQueue: AppsQueueState;
  featuresQueue: FeaturesQueueState;
  servicesQueue: ServicesQueueState;
  updatesQueue: UpdatesQueueState;
  driversQueue: DriversQueueState;
  registryQueue: RegistryQueueState;
  filesQueue: FilesQueueState;
  postSetupQueue: PostSetupQueueState;
  onChangeUpdatesQueue: (next: UpdatesQueueState) => void;
  output: OutputOptions;
  onChangeOutput: (next: OutputOptions) => void;
  onNavigate: (section: SectionId) => void;
  isBuilding: boolean;
  cancellingBuild?: boolean;
  cancelError?: string | null;
  buildStartedAt?: number | null;
  presetLoading?: boolean;
  buildProgress: BuildProgressEvent | null;
  buildResultMessage: BuildResultState | null;
  missingPresetPaths: MissingPresetPath[];
  presetBanner: PresetBannerState | null;
  onDismissPresetBanner: () => void;
  onLoadPreset: () => void;
  onSavePreset: () => void;
  onRelinkMissingPath: (missing: MissingPresetPath) => void;
  onRemoveMissingPath: (missing: MissingPresetPath) => void;
  onCancelBuild: () => void;
}

export function BuildScreen({
  inspection,
  selectedIndex,
  appsQueue,
  featuresQueue,
  servicesQueue,
  updatesQueue,
  driversQueue,
  registryQueue,
  filesQueue,
  postSetupQueue,
  onChangeUpdatesQueue,
  output,
  onChangeOutput,
  onNavigate,
  isBuilding,
  cancellingBuild = false,
  cancelError = null,
  buildStartedAt = null,
  presetLoading = false,
  buildProgress,
  buildResultMessage,
  missingPresetPaths,
  presetBanner,
  onDismissPresetBanner,
  onLoadPreset,
  onSavePreset,
  onRelinkMissingPath,
  onRemoveMissingPath,
  onCancelBuild,
}: BuildScreenProps) {
  const { t } = useTranslation();
  const [xmlStatus, setXmlStatus] = useState<AutounattendValidation | null>(null);
  const [diagStatus, setDiagStatus] = useState<DiagStatus>(null);

  useEffect(() => {
    let active = true;
    if (!output.autounattendPath) {
      setXmlStatus(null);
      return;
    }
    const timer = window.setTimeout(() => {
      invoke<AutounattendValidation>("validate_autounattend", {
        path: output.autounattendPath,
      })
        .then((res) => {
          if (active) setXmlStatus(res);
        })
        .catch((err) => {
          if (active) {
            setXmlStatus({
              valid: false,
              rootElement: null,
              passes: [],
              error: String(err),
            });
          }
        });
    }, 250);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [output.autounattendPath]);

  const canBuildIso = inspection?.sourceKind === "iso";

  const handleBrowseOutput = async () => {
    const ext =
      output.target === "iso"
        ? "iso"
        : output.installFormat === "esd"
        ? "esd"
        : "wim";
    const defaultName =
      output.target === "iso" ? "Win11_Custom_x64.iso" : `install.${ext}`;
    try {
      const dest = await save({
        defaultPath: output.outputPath || defaultName,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      });
      if (dest) {
        onChangeOutput({ ...output, outputPath: dest });
      }
    } catch (err) {
      setDiagStatus({ ok: false, error: String(err) });
    }
  };

  const handleBrowseAutounattend = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: t("dialogs.xmlAnswerFile"), extensions: ["xml"] }],
      });
      if (typeof selected === "string" && selected.trim()) {
        onChangeOutput({ ...output, autounattendPath: selected });
      }
    } catch (err) {
      setDiagStatus({ ok: false, error: String(err) });
    }
  };

  const handleExportDiagnostics = async () => {
    setDiagStatus(null);
    try {
      const dest = await save({
        defaultPath: "isocraft-diagnostics.log",
        filters: [{ name: t("dialogs.logFiles"), extensions: ["log", "txt"] }],
      });
      if (!dest) return;
      const savedPath = await invoke<string>("export_diagnostics", { destPath: dest });
      setDiagStatus({
        ok: true,
        path: savedPath,
      });
    } catch (err) {
      setDiagStatus({
        ok: false,
        error: String(err),
      });
    }
  };

  const appsSummary =
    appsQueue.removeFamilies.length > 0 || appsQueue.addPackages.length > 0
      ? t("screens.build.summaryAppsCount", {
          remove: appsQueue.removeFamilies.length,
          add: appsQueue.addPackages.length,
        })
      : t("screens.build.noQueuedItems");

  const featEnableTotal =
    featuresQueue.enableFeatures.length + featuresQueue.addCapabilities.length;
  const featDisableTotal =
    featuresQueue.disableFeatures.length + featuresQueue.removeCapabilities.length;
  const featuresSummary =
    featEnableTotal > 0 || featDisableTotal > 0
      ? t("screens.build.summaryFeaturesCount", {
          enable: featEnableTotal,
          disable: featDisableTotal,
        })
      : t("screens.build.noQueuedItems");

  const totalUpdatePkgs = updatesQueue.items.reduce(
    (sum, item) => sum + item.packageCount,
    0
  );
  const updatesSummary =
    updatesQueue.items.length > 0
      ? [
          t("screens.build.summaryUpdatesCount", {
            count: updatesQueue.items.length,
            pkgs: totalUpdatePkgs,
          }),
          updatesQueue.runComponentCleanup
            ? t("screens.build.summaryUpdatesCleanup")
            : null,
        ]
          .filter(Boolean)
          .join(" • ")
      : updatesQueue.runComponentCleanup
      ? t("screens.build.summaryUpdatesCleanup")
      : t("screens.build.noQueuedItems");

  const totalDriverInfs = driversQueue.items.reduce(
    (sum, item) => sum + item.infCount,
    0
  );
  const removedDriversCount = driversQueue.removePublishedNames.length;
  const hasBootWimDrivers = driversQueue.items.some(
    (d) => d.injectToBootWim && output.target === "iso"
  );
  const driversSummary =
    driversQueue.items.length > 0 || removedDriversCount > 0
      ? [
          driversQueue.items.length > 0
            ? t("screens.build.summaryDriversCount", {
                count: driversQueue.items.length,
                infs: totalDriverInfs,
              })
            : null,
          removedDriversCount > 0
            ? t("screens.build.summaryDriversRemoved", {
                count: removedDriversCount,
              })
            : null,
          hasBootWimDrivers ? t("screens.build.summaryDriversBootBadge") : null,
        ]
          .filter(Boolean)
          .join(" • ")
      : t("screens.build.noQueuedItems");

  const summaryRows: BuildSummaryRow[] = [
    {
      id: "apps",
      label: t("screens.build.summaryApps"),
      icon: LayoutGrid,
      summary: appsSummary,
    },
    {
      id: "features",
      label: t("screens.build.summaryFeatures"),
      icon: SlidersHorizontal,
      summary: featuresSummary,
    },
    {
      id: "services",
      label: t("screens.build.summaryServices"),
      icon: Cog,
      summary: (() => {
        const total = servicesQueue.changes.length;
        if (total === 0) return t("screens.build.noQueuedItems");
        const disabledCount = servicesQueue.changes.filter(
          (c) => c.startType === "disabled"
        ).length;
        const base = t("screens.build.summaryServicesCount", { count: total });
        return disabledCount > 0
          ? `${base} • ${t("screens.build.summaryServicesDisabled", {
              count: disabledCount,
            })}`
          : base;
      })(),
    },
    {
      id: "updates",
      label: t("screens.build.summaryUpdates"),
      icon: RefreshCw,
      summary: updatesSummary,
    },
    {
      id: "drivers",
      label: t("screens.build.summaryDrivers"),
      icon: Cpu,
      summary: driversSummary,
    },
    {
      id: "registry",
      label: t("screens.build.summaryRegistry"),
      icon: FileCode2,
      summary: (() => {
        const enabledRegs = registryQueue.regFiles.filter((r) => r.enabled);
        if (enabledRegs.length === 0) return t("screens.build.noQueuedItems");
        const set = enabledRegs.reduce((s, r) => s + r.preview.totalSet, 0);
        const del = enabledRegs.reduce((s, r) => s + r.preview.totalDelete, 0);
        const hasErr = enabledRegs.some((r) => !r.preview.valid);
        const base = t("screens.build.summaryRegistryCount", {
          count: enabledRegs.length,
          set,
          del,
        });
        return hasErr
          ? `${base} • ${t("screens.build.summaryRegistryErrors")}`
          : base;
      })(),
    },
    {
      id: "files",
      label: t("screens.build.summaryFiles"),
      icon: FolderPlus,
      summary: (() => {
        const activeFiles = filesQueue.items.filter(
          (f) => f.target !== "isoRoot" || output.target === "iso"
        );
        if (activeFiles.length === 0) return t("screens.build.noQueuedItems");
        const hasIsoFiles = activeFiles.some((f) => f.target === "isoRoot");
        const base = t("screens.build.summaryFilesCount", {
          count: activeFiles.length,
        });
        return hasIsoFiles
          ? `${base} ${t("screens.build.summaryFilesIsoBadge")}`
          : base;
      })(),
    },
    {
      id: "postSetup",
      label: t("screens.build.summaryPostSetup"),
      icon: Terminal,
      summary: (() => {
        const enabled = postSetupQueue.items.filter((i) => i.enabled);
        if (enabled.length === 0) return t("screens.build.noQueuedItems");
        const before = enabled.filter((i) => i.phase === "beforeLogon").length;
        const first = enabled.filter((i) => i.phase === "firstLogon").length;
        const winget = enabled.filter((i) => i.kind === "winget").length;
        return [
          t("screens.build.summaryPostSetupCount", {
            count: enabled.length,
            before,
            first,
          }),
          winget > 0
            ? t("screens.build.summaryPostSetupWinget", { count: winget })
            : null,
        ]
          .filter(Boolean)
          .join(" • ");
      })(),
    },
  ];

  const handleTargetChange = (target: "iso" | "image") => {
    let nextPath = output.outputPath;
    if (nextPath) {
      const ext =
        target === "iso"
          ? "iso"
          : output.installFormat === "esd"
          ? "esd"
          : "wim";
      nextPath = nextPath.replace(/\.(iso|wim|esd)$/i, `.${ext}`);
    }
    onChangeOutput({ ...output, target, outputPath: nextPath });
  };

  const handleFormatChange = (installFormat: InstallOutputFormat) => {
    let nextPath = output.outputPath;
    if (nextPath && output.target === "image") {
      nextPath = nextPath.replace(/\.(wim|esd)$/i, `.${installFormat}`);
    }
    onChangeOutput({ ...output, installFormat, outputPath: nextPath });
  };

  const hasValidImage = Boolean(
    inspection && inspection.isValidWin11ClientX64 && selectedIndex !== null
  );

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.build.title")}
        subtitle={t("screens.build.subtitle")}
        actions={
          <>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding || presetLoading}
              onClick={onLoadPreset}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <Upload className="w-3.5 h-3.5" strokeWidth={1.7} />
              <span>{t("screens.build.loadPreset")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding || presetLoading}
              onClick={onSavePreset}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <Download className="w-3.5 h-3.5" strokeWidth={1.7} />
              <span>{t("screens.build.savePreset")}</span>
            </button>
          </>
        }
      />

      {presetBanner && (
        <FluentReveal
          className={`flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg text-[12.5px] ${
            presetBanner.ok
              ? "bg-status-success-bg text-status-success"
              : "bg-status-danger-bg text-status-danger"
          }`}
        >
          <div className="flex items-center gap-2 min-w-0">
            {presetBanner.ok ? (
              <CheckCircle2 className="w-4 h-4 shrink-0" strokeWidth={1.75} />
            ) : (
              <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
            )}
            <span className="break-all">
              {presetBanner.ok
                ? presetBanner.kind === "saved"
                  ? t("screens.build.presetSaved", { path: presetBanner.path })
                  : presetBanner.kind === "loadedWithMissing"
                  ? t("screens.build.presetLoadedWithMissing", {
                      path: presetBanner.path,
                      count: presetBanner.count,
                    })
                  : t("screens.build.presetLoaded", { path: presetBanner.path })
                : t("screens.build.presetError", {
                    error: localizeBackendError(presetBanner.error),
                  })}
            </span>
          </div>
          <button
            type="button"
            onClick={onDismissPresetBanner}
            aria-label={t("logDrawer.close")}
            className="p-1 rounded hover:bg-subtle transition-colors shrink-0"
          >
            <X className="w-3.5 h-3.5" strokeWidth={1.7} />
          </button>
        </FluentReveal>
      )}

      {!hasValidImage ? (
        <EmptyStateCard
          icon={PackageCheck}
          title={t("screens.build.emptyTitle")}
          description={t("screens.build.emptyDesc")}
        />
      ) : (
        <>
          {!isBuilding && !presetLoading && missingPresetPaths.length > 0 && (
            <MissingPresetPathsCard
              missingPresetPaths={missingPresetPaths}
              onRelinkMissingPath={onRelinkMissingPath}
              onRemoveMissingPath={onRemoveMissingPath}
            />
          )}

          {/* Card 1: Pending changes summary */}
          <BuildSummaryCard
            summaryRows={summaryRows}
            onNavigate={onNavigate}
          />

          {/* Card 2: Output Settings OR Active Build Progress */}
          {isBuilding ? (
            <BuildProgressCard
              buildProgress={buildProgress}
              buildStartedAt={buildStartedAt}
              cancellingBuild={cancellingBuild}
              cancelError={cancelError}
              diagStatus={diagStatus}
              onDismissDiag={() => setDiagStatus(null)}
              onExportDiagnostics={handleExportDiagnostics}
              onCancelBuild={onCancelBuild}
            />
          ) : (
            <FluentReveal className="bg-surface border border-border-subtle rounded-lg p-5 space-y-4">
              <h2 className="text-[14px] font-semibold text-text-primary">
                {t("screens.build.outputTitle")}
              </h2>

              {buildResultMessage && (
                <FluentReveal
                  className={`flex items-center justify-between gap-3 px-3.5 py-2.5 rounded-md text-[12.5px] ${
                    buildResultMessage.ok
                      ? "bg-status-success-bg text-status-success"
                      : "bg-status-danger-bg text-status-danger"
                  }`}
                >
                  <span className="break-all">
                    {buildResultMessage.ok
                      ? t("screens.build.buildDone", {
                          path: buildResultMessage.path,
                        })
                      : t("screens.build.buildError", {
                          error: localizeBackendError(buildResultMessage.error),
                        })}
                  </span>
                  <div className="flex items-center gap-2 shrink-0">
                    {buildResultMessage.ok && (
                      <button
                        type="button"
                        onClick={() => {
                          void invoke("reveal_in_explorer", {
                            path: buildResultMessage.path,
                          }).catch((err) =>
                            setDiagStatus({ ok: false, error: String(err) })
                          );
                        }}
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded bg-surface/80 text-text-primary border border-border-subtle text-[11.5px] transition-all duration-150 active:scale-[0.98]"
                      >
                        <FolderOpen className="w-3 h-3" strokeWidth={1.6} />
                        {t("screens.build.openInExplorer")}
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={handleExportDiagnostics}
                      className="inline-flex items-center gap-1 px-2.5 py-1 rounded bg-surface/80 text-text-primary border border-border-subtle text-[11.5px] transition-all duration-150 active:scale-[0.98]"
                    >
                      <Download className="w-3 h-3" strokeWidth={1.6} />
                      {t("screens.build.exportDiagnostics")}
                    </button>
                  </div>
                </FluentReveal>
              )}

              {diagStatus && (
                <FluentReveal
                  className={`flex items-center justify-between gap-2 px-3.5 py-2 rounded-md text-[12px] ${
                    diagStatus.ok
                      ? "bg-status-success-bg text-status-success"
                      : "bg-status-danger-bg text-status-danger"
                  }`}
                >
                  <span className="break-all">
                    {diagStatus.ok
                      ? t("screens.build.diagnosticsExported", {
                          path: diagStatus.path,
                        })
                      : t("screens.build.diagnosticsExportError", {
                          error: localizeBackendError(diagStatus.error),
                        })}
                  </span>
                  <button
                    type="button"
                    onClick={() => setDiagStatus(null)}
                    aria-label={t("logDrawer.close")}
                    className="p-0.5 rounded hover:bg-subtle transition-colors shrink-0"
                  >
                    <X className="w-3.5 h-3.5" strokeWidth={1.7} />
                  </button>
                </FluentReveal>
              )}

              {/* Target container */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div className="space-y-1.5">
                  <label className="block text-[12.5px] font-medium text-text-secondary">
                    {t("screens.build.targetLabel")}
                  </label>
                  <FluentSegmentedControl<"iso" | "image">
                    value={output.target}
                    onChange={handleTargetChange}
                    variant="surface"
                    size="md"
                    fullWidth
                    options={[
                      {
                        value: "iso",
                        label: t("screens.build.targetIso"),
                        disabled: !canBuildIso,
                      },
                      {
                        value: "image",
                        label: t("screens.build.targetImage"),
                      },
                    ]}
                  />
                  {!canBuildIso && (
                    <p className="text-[11.5px] text-text-secondary">
                      {t("screens.build.targetIsoDisabledHint")}
                    </p>
                  )}
                </div>

                {/* install.* Format */}
                <div className="space-y-1.5">
                  <label className="block text-[12.5px] font-medium text-text-secondary">
                    {t("screens.build.installFormatLabel")}
                  </label>
                  <FluentSegmentedControl<InstallOutputFormat>
                    value={output.installFormat}
                    onChange={handleFormatChange}
                    variant="surface"
                    size="md"
                    fullWidth
                    options={[
                      { value: "wim", label: "WIM" },
                      { value: "esd", label: "ESD" },
                    ]}
                  />
                </div>
              </div>

              {/* Output Path */}
              <div className="space-y-1.5">
                <label className="block text-[12.5px] font-medium text-text-secondary">
                  {t("screens.build.outputPathLabel")}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    value={output.outputPath}
                    placeholder={t("screens.build.outputPathPlaceholder")}
                    onChange={(e) =>
                      onChangeOutput({ ...output, outputPath: e.target.value })
                    }
                    className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent"
                  />
                  <button
                    type="button"
                    onClick={handleBrowseOutput}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] shrink-0"
                  >
                    <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                    {t("screens.build.browse")}
                  </button>
                </div>
              </div>

              {/* ISO-only options */}
              {output.target === "iso" && (
                <FluentReveal className="pt-3 border-t border-border-subtle space-y-3">
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between gap-2">
                      <label className="text-[12.5px] font-medium text-text-secondary">
                        {t("screens.build.autounattendLabel")}
                      </label>
                      {xmlStatus && (
                        <FluentReveal
                          className={`inline-flex items-center gap-1 text-[11.5px] font-medium ${
                            xmlStatus.valid ? "text-status-success" : "text-status-danger"
                          }`}
                        >
                          {xmlStatus.valid ? (
                            <>
                              <CheckCircle2 className="w-3.5 h-3.5" strokeWidth={1.75} />
                              {xmlStatus.passes.length > 0
                                ? t("screens.build.xmlValid", {
                                    passes: xmlStatus.passes.join(", "),
                                  })
                                : t("screens.build.xmlValidNoPass")}
                            </>
                          ) : (
                            <>
                              <XCircle className="w-3.5 h-3.5" strokeWidth={1.75} />
                              {t("screens.build.xmlInvalid", {
                                error: xmlStatus.error
                                  ? localizeXmlError(xmlStatus.error)
                                  : "",
                              })}
                            </>
                          )}
                        </FluentReveal>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      <input
                        type="text"
                        value={output.autounattendPath ?? ""}
                        placeholder={t("screens.build.autounattendPlaceholder")}
                        onChange={(e) =>
                          onChangeOutput({
                            ...output,
                            autounattendPath: e.target.value.trim() ? e.target.value : null,
                          })
                        }
                        className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent"
                      />
                      <button
                        type="button"
                        onClick={handleBrowseAutounattend}
                        className="px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] shrink-0"
                      >
                        {t("screens.build.browse")}
                      </button>
                      {output.autounattendPath && (
                        <button
                          type="button"
                          onClick={() =>
                            onChangeOutput({ ...output, autounattendPath: null })
                          }
                          className="px-2.5 py-1.5 rounded-md text-[12px] text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98]"
                        >
                          {t("common.clear")}
                        </button>
                      )}
                    </div>
                  </div>

                  <div className="grid grid-cols-1 md:grid-cols-2 gap-4 items-center">
                    <div className="space-y-1">
                      <label className="block text-[12.5px] font-medium text-text-secondary">
                        {t("screens.build.volumeLabel")}
                      </label>
                      <input
                        type="text"
                        maxLength={32}
                        value={output.volumeLabel}
                        onChange={(e) =>
                          onChangeOutput({ ...output, volumeLabel: e.target.value })
                        }
                        className="w-full px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent"
                      />
                    </div>

                    <div className="space-y-2 pt-2 md:pt-5">
                      <label className="flex items-center gap-2 text-[12.5px] text-text-primary cursor-pointer">
                        <input
                          type="checkbox"
                          checked={output.uefiNoPrompt}
                          onChange={(e) =>
                            onChangeOutput({ ...output, uefiNoPrompt: e.target.checked })
                          }
                          className="rounded border-border-subtle accent-accent"
                        />
                        <span>{t("screens.build.uefiNoPrompt")}</span>
                      </label>
                      <label className="flex items-center gap-2 text-[12.5px] text-text-primary cursor-pointer">
                        <input
                          type="checkbox"
                          checked={output.addGenericEiCfg}
                          onChange={(e) =>
                            onChangeOutput({ ...output, addGenericEiCfg: e.target.checked })
                          }
                          className="rounded border-border-subtle accent-accent"
                        />
                        <span>{t("screens.build.addGenericEiCfg")}</span>
                      </label>
                    </div>
                  </div>
                </FluentReveal>
              )}

              {/* Component cleanup (applies to both ISO and Image targets) */}
              <div className="pt-3 border-t border-border-subtle space-y-2">
                <label className="flex items-center gap-2 text-[12.5px] text-text-primary cursor-pointer">
                  <input
                    type="checkbox"
                    checked={updatesQueue.runComponentCleanup}
                    onChange={(e) =>
                      onChangeUpdatesQueue({
                        ...updatesQueue,
                        runComponentCleanup: e.target.checked,
                        cleanupResetBase: e.target.checked
                          ? updatesQueue.cleanupResetBase
                          : false,
                      })
                    }
                    className="rounded border-border-subtle accent-accent"
                  />
                  <span>{t("screens.build.cleanupLabel")}</span>
                </label>

                {updatesQueue.runComponentCleanup && (
                  <FluentReveal className="pl-6 space-y-2">
                    <label className="flex items-center gap-2 text-[12px] text-text-secondary cursor-pointer">
                      <input
                        type="checkbox"
                        checked={updatesQueue.cleanupResetBase}
                        onChange={(e) =>
                          onChangeUpdatesQueue({
                            ...updatesQueue,
                            cleanupResetBase: e.target.checked,
                          })
                        }
                        className="rounded border-border-subtle accent-accent"
                      />
                      <span>{t("screens.build.cleanupResetBaseLabel")}</span>
                    </label>
                    {updatesQueue.cleanupResetBase && (
                      <FluentReveal className="flex items-start gap-2 px-3 py-2 rounded-md bg-status-warning-bg text-status-warning text-[12px]">
                        <AlertTriangle
                          className="w-4 h-4 shrink-0 mt-0.5"
                          strokeWidth={1.75}
                        />
                        <span>{t("screens.build.cleanupResetBaseWarning")}</span>
                      </FluentReveal>
                    )}
                  </FluentReveal>
                )}
              </div>
            </FluentReveal>
          )}
        </>
      )}
    </div>
  );
}
