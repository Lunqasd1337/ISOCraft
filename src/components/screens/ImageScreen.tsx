import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  Disc,
  ExternalLink,
  FolderOpen,
  Loader2,
  RefreshCw,
  ShieldAlert,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { FluentReveal } from "../FluentMotion";
import { localizeBackendError } from "../../utils/backendErrors";
import type {
  EnvironmentStatus,
  ImageCatalog,
  ImageInspectionResult,
} from "../../types/studio";

interface ImageScreenProps {
  selectedImagePath: string | null;
  inspection: ImageInspectionResult | null;
  inspecting: boolean;
  inspectError: string | null;
  selectedIndex: number | null;
  catalog: ImageCatalog | null;
  catalogLoading: boolean;
  catalogError: string | null;
  envStatus: EnvironmentStatus | null;
  onSelectImagePath: (path: string) => void;
  onSelectEditionIndex: (index: number) => void;
  onReloadCatalog: () => void;
  isBuilding?: boolean;
}

const IMAGE_REASON_KEYS = {
  boot_wim: "screens.image.reasons.boot_wim",
  not_x64: "screens.image.reasons.not_x64",
  not_client: "screens.image.reasons.not_client",
  not_win11: "screens.image.reasons.not_win11",
  missing_install_image: "screens.image.reasons.missing_install_image",
  unsupported_image: "screens.image.reasons.unsupported_image",
} as const;

