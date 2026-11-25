import { useTranslation } from "react-i18next";
import { AlertTriangle, FolderOpen, Trash2 } from "lucide-react";
import { FluentReveal } from "../../FluentMotion";
import type { MissingPresetPath } from "../../../types/studio";

interface MissingPresetPathsCardProps {
  missingPresetPaths: MissingPresetPath[];
  onRelinkMissingPath: (item: MissingPresetPath) => void;
  onRemoveMissingPath: (item: MissingPresetPath) => void;
}

export function MissingPresetPathsCard({
  missingPresetPaths,
  onRelinkMissingPath,
  onRemoveMissingPath,
}: MissingPresetPathsCardProps) {
  const { t } = useTranslation();

  return (
    <FluentReveal className="bg-surface border border-border-subtle rounded-lg overflow-hidden">
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
                className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
              >
                <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                <span>{t("screens.build.relinkBtn")}</span>
              </button>
              <button
                type="button"
                onClick={() => onRemoveMissingPath(missing)}
                className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-all duration-150 active:scale-[0.98]"
              >
                <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                <span>{t("screens.build.removeMissingBtn")}</span>
              </button>
            </div>
          </div>
        ))}
      </div>
    </FluentReveal>
  );
}
