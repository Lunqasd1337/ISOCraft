import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  Cpu,
  Download,
  FileCode2,
  FolderOpen,
  FolderPlus,
  LayoutGrid,
  Loader2,
  PackageCheck,
  RefreshCw,
  SlidersHorizontal,
  Trash2,
  Upload,
  X,
  XCircle,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import type {
  AppsQueueState,
  AutounattendValidation,
  BuildProgressEvent,
  DriversQueueState,
  FeaturesQueueState,
  FilesQueueState,
  ImageInspectionResult,
  InstallOutputFormat,
  MissingPresetPath,
  OutputOptions,
  RegistryQueueState,
  SectionId,
  UpdatesQueueState,
} from "../../types/studio";

interface BuildScreenProps {
  inspection: ImageInspectionResult | null;
  selectedIndex: number | null;
  appsQueue: AppsQueueState;
  featuresQueue: FeaturesQueueState;
  updatesQueue: UpdatesQueueState;
  driversQueue: DriversQueueState;
  registryQueue: RegistryQueueState;
  filesQueue: FilesQueueState;
  onChangeUpdatesQueue: (next: UpdatesQueueState) => void;
  output: OutputOptions;
  onChangeOutput: (next: OutputOptions) => void;
  onNavigate: (section: SectionId) => void;
  isBuilding: boolean;
  buildProgress: BuildProgressEvent | null;
  buildResultMessage: { ok: boolean; text: string } | null;
  missingPresetPaths: MissingPresetPath[];
  presetBanner: { ok: boolean; text: string } | null;
  onDismissPresetBanner: () => void;
  onLoadPreset: () => void;
  onSavePreset: () => void;
  onRelinkMissingPath: (missing: MissingPresetPath) => void;
  onRemoveMissingPath: (missing: MissingPresetPath) => void;
  onCancelBuild: () => void;
}

const PIPELINE_STEPS = [1, 2, 3, 4, 5, 6, 7, 8, 9] as const;

