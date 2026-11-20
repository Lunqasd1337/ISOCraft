import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import { FluentModal, useAnimatedModalClose } from "./FluentModal";

interface CloseGuardModalProps {
  onKeepRunning: () => void;
  onForceStop: () => void;
}

export function CloseGuardModal({
  onKeepRunning,
  onForceStop,
}: CloseGuardModalProps) {
  const { t } = useTranslation();
  const { isClosing, requestClose: requestKeepRunning } =
    useAnimatedModalClose(onKeepRunning);

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestKeepRunning}
      maxWidthClass="max-w-[440px]"
      backdropClass="bg-black/50"
    >
      <div className="p-5 flex items-start gap-3.5">
          <div className="w-9 h-9 rounded-full bg-status-warning-bg text-status-warning flex items-center justify-center shrink-0">
            <AlertTriangle className="animate-fluent-icon-pop w-5 h-5" strokeWidth={1.75} />
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
            onClick={requestKeepRunning}
            className="px-3.5 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98]"
          >
            {t("closeGuard.keepRunning")}
          </button>
          <button
            type="button"
            onClick={onForceStop}
            className="px-3.5 py-1.5 rounded-md text-[13px] font-semibold bg-status-danger text-white hover:opacity-90 transition-all duration-150 active:scale-[0.98]"
          >
            {t("closeGuard.forceStop")}
          </button>
        </div>
      </div>
    </FluentModal>
  );
}
