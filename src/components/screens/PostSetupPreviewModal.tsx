import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, Check, Copy, FileCode2, Loader2, X } from "lucide-react";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { FluentSegmentedControl } from "../FluentMotion";
import { localizeBackendError } from "../../utils/backendErrors";
import type { PostSetupPreview, PostSetupQueueState } from "../../types/studio";

interface PostSetupPreviewModalProps {
  plan: PostSetupQueueState;
  autounattendPath: string | null;
  target: "iso" | "image";
  onClose: () => void;
}

type PreviewTabId =
  | "setupComplete"
  | "beforeLogon"
  | "firstLogonPs1"
  | "firstLogonCmd"
  | "runOnce"
  | "autounattend";

export function PostSetupPreviewModal({
  plan,
  autounattendPath,
  target,
  onClose,
}: PostSetupPreviewModalProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<PostSetupPreview | null>(null);
  const [activeTab, setActiveTab] = useState<PreviewTabId>("beforeLogon");
  const [copied, setCopied] = useState(false);

  const { isClosing, requestClose } = useAnimatedModalClose(onClose);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    invoke<PostSetupPreview>("preview_post_setup", {
      plan,
      autounattendPath,
      target,
    })
      .then((res) => {
        if (!active) return;
        setPreview(res);
        if (res.beforeLogonCmd) setActiveTab("beforeLogon");
        else if (res.firstLogonPs1) setActiveTab("firstLogonPs1");
        else if (res.runOnceRegEntry) setActiveTab("runOnce");
        else if (res.autounattendSnippet) setActiveTab("autounattend");
      })
      .catch((err) => {
        if (active) setError(String(err));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [autounattendPath, plan, target]);

  const tabs = useMemo(() => {
    if (!preview) return [];
    const list: { value: PreviewTabId; label: string; content: string }[] = [];
    if (preview.setupCompleteSnippet) {
      list.push({
        value: "setupComplete",
        label: "SetupComplete.cmd",
        content: preview.setupCompleteSnippet,
      });
    }
    if (preview.beforeLogonCmd) {
      list.push({
        value: "beforeLogon",
        label: "before_logon.cmd",
        content: preview.beforeLogonCmd,
      });
    }
    if (preview.firstLogonPs1) {
      list.push({
        value: "firstLogonPs1",
        label: "first_logon.ps1",
        content: preview.firstLogonPs1.replace(/^\uFEFF/, ""),
      });
    }
    if (preview.firstLogonCmd) {
      list.push({
        value: "firstLogonCmd",
        label: "first_logon.cmd",
        content: preview.firstLogonCmd,
      });
    }
    if (preview.runOnceRegEntry) {
      list.push({
        value: "runOnce",
        label: "RunOnce (NTUSER.DAT)",
        content: preview.runOnceRegEntry,
      });
    }
    if (preview.autounattendSnippet) {
      list.push({
        value: "autounattend",
        label: "autounattend.xml",
        content: preview.autounattendSnippet,
      });
    }
    return list;
  }, [preview]);

  const currentTab = tabs.find((tb) => tb.value === activeTab) ?? tabs[0] ?? null;

  const handleCopy = async () => {
    if (!currentTab) return;
    try {
      await navigator.clipboard.writeText(currentTab.content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      // ignore clipboard errors
    }
  };

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestClose}
      ariaLabel={t("screens.postSetup.previewModal.title")}
      maxWidthClass="max-w-[780px]"
      panelClassName="flex flex-col max-h-[86vh]"
    >
      <div className="px-5 py-4 border-b border-border-subtle flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="text-[15.5px] font-semibold text-text-primary flex items-center gap-2">
            <FileCode2 className="w-4 h-4 text-accent shrink-0" strokeWidth={1.75} />
            <span>{t("screens.postSetup.previewModal.title")}</span>
          </h2>
          <p className="text-[12px] text-text-secondary mt-0.5">
            {t("screens.postSetup.previewModal.subtitle")}
          </p>
        </div>
        <button
          type="button"
          onClick={requestClose}
          aria-label={t("modals.closeAria")}
          className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.95] shrink-0"
        >
          <X className="w-4 h-4" strokeWidth={1.75} />
        </button>
      </div>

      <div className="p-5 flex-1 overflow-y-auto space-y-3">
        {loading ? (
          <div className="py-12 flex flex-col items-center justify-center gap-2.5 text-[13px] text-text-secondary">
            <Loader2 className="w-5 h-5 animate-spin text-accent" strokeWidth={1.8} />
            <span>{t("screens.postSetup.previewModal.loading")}</span>
          </div>
        ) : error ? (
          <div className="px-4 py-3 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px] flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
            <span className="break-words">{localizeBackendError(error)}</span>
          </div>
        ) : tabs.length === 0 ? (
          <div className="py-10 text-center text-[13px] text-text-secondary">
            {t("screens.postSetup.previewModal.empty")}
          </div>
        ) : (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <FluentSegmentedControl<PreviewTabId>
                value={currentTab?.value ?? tabs[0].value}
                onChange={setActiveTab}
                variant="accent"
                size="sm"
                options={tabs.map((tb) => ({
                  value: tb.value,
                  label: tb.label,
                }))}
              />
              {currentTab && (
                <button
                  type="button"
                  onClick={() => void handleCopy()}
                  className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                >
                  {copied ? (
                    <Check
                      className="animate-fluent-icon-pop w-3.5 h-3.5 text-status-success"
                      strokeWidth={1.8}
                    />
                  ) : (
                    <Copy className="w-3.5 h-3.5" strokeWidth={1.6} />
                  )}
                  <span>
                    {copied
                      ? t("screens.postSetup.previewModal.copied")
                      : t("screens.postSetup.previewModal.copy")}
                  </span>
                </button>
              )}
            </div>

            {currentTab && (
              <pre className="p-4 rounded-lg bg-canvas border border-border-subtle text-[12px] font-mono text-text-primary overflow-x-auto max-h-[52vh] leading-relaxed whitespace-pre">
                {currentTab.content}
              </pre>
            )}
          </>
        )}
      </div>

      <div className="px-5 py-3 border-t border-border-subtle bg-canvas flex justify-end">
        <button
          type="button"
          onClick={requestClose}
          className="px-3.5 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
        >
          {t("screens.postSetup.previewModal.close")}
        </button>
      </div>
    </FluentModal>
  );
}