export function BuildScreen({
  inspection,
  selectedIndex,
  appsQueue,
  featuresQueue,
  updatesQueue,
  driversQueue,
  registryQueue,
  filesQueue,
  onChangeUpdatesQueue,
  output,
  onChangeOutput,
  onNavigate,
  isBuilding,
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

  useEffect(() => {
    let active = true;
    if (!output.autounattendPath) {
      setXmlStatus(null);
      return;
    }
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
    return () => {
      active = false;
    };
  }, [output.autounattendPath]);

  const canBuildIso = inspection?.sourceKind === "iso";

  const handleBrowseOutput = async () => {
    const ext =
      output.target === "iso"
        ? "iso"
        : output.installFormat === "esd"
        ? "esd"
        : output.installFormat === "swm"
        ? "swm"
        : "wim";
    const defaultName =
      output.target === "iso" ? "Win11_Custom_x64.iso" : `install.${ext}`;
    const dest = await save({
      defaultPath: output.outputPath || defaultName,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    if (dest) {
      onChangeOutput({ ...output, outputPath: dest });
    }
  };

  const handleBrowseAutounattend = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "XML Answer File", extensions: ["xml"] }],
    });
    if (typeof selected === "string" && selected.trim()) {
      onChangeOutput({ ...output, autounattendPath: selected });
    }
  };

  const handleExportDiagnostics = async () => {
    const dest = await save({
      defaultPath: "isocraft-diagnostics.log",
      filters: [{ name: "Log files", extensions: ["log", "txt"] }],
    });
    if (dest) {
      await invoke("export_diagnostics", { destPath: dest });
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
  const hasBootWimDrivers = driversQueue.items.some(
    (d) => d.injectToBootWim && output.target === "iso"
  );
  const driversSummary =
    driversQueue.items.length > 0
      ? [
          t("screens.build.summaryDriversCount", {
            count: driversQueue.items.length,
            infs: totalDriverInfs,
          }),
          hasBootWimDrivers ? t("screens.build.summaryDriversBootBadge") : null,
        ]
          .filter(Boolean)
          .join(" ")
      : t("screens.build.noQueuedItems");

  const summaryRows: {
    id: SectionId;
    label: string;
    icon: typeof LayoutGrid;
    summary: string;
  }[] = [
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
  ];

  const handleTargetChange = (target: "iso" | "image") => {
    let nextPath = output.outputPath;
    if (nextPath) {
      const ext =
        target === "iso"
          ? "iso"
          : output.installFormat === "esd"
          ? "esd"
          : output.installFormat === "swm"
          ? "swm"
          : "wim";
      nextPath = nextPath.replace(/\.(iso|wim|esd|swm)$/i, `.${ext}`);
    }
    onChangeOutput({ ...output, target, outputPath: nextPath });
  };

  const handleFormatChange = (installFormat: InstallOutputFormat) => {
    let nextPath = output.outputPath;
    if (nextPath && output.target === "image") {
      nextPath = nextPath.replace(/\.(wim|esd|swm)$/i, `.${installFormat}`);
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
              disabled={!hasValidImage || isBuilding}
              onClick={onLoadPreset}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Upload className="w-3.5 h-3.5" strokeWidth={1.7} />
              <span>{t("screens.build.loadPreset")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={onSavePreset}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Download className="w-3.5 h-3.5" strokeWidth={1.7} />
              <span>{t("screens.build.savePreset")}</span>
            </button>
          </>
        }
      />

      {presetBanner && (
        <div
          className={`animate-fluent-item flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg text-[12.5px] ${
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
            <span className="break-all">{presetBanner.text}</span>
          </div>
          <button
            type="button"
            onClick={onDismissPresetBanner}
            className="p-1 rounded hover:bg-subtle shrink-0"
          >
            <X className="w-3.5 h-3.5" strokeWidth={1.7} />
          </button>
        </div>
      )}

      {!hasValidImage ? (
        <EmptyStateCard
          icon={PackageCheck}
          title={t("screens.build.emptyTitle")}
          description={t("screens.build.emptyDesc")}
        />
      ) : (
        <>
          {!isBuilding && missingPresetPaths.length > 0 && (
            <div className="animate-fluent-item bg-surface border border-border-subtle rounded-lg overflow-hidden">
              <div className="px-5 py-3 bg-status-warning-bg text-status-warning flex items-start gap-2.5">
                <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5" strokeWidth={1.8} />
                <div className="min-w-0">
                  <div className="text-[13px] font-semibold">
                    {t("screens.build.missingPathsTitle", {
                      count: missingPresetPaths.length,
                    })}
                  </div>
                  <div className="text-[12px] opacity-90">
                    {t("screens.build.missingPathsDesc")}
                  </div>
                </div>
              </div>
              <div className="divide-y divide-border-subtle">
                {missingPresetPaths.map((missing) => (
                  <div
                    key={missing.id}
                    className="px-5 py-2.5 flex flex-col sm:flex-row sm:items-center justify-between gap-3"
                  >
                    <div className="min-w-0 space-y-0.5">
                      <span className="inline-block px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-warning-bg text-status-warning">
                        {t(`screens.build.missingKinds.${missing.kind}`)}
                      </span>
                      <div className="text-[12px] font-mono text-text-primary break-all">
                        {missing.path}
                      </div>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      <button
                        type="button"
                        onClick={() => onRelinkMissingPath(missing)}
                        className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary"
                      >
                        <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                        <span>{t("screens.build.relinkBtn")}</span>
                      </button>
                      <button
                        type="button"
                        onClick={() => onRemoveMissingPath(missing)}
                        className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-colors"
                      >
                        <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                        <span>{t("screens.build.removeMissingBtn")}</span>
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Card 1: Pending changes summary */}
          <div className="bg-surface border border-border-subtle rounded-lg overflow-hidden">
            <div className="px-5 py-3 border-b border-border-subtle">
              <h2 className="text-[14px] font-semibold text-text-primary">
                {t("screens.build.summaryTitle")}
              </h2>
            </div>
            <div className="divide-y divide-border-subtle">
              {summaryRows.map((row) => {
                const Icon = row.icon;
                return (
                  <button
                    key={row.id}
                    type="button"
                    onClick={() => onNavigate(row.id)}
                    className="w-full px-5 py-2.5 flex items-center justify-between text-left hover:bg-subtle transition-colors"
                  >
                    <span className="flex items-center gap-2.5 text-[13px] font-medium text-text-primary">
                      <Icon className="w-4 h-4 text-text-secondary" strokeWidth={1.6} />
                      {row.label}
                    </span>
                    <span className="text-[12px] text-text-secondary">
                      {row.summary}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          {/* Card 2: Output Settings OR Active Build Progress */}
          {isBuilding ? (
            <div className="bg-surface border border-border-subtle rounded-lg p-5 space-y-4">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <Loader2 className="w-4 h-4 animate-spin text-accent" strokeWidth={1.8} />
                  <h2 className="text-[14px] font-semibold text-text-primary">
                    {t("screens.build.progressTitle")}
                  </h2>
                </div>
                {buildProgress && (
                  <span className="text-[12.5px] font-mono text-text-secondary">
                    {t("screens.build.stepCounter", {
                      step: buildProgress.step,
                      total: buildProgress.totalSteps,
                    })}{" "}
                    • {Math.round(buildProgress.percent)}%
                  </span>
                )}
              </div>

              <div className="w-full h-2 rounded-full bg-subtle overflow-hidden">
                <div
                  className="h-full bg-accent transition-all duration-300"
                  style={{ width: `${Math.min(100, buildProgress?.percent ?? 5)}%` }}
                />
              </div>

              {/* 9-Step Pipeline Checklist */}
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-2 pt-1">
                {PIPELINE_STEPS.map((stepNum) => {
                  const currentStep = buildProgress?.step ?? 1;
                  const isDone =
                    stepNum < currentStep || buildProgress?.phaseKey === "step_done";
                  const isActive =
                    stepNum === currentStep && buildProgress?.phaseKey !== "step_done";
                  return (
                    <div
                      key={stepNum}
                      className={`flex items-center gap-2 px-2.5 py-2 rounded-md border text-[12px] transition-colors ${
                        isActive
                          ? "bg-accent/10 border-accent text-text-primary font-medium"
                          : isDone
                          ? "bg-canvas border-border-subtle text-text-primary"
                          : "bg-canvas border-border-subtle text-text-secondary opacity-70"
                      }`}
                    >
                      {isDone ? (
                        <CheckCircle2
                          className="w-3.5 h-3.5 shrink-0 text-status-success"
                          strokeWidth={1.8}
                        />
                      ) : isActive ? (
                        <Loader2
                          className="w-3.5 h-3.5 shrink-0 animate-spin text-accent"
                          strokeWidth={1.8}
                        />
                      ) : (
                        <Circle
                          className="w-3.5 h-3.5 shrink-0 text-text-secondary"
                          strokeWidth={1.6}
                        />
                      )}
                      <span className="truncate">
                        {stepNum}. {t(`screens.build.steps.${stepNum}`)}
                      </span>
                    </div>
                  );
                })}
              </div>

              <div className="px-3 py-2 rounded-md bg-canvas border border-border-subtle text-[12px] font-mono text-text-secondary break-all">
                {buildProgress?.message ?? "..."}
              </div>

              <div className="flex items-center justify-end gap-2 pt-1">
                <button
                  type="button"
                  onClick={handleExportDiagnostics}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary"
                >
                  <Download className="w-3.5 h-3.5" strokeWidth={1.6} />
                  {t("screens.build.exportDiagnostics")}
                </button>
                <button
                  type="button"
                  onClick={onCancelBuild}
                  className="px-3.5 py-1.5 rounded-md text-[12.5px] font-semibold bg-status-danger text-white hover:opacity-90"
                >
                  {t("screens.build.cancelBuild")}
                </button>
              </div>
            </div>
          ) : (
            <div className="bg-surface border border-border-subtle rounded-lg p-5 space-y-4">
              <h2 className="text-[14px] font-semibold text-text-primary">
                {t("screens.build.outputTitle")}
              </h2>

              {buildResultMessage && (
                <div
                  className={`flex items-center justify-between gap-3 px-3.5 py-2.5 rounded-md text-[12.5px] ${
                    buildResultMessage.ok
                      ? "bg-status-success-bg text-status-success"
                      : "bg-status-danger-bg text-status-danger"
                  }`}
                >
                  <span className="break-all">{buildResultMessage.text}</span>
                  <button
                    type="button"
                    onClick={handleExportDiagnostics}
                    className="inline-flex items-center gap-1 px-2.5 py-1 rounded bg-surface/80 text-text-primary border border-border-subtle text-[11.5px] shrink-0"
                  >
                    <Download className="w-3 h-3" strokeWidth={1.6} />
                    {t("screens.build.exportDiagnostics")}
                  </button>
                </div>
              )}

              {/* Target container */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div className="space-y-1.5">
                  <label className="block text-[12.5px] font-medium text-text-secondary">
                    {t("screens.build.targetLabel")}
                  </label>
                  <div className="inline-flex rounded-md border border-border-subtle bg-canvas p-0.5 w-full">
                    <button
                      type="button"
                      disabled={!canBuildIso}
                      onClick={() => handleTargetChange("iso")}
                      className={`flex-1 py-1.5 px-3 rounded-[4px] text-[12.5px] font-medium transition-colors disabled:opacity-45 disabled:cursor-not-allowed ${
                        output.target === "iso"
                          ? "bg-surface text-text-primary shadow-sm"
                          : "text-text-secondary hover:text-text-primary"
                      }`}
                    >
                      {t("screens.build.targetIso")}
                    </button>
                    <button
                      type="button"
                      onClick={() => handleTargetChange("image")}
                      className={`flex-1 py-1.5 px-3 rounded-[4px] text-[12.5px] font-medium transition-colors ${
                        output.target === "image"
                          ? "bg-surface text-text-primary shadow-sm"
                          : "text-text-secondary hover:text-text-primary"
                      }`}
                    >
                      {t("screens.build.targetImage")}
                    </button>
                  </div>
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
                  <div className="inline-flex rounded-md border border-border-subtle bg-canvas p-0.5 w-full">
                    {(["wim", "esd", "swm"] as InstallOutputFormat[]).map((fmt) => (
                      <button
                        key={fmt}
                        type="button"
                        onClick={() => handleFormatChange(fmt)}
                        className={`flex-1 py-1.5 px-2.5 rounded-[4px] text-[12.5px] font-medium uppercase transition-colors ${
                          output.installFormat === fmt
                            ? "bg-surface text-text-primary shadow-sm"
                            : "text-text-secondary hover:text-text-primary"
                        }`}
                      >
                        {fmt}
                      </button>
                    ))}
                  </div>
                  {output.installFormat === "swm" && (
                    <div className="flex items-center gap-2 pt-1">
                      <label className="text-[12px] text-text-secondary">
                        {t("screens.build.splitSizeLabel")}:
                      </label>
                      <input
                        type="number"
                        min={100}
                        max={4095}
                        value={output.splitSizeMb}
                        onChange={(e) =>
                          onChangeOutput({
                            ...output,
                            splitSizeMb: Math.max(100, Number(e.target.value) || 3800),
                          })
                        }
                        className="w-24 px-2 py-1 rounded bg-canvas border border-border-subtle text-[12px] font-mono text-text-primary"
                      />
                    </div>
                  )}
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
                    className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary focus:outline-none focus:border-accent"
                  />
                  <button
                    type="button"
                    onClick={handleBrowseOutput}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary shrink-0"
                  >
                    <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                    {t("screens.build.browse")}
                  </button>
                </div>
              </div>

              {/* ISO-only options */}
              {output.target === "iso" && (
                <div className="pt-3 border-t border-border-subtle space-y-3">
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between gap-2">
                      <label className="text-[12.5px] font-medium text-text-secondary">
                        {t("screens.build.autounattendLabel")}
                      </label>
                      {xmlStatus && (
                        <span
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
                                error: xmlStatus.error ?? "",
                              })}
                            </>
                          )}
                        </span>
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
                        className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary focus:outline-none focus:border-accent"
                      />
                      <button
                        type="button"
                        onClick={handleBrowseAutounattend}
                        className="px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary shrink-0"
                      >
                        {t("screens.build.browse")}
                      </button>
                      {output.autounattendPath && (
                        <button
                          type="button"
                          onClick={() =>
                            onChangeOutput({ ...output, autounattendPath: null })
                          }
                          className="px-2.5 py-1.5 rounded-md text-[12px] text-text-secondary hover:text-text-primary hover:bg-subtle"
                        >
                          {t("settings.clear")}
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
                        className="w-full px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary"
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
                </div>
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
                  <div className="animate-fluent-item pl-6 space-y-2">
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
                      <div className="animate-fluent-item flex items-start gap-2 px-3 py-2 rounded-md bg-status-warning-bg text-status-warning text-[12px]">
                        <AlertTriangle
                          className="w-4 h-4 shrink-0 mt-0.5"
                          strokeWidth={1.75}
                        />
                        <span>{t("screens.build.cleanupResetBaseWarning")}</span>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
