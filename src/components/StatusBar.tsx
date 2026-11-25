import { useTranslation } from "react-i18next";
import { ArrowRight, Disc, Loader2, Terminal } from "lucide-react";

interface StatusBarProps {
  imageLabel: string | null;
  editionLabel: string | null;
  queuedCount: number;
  freeSpaceGb: number | null;
  lowSpaceWarning: boolean;
  isLogOpen: boolean;
  onToggleLog: () => void;
  onPrimaryBuildClick: () => void;
  buildDisabled?: boolean;
  isBuilding?: boolean;
  buildPercent?: number;
}

export function StatusBar({
  imageLabel,
  editionLabel,
  queuedCount,
  freeSpaceGb,
  lowSpaceWarning,
  isLogOpen,
  onToggleLog,
  onPrimaryBuildClick,
  buildDisabled = false,
  isBuilding = false,
  buildPercent = 0,
}: StatusBarProps) {
  const { t } = useTranslation();

  const fullLabel = imageLabel
    ? editionLabel
      ? `${imageLabel} • ${editionLabel}`
      : imageLabel
    : t("statusBar.noImage");

  return (
    <footer className="h-[48px] shrink-0 bg-sidebar border-t border-border-subtle px-4 flex items-center justify-between gap-4 select-none">
      {/* Left: Image & Queue status */}
      <div className="flex items-center gap-3 min-w-0 text-[12.5px] text-text-secondary">
        <span className="flex items-center gap-2 min-w-0 truncate">
          <Disc className="w-3.5 h-3.5 shrink-0 text-text-secondary" strokeWidth={1.6} />
          <span className="truncate font-medium text-text-primary">{fullLabel}</span>
        </span>
        <span className="text-border-subtle">•</span>
        <span className="shrink-0">
          {t("statusBar.queuedChanges", { count: queuedCount })}
        </span>
        {freeSpaceGb !== null && freeSpaceGb > 0 && (
          <>
            <span className="text-border-subtle hidden sm:inline">•</span>
            <span
              className={`shrink-0 hidden sm:inline ${
                lowSpaceWarning ? "text-status-warning font-medium" : ""
              }`}
            >
              {t("statusBar.freeSpace", { gb: freeSpaceGb.toFixed(1) })}
            </span>
          </>
        )}
      </div>

      {/* Right: Log Drawer toggle + Build button */}
      <div className="flex items-center gap-2 shrink-0">
        <button
          type="button"
          onClick={onToggleLog}
          className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium border transition-all duration-150 active:scale-[0.98] ${
            isLogOpen
              ? "bg-surface border-accent text-accent"
              : "bg-surface border-border-subtle text-text-primary hover:bg-subtle"
          }`}
        >
          <Terminal className="w-3.5 h-3.5" strokeWidth={1.6} />
          <span>{t("statusBar.logs")}</span>
        </button>

        <button
          type="button"
          onClick={onPrimaryBuildClick}
          disabled={buildDisabled || isBuilding}
          className="inline-flex items-center gap-1.5 px-4 py-1.5 rounded-md text-[12.5px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
        >
          {isBuilding ? (
            <>
              <Loader2 className="w-3.5 h-3.5 animate-spin" strokeWidth={1.8} />
              <span>
                {t("statusBar.building", { percent: Math.round(buildPercent) })}
              </span>
            </>
          ) : (
            <>
              <span>{t("statusBar.buildImage")}</span>
              <ArrowRight className="w-3.5 h-3.5" strokeWidth={1.8} />
            </>
          )}
        </button>
      </div>
    </footer>
  );
}