export function ImageScreen({
  selectedImagePath,
  inspection,
  inspecting,
  inspectError,
  selectedIndex,
  catalog,
  catalogLoading,
  catalogError,
  envStatus,
  onSelectImagePath,
  onSelectEditionIndex,
  onReloadCatalog,
  isBuilding = false,
}: ImageScreenProps) {
  const { t } = useTranslation();

  const handleBrowseImage = async () => {
    if (inspecting || isBuilding) return;
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: t("dialogs.win11Image"),
            extensions: ["iso", "wim", "esd"],
          },
        ],
      });
      if (typeof selected === "string" && selected.trim()) {
        onSelectImagePath(selected);
      }
    } catch {
      // ignore dialog cancellation errors
    }
  };

  const formatGb = (bytes: number) => (bytes / (1024 * 1024 * 1024)).toFixed(2);

  const translateRejection = (reason: string | null) => {
    if (!reason) return t("screens.image.reasons.unsupported_image");
    if (reason in IMAGE_REASON_KEYS) {
      return t(IMAGE_REASON_KEYS[reason as keyof typeof IMAGE_REASON_KEYS]);
    }
    return reason;
  };

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.image.title")}
        subtitle={t("screens.image.subtitle")}
        actions={
          <button
            type="button"
            onClick={handleBrowseImage}
            disabled={inspecting || isBuilding}
            className="inline-flex items-center gap-2 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-60 disabled:active:scale-100 disabled:cursor-not-allowed"
          >
            {inspecting ? (
              <Loader2 className="w-4 h-4 animate-spin" strokeWidth={1.7} />
            ) : (
              <FolderOpen className="w-4 h-4" strokeWidth={1.7} />
            )}
            <span>{t("screens.image.selectBtn")}</span>
          </button>
        }
      />

      {envStatus?.lowSpaceWarning && (
        <FluentReveal className="flex items-center gap-2.5 px-4 py-2.5 rounded-lg bg-status-warning-bg text-status-warning text-[12.5px] font-medium">
          <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
          <span>
            {t("screens.image.lowSpaceBanner", {
              gb: envStatus.freeSpaceGb.toFixed(1),
            })}
          </span>
        </FluentReveal>
      )}

      {!selectedImagePath && !inspecting && (
        <EmptyStateCard
          icon={Disc}
          title={t("screens.image.emptyTitle")}
          description={t("screens.image.emptyDesc")}
          action={
            <div className="flex flex-col items-center gap-4">
              <button
                type="button"
                onClick={handleBrowseImage}
                disabled={inspecting || isBuilding}
                className="inline-flex items-center gap-2 px-4 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-60 disabled:cursor-not-allowed"
              >
                <FolderOpen className="w-4 h-4" strokeWidth={1.7} />
                <span>{t("screens.image.selectBtn")}</span>
              </button>

              <div className="w-full max-w-[460px] pt-3 border-t border-border-subtle flex flex-col items-center gap-2.5">
                <span className="text-[12px] text-text-secondary">
                  {t("screens.image.noImageHint")}
                </span>
                <div className="flex flex-wrap items-center justify-center gap-2">
                  <button
                    type="button"
                    onClick={() =>
                      invoke("open_external_url", {
                        url: "https://www.microsoft.com/software-download/windows11",
                      }).catch(() => {})
                    }
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                  >
                    <span>{t("screens.image.downloadMicrosoft")}</span>
                    <ExternalLink className="w-3.5 h-3.5 text-text-secondary" strokeWidth={1.6} />
                  </button>
                  <button
                    type="button"
                    onClick={() =>
                      invoke("open_external_url", {
                        url: "https://uupdump.net/",
                      }).catch(() => {})
                    }
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                  >
                    <span>{t("screens.image.buildUupDump")}</span>
                    <ExternalLink className="w-3.5 h-3.5 text-text-secondary" strokeWidth={1.6} />
                  </button>
                </div>
                <p className="text-[11.5px] text-text-secondary leading-snug">
                  {t("screens.image.uupDumpNote")}
                </p>
              </div>
            </div>
          }
        />
      )}

      {inspecting && (
        <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-10 flex flex-col items-center gap-3 text-center">
          <Loader2 className="w-6 h-6 animate-spin text-accent" strokeWidth={1.7} />
          <div className="text-[13.5px] font-medium text-text-primary">
            {t("screens.image.inspecting")}
          </div>
          {selectedImagePath && (
            <div className="text-[12px] font-mono text-text-secondary break-all max-w-[600px]">
              {selectedImagePath}
            </div>
          )}
        </FluentReveal>
      )}

      {!inspecting && inspectError && (
        <FluentReveal className="bg-surface border border-status-danger/40 rounded-lg p-5 flex items-start gap-3">
          <ShieldAlert className="w-5 h-5 text-status-danger shrink-0 mt-0.5" strokeWidth={1.75} />
          <div className="space-y-1 min-w-0">
            <div className="text-[14px] font-semibold text-status-danger">
              {t("screens.image.blockedTitle")}
            </div>
            <div className="text-[12.5px] text-text-primary break-words">
              {localizeBackendError(inspectError)}
            </div>
          </div>
        </FluentReveal>
      )}

      {!inspecting && inspection && (
        <>
          {/* Source metadata card */}
          <FluentReveal className="bg-surface border border-border-subtle rounded-lg p-5 space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="text-[12px] font-semibold uppercase tracking-wide text-text-secondary">
                {t("screens.image.selectedPath")}
              </span>
              <div className="flex flex-wrap items-center gap-2 text-[11.5px] font-medium">
                <span className="px-2 py-0.5 rounded bg-subtle text-text-primary uppercase">
                  {inspection.sourceKind}
                </span>
                <span className="px-2 py-0.5 rounded bg-subtle text-text-secondary uppercase">
                  {t("screens.image.installFormat")}: {inspection.installFileFormat}
                </span>
                {inspection.sourceKind === "iso" && (
                  <span
                    className={`px-2 py-0.5 rounded ${
                      inspection.hasBootWim
                        ? "bg-status-success-bg text-status-success"
                        : "bg-subtle text-text-secondary"
                    }`}
                  >
                    {inspection.hasBootWim
                      ? t("screens.image.bootWimPresent")
                      : t("screens.image.bootWimMissing")}
                  </span>
                )}
                {inspection.hasSxs && (
                  <span className="px-2 py-0.5 rounded bg-status-success-bg text-status-success">
                    {t("screens.image.sxsPresent")}
                  </span>
                )}
              </div>
            </div>
            <div className="px-3 py-2 rounded-md bg-subtle font-mono text-[12.5px] text-text-primary break-all select-text">
              {inspection.sourcePath}
            </div>
          </FluentReveal>

          {/* Blocking card if not valid Win11 Client x64 */}
          {!inspection.isValidWin11ClientX64 && (
            <FluentReveal className="bg-status-danger-bg border border-status-danger/30 rounded-lg p-5 flex items-start gap-3">
              <ShieldAlert className="w-5 h-5 text-status-danger shrink-0 mt-0.5" strokeWidth={1.75} />
              <div className="space-y-1">
                <div className="text-[14px] font-semibold text-status-danger">
                  {t("screens.image.blockedTitle")}
                </div>
                <p className="text-[13px] text-text-primary leading-relaxed">
                  {translateRejection(inspection.blockingReason)}
                </p>
              </div>
            </FluentReveal>
          )}

          {/* Editions radio list */}
          {inspection.editions.length > 0 && (
            <FluentReveal className="bg-surface border border-border-subtle rounded-lg overflow-hidden">
              <div className="px-5 py-3.5 border-b border-border-subtle flex flex-wrap items-center justify-between gap-2">
                <div>
                  <h2 className="text-[14px] font-semibold text-text-primary">
                    {t("screens.image.editionsTitle")}
                  </h2>
                  <p className="text-[12px] text-text-secondary mt-0.5">
                    {t("screens.image.editionsHint")}
                  </p>
                </div>
              </div>

              <div className="divide-y divide-border-subtle">
                {inspection.editions.map((ed) => {
                  const selected = selectedIndex === ed.index;
                  const disabled = !ed.isSupported || isBuilding;
                  return (
                    <button
                      key={ed.index}
                      type="button"
                      disabled={disabled}
                      onClick={() => onSelectEditionIndex(ed.index)}
                      className={`w-full text-left px-5 py-3 flex items-center justify-between gap-4 transition-colors ${
                        disabled
                          ? "opacity-55 cursor-not-allowed bg-subtle"
                          : selected
                          ? "bg-subtle"
                          : "hover:bg-subtle"
                      }`}
                    >
                      <div className="flex items-center gap-3 min-w-0">
                        <span
                          className={`w-4 h-4 rounded-full border flex items-center justify-center shrink-0 transition-all duration-150 ${
                            selected
                              ? "border-accent bg-accent"
                              : "border-text-secondary/50 bg-surface"
                          }`}
                        >
                          {selected && (
                            <span className="animate-fluent-icon-pop w-1.5 h-1.5 rounded-full bg-accent-fg" />
                          )}
                        </span>
                        <div className="min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-[13.5px] font-semibold text-text-primary truncate">
                              {ed.name ||
                                ed.editionId ||
                                t("screens.image.editionFallback", { index: ed.index })}
                            </span>
                            <span className="px-1.5 py-0.5 rounded text-[11px] font-mono bg-subtle text-text-secondary shrink-0">
                              {t("screens.image.indexLabel", { index: ed.index })}
                            </span>
                          </div>
                          <div className="text-[12px] text-text-secondary flex flex-wrap items-center gap-2 mt-0.5">
                            {ed.version && <span className="font-mono">{ed.version}</span>}
                            {ed.languages.length > 0 && (
                              <span>• {ed.languages.join(", ")}</span>
                            )}
                            {ed.rejectionReason && (
                              <span className="text-status-danger">
                                • {translateRejection(ed.rejectionReason)}
                              </span>
                            )}
                          </div>
                        </div>
                      </div>

                      <div className="flex items-center gap-2 shrink-0">
                        {ed.totalBytes > 0 && (
                          <span className="text-[12px] font-mono text-text-secondary">
                            {t("screens.image.sizeGb", { gb: formatGb(ed.totalBytes) })}
                          </span>
                        )}
                        <span
                          className={`px-2 py-0.5 rounded text-[11px] font-semibold uppercase ${
                            ed.architecture === "x64"
                              ? "bg-status-success-bg text-status-success"
                              : "bg-status-danger-bg text-status-danger"
                          }`}
                        >
                          {ed.architecture || "?"}
                        </span>
                        {selected && (
                          <CheckCircle2 className="animate-fluent-icon-pop w-4 h-4 text-accent" strokeWidth={1.8} />
                        )}
                      </div>
                    </button>
                  );
                })}
              </div>
            </FluentReveal>
          )}

          {/* Catalog readiness status */}
          {selectedIndex !== null && (
            <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-5 py-3 flex flex-wrap items-center justify-between gap-3">
              {catalogLoading ? (
                <div className="flex items-center gap-2.5 text-[12.5px] text-text-primary">
                  <Loader2 className="w-4 h-4 animate-spin text-accent" strokeWidth={1.7} />
                  <span>{t("screens.image.catalogLoading")}</span>
                </div>
              ) : catalogError ? (
                <div className="flex items-center gap-2 text-[12.5px] text-status-danger">
                  <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                  <span>
                    {t("screens.image.catalogError", {
                      error: localizeBackendError(catalogError),
                    })}
                  </span>
                </div>
              ) : catalog ? (
                <div className="flex items-center gap-2 text-[12.5px] text-text-primary">
                  <CheckCircle2 className="animate-fluent-icon-pop w-4 h-4 text-status-success shrink-0" strokeWidth={1.75} />
                  <span>
                    {t("screens.image.catalogReady", {
                      appx: catalog.appxPackages.length,
                      features: catalog.features.length,
                      caps: catalog.capabilities.length,
                    })}{" "}
                    {catalog.fromCache && (
                      <span className="text-text-secondary">
                        {t("screens.image.catalogCached")}
                      </span>
                    )}
                  </span>
                </div>
              ) : null}

              {!catalogLoading && (
                <button
                  type="button"
                  disabled={isBuilding}
                  onClick={onReloadCatalog}
                  className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[12px] font-medium bg-subtle hover:bg-border-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <RefreshCw className="w-3.5 h-3.5" strokeWidth={1.6} />
                  {t("screens.image.reloadCatalog")}
                </button>
              )}
            </FluentReveal>
          )}
        </>
      )}
    </div>
  );
}
