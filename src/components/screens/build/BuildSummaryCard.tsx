import { useTranslation } from "react-i18next";
import type { LucideIcon } from "lucide-react";
import type { SectionId } from "../../../types/studio";

export interface BuildSummaryRow {
  id: SectionId;
  label: string;
  icon: LucideIcon;
  summary: string;
}

interface BuildSummaryCardProps {
  summaryRows: BuildSummaryRow[];
  onNavigate: (section: SectionId) => void;
}

export function BuildSummaryCard({
  summaryRows,
  onNavigate,
}: BuildSummaryCardProps) {
  const { t } = useTranslation();

  return (
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
  );
}
