import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";

interface CloseGuardModalProps {
  onKeepRunning: () => void;
  onForceStop: () => void;
}

export function CloseGuardModal({
  onKeepRunning,
  onForceStop,
}: CloseGuardModalProps) {
  const { t } = useTranslation();

  return (
    <div className="animate-fluent-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="animate-fluent-modal w-full max-w-[440px] bg-surface border border-border-subtle rounded-lg shadow-xl overflow-hidden">
        <div className="p-5 flex items-start gap-3.5">
          <div className="w-9 h-9 rounded-full bg-status-warning-bg text-status-warning flex items-center justify-center shrink-0">
            <AlertTriangle className="w-5 h-5" strokeWidth={1.75} />
          </div>
          <div>
            <h3 className="text-[15px] font-semibold text-text-primary">
              {t("closeGuard.title")}
            </h3>
            <p className="text-[13px] text-text-secondary mt-1.5 leading-relaxed">
              {t("closeGuard.description")}
            </p>
          </div>
        </div>
        <div className="px-5 py-3 border-t border-border-subtle bg-sidebar flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onKeepRunning}
            className="px-3.5 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-colors"
          >
            {t("closeGuard.keepRunning")}
          </button>
          <button
            type="button"
            onClick={onForceStop}
            className="px-3.5 py-1.5 rounded-md text-[13px] font-semibold bg-status-danger text-white hover:opacity-90 transition-opacity"
          >
            {t("closeGuard.forceStop")}
          </button>
        </div>
      </div>
    </div>
  );
}
