import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, Circle, Download, Loader2, X } from "lucide-react";
import { FluentReveal } from "../../FluentMotion";
import { localizeBackendError } from "../../../utils/backendErrors";
import type { BuildProgressEvent } from "../../../types/studio";

const PIPELINE_STEPS = [1, 2, 3, 4, 5, 6, 7, 8, 9] as const;

function formatElapsedTime(totalSeconds: number): string {
  const hrs = Math.floor(totalSeconds / 3600);
  const mins = Math.floor((totalSeconds % 3600) / 60);
  const secs = totalSeconds % 60;
  const mm = String(mins).padStart(2, "0");
  const ss = String(secs).padStart(2, "0");
  return hrs > 0 ? `${hrs}:${mm}:${ss}` : `${mm}:${ss}`;
}

export type DiagStatus =
  | { ok: true; path: string }
  | { ok: false; error: string }
  | null;

interface BuildProgressCardProps {
  buildProgress: BuildProgressEvent | null;
  buildStartedAt: number | null;
  cancellingBuild?: boolean;
  cancelError?: string | null;
  diagStatus: DiagStatus;
  onDismissDiag: () => void;
  onExportDiagnostics: () => void;
  onCancelBuild: () => void;
}

export function BuildProgressCard({
  buildProgress,
  buildStartedAt,
  cancellingBuild = false,
  cancelError = null,
  diagStatus,
  onDismissDiag,
  onExportDiagnostics,
  onCancelBuild,
}: BuildProgressCardProps) {
  const { t } = useTranslation();
  const [elapsedSec, setElapsedSec] = useState(() =>
    buildStartedAt
      ? Math.max(0, Math.floor((Date.now() - buildStartedAt) / 1000))
      : 0
  );

  useEffect(() => {
    const sync = () => {
      if (buildStartedAt) {
        setElapsedSec(Math.max(0, Math.floor((Date.now() - buildStartedAt) / 1000)));
      } else {
        setElapsedSec((prev) => prev + 1);
      }
    };
    sync();
    const timer = window.setInterval(sync, 1000);
    return () => window.clearInterval(timer);
  }, [buildStartedAt]);

  const detailMessage =
    buildProgress?.message &&
    buildProgress.message.trim().length > 0 &&
    buildProgress.message.trim() !== "..."
      ? buildProgress.message
      : null;

  return (
    <FluentReveal className="bg-surface border border-border-subtle rounded-lg p-5 space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2.5 min-w-0">
          <Loader2
            className="w-4 h-4 animate-spin text-accent shrink-0"
            strokeWidth={1.8}
          />
          <h2 className="text-[14px] font-semibold text-text-primary truncate">
            {t("screens.build.progressTitle")}
          </h2>
        </div>
        <div className="flex items-center gap-3 shrink-0 text-[12.5px] font-mono text-text-secondary">
          <span>
            {t("screens.build.elapsedLabel", {
              time: formatElapsedTime(elapsedSec),
            })}
          </span>
          {buildProgress && (
            <span>
              {t("screens.build.stepCounter", {
                step: buildProgress.step,
                total: buildProgress.totalSteps,
              })}{" "}
              • {Math.round(buildProgress.percent)}%
            </span>
          )}
        </div>
      </div>

      <div className="w-full h-2 rounded-full bg-subtle overflow-hidden">
        <div
          className="h-full bg-accent transition-[width] duration-300 ease-out"
          style={{ width: `${Math.min(100, buildProgress?.percent ?? 5)}%` }}
        />
      </div>

      {detailMessage && (
        <div
          className="text-[12px] text-text-secondary font-mono truncate"
          title={detailMessage}
        >
          {detailMessage}
        </div>
      )}

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
                  className="animate-fluent-icon-pop w-3.5 h-3.5 shrink-0 text-status-success"
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

      {diagStatus && (
        <FluentReveal
          className={`flex items-center justify-between gap-2 px-3 py-2 rounded-md text-[12px] ${
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
            onClick={onDismissDiag}
            aria-label={t("logDrawer.close")}
            className="p-0.5 rounded hover:bg-subtle transition-colors shrink-0"
          >
            <X className="w-3.5 h-3.5" strokeWidth={1.7} />
          </button>
        </FluentReveal>
      )}
      {cancelError && (
        <div className="px-3 py-2 rounded-md text-[12px] bg-status-danger-bg text-status-danger break-words">
          {localizeBackendError(cancelError)}
        </div>
      )}
      <div className="flex items-center justify-end gap-2 pt-1">
        <button
          type="button"
          onClick={onExportDiagnostics}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
        >
          <Download className="w-3.5 h-3.5" strokeWidth={1.6} />
          {t("screens.build.exportDiagnostics")}
        </button>
        <button
          type="button"
          onClick={onCancelBuild}
          disabled={cancellingBuild}
          className="px-3.5 py-1.5 rounded-md text-[12.5px] font-semibold bg-status-danger text-white hover:opacity-90 transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
        >
          {cancellingBuild
            ? t("screens.build.cancellingBuild")
            : t("screens.build.cancelBuild")}
        </button>
      </div>
    </FluentReveal>
  );
}
