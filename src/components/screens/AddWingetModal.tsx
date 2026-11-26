import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Download, Plus, Search, X } from "lucide-react";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { FluentSegmentedControl } from "../FluentMotion";
import type {
  PostSetupItem,
  WingetScope,
  WingetSource,
} from "../../types/studio";

interface AddWingetModalProps {
  existingIds: string[];
  onAddItems: (items: PostSetupItem[]) => void;
  onClose: () => void;
}

const POPULAR_WINGET_APPS = [
  { id: "7zip.7zip", name: "7-Zip" },
  { id: "Mozilla.Firefox", name: "Mozilla Firefox" },
  { id: "Google.Chrome", name: "Google Chrome" },
  { id: "VideoLAN.VLC", name: "VLC Media Player" },
  { id: "Notepad++.Notepad++", name: "Notepad++" },
  { id: "Microsoft.VisualStudioCode", name: "Visual Studio Code" },
  { id: "Git.Git", name: "Git for Windows" },
  { id: "Microsoft.PowerToys", name: "Microsoft PowerToys" },
  { id: "OBSProject.OBSStudio", name: "OBS Studio" },
  { id: "Telegram.TelegramDesktop", name: "Telegram Desktop" },
] as const;

const WINGET_ID_RE = /^[A-Za-z0-9._+-]{1,128}$/;

function isValidWingetIdClient(raw: string): boolean {
  const trimmed = raw.trim();
  return Boolean(trimmed) && !trimmed.startsWith("-") && WINGET_ID_RE.test(trimmed);
}

function makeItemId(seed: string): string {
  return typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${seed}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function AddWingetModal({
  existingIds,
  onAddItems,
  onClose,
}: AddWingetModalProps) {
  const { t } = useTranslation();
  const [wingetId, setWingetId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [wingetSource, setWingetSource] = useState<WingetSource>("winget");
  const [wingetScope, setWingetScope] = useState<WingetScope>("default");
  const [extraArgs, setExtraArgs] = useState("");
  const [presetSearch, setPresetSearch] = useState("");
  const [selectedPresets, setSelectedPresets] = useState<Set<string>>(() => new Set());

  const { isClosing, requestClose } = useAnimatedModalClose(onClose);

  const existingSet = useMemo(
    () => new Set(existingIds.map((id) => id.toLowerCase())),
    [existingIds]
  );

  const filteredPopular = useMemo(() => {
    const q = presetSearch.trim().toLowerCase();
    if (!q) return POPULAR_WINGET_APPS;
    return POPULAR_WINGET_APPS.filter(
      (app) => app.id.toLowerCase().includes(q) || app.name.toLowerCase().includes(q)
    );
  }, [presetSearch]);

  const togglePresetApp = (id: string) => {
    setSelectedPresets((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const manualTrimmed = wingetId.trim();
  const manualValid = manualTrimmed.length === 0 || isValidWingetIdClient(manualTrimmed);
  const canAddManual = manualTrimmed.length > 0 && manualValid;
  const totalToAdd = (canAddManual ? 1 : 0) + selectedPresets.size;

  const previewCommand = useMemo(() => {
    const sampleId = manualTrimmed || Array.from(selectedPresets)[0] || "Vendor.App";
    const parts = [
      "winget install --id",
      sampleId,
      "--exact --silent --accept-package-agreements --accept-source-agreements",
    ];
    if (wingetSource === "msstore") parts.push("--source msstore");
    if (wingetScope !== "default") parts.push(`--scope ${wingetScope}`);
    if (extraArgs.trim()) parts.push(extraArgs.trim());
    return parts.join(" ");
  }, [extraArgs, manualTrimmed, selectedPresets, wingetScope, wingetSource]);

  const handleSubmit = () => {
    if (totalToAdd === 0 || !manualValid) return;
    const toAdd: PostSetupItem[] = [];
    const seen = new Set<string>();

    if (canAddManual) {
      seen.add(manualTrimmed.toLowerCase());
      toAdd.push({
        id: makeItemId(manualTrimmed),
        kind: "winget",
        phase: "firstLogon",
        enabled: true,
        sourcePath: null,
        args: extraArgs.trim(),
        wingetId: manualTrimmed,
        displayName: displayName.trim() || manualTrimmed,
        wingetSource,
        wingetScope,
      });
    }

    for (const app of POPULAR_WINGET_APPS) {
      if (!selectedPresets.has(app.id)) continue;
      if (seen.has(app.id.toLowerCase())) continue;
      seen.add(app.id.toLowerCase());
      toAdd.push({
        id: makeItemId(app.id),
        kind: "winget",
        phase: "firstLogon",
        enabled: true,
        sourcePath: null,
        args: extraArgs.trim(),
        wingetId: app.id,
        displayName: app.name,
        wingetSource: "winget",
        wingetScope,
      });
    }

    if (toAdd.length > 0) {
      onAddItems(toAdd);
    }
    requestClose();
  };

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestClose}
      ariaLabel={t("screens.postSetup.wingetModal.title")}
      maxWidthClass="max-w-[660px]"
      panelClassName="flex flex-col max-h-[86vh]"
    >
      <div className="px-5 py-4 border-b border-border-subtle flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="text-[15.5px] font-semibold text-text-primary flex items-center gap-2">
            <Download className="w-4 h-4 text-accent shrink-0" strokeWidth={1.75} />
            <span>{t("screens.postSetup.wingetModal.title")}</span>
          </h2>
          <p className="text-[12px] text-text-secondary mt-0.5">
            {t("screens.postSetup.wingetModal.subtitle")}
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

      <div className="p-5 space-y-4 overflow-y-auto">
        {/* Manual ID & Label */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div className="space-y-1.5">
            <label className="block text-[12px] font-semibold text-text-primary">
              {t("screens.postSetup.wingetModal.idLabel")}
            </label>
            <input
              type="text"
              value={wingetId}
              onChange={(e) => setWingetId(e.target.value)}
              placeholder={t("screens.postSetup.wingetModal.idPlaceholder")}
              className={`w-full px-3 py-1.5 rounded-md bg-canvas border text-[12.5px] font-mono text-text-primary focus:outline-none transition-colors duration-150 ${
                manualValid
                  ? "border-border-subtle focus:border-accent"
                  : "border-status-danger"
              }`}
            />
            {!manualValid && (
              <div className="text-[11.5px] text-status-danger">
                {t("screens.postSetup.wingetModal.invalidIdHint")}
              </div>
            )}
          </div>
          <div className="space-y-1.5">
            <label className="block text-[12px] font-semibold text-text-primary">
              {t("screens.postSetup.wingetModal.nameLabel")}
            </label>
            <input
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder={t("screens.postSetup.wingetModal.namePlaceholder")}
              className="w-full px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
            />
          </div>
        </div>

        {/* Source, Scope, Extra args */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div className="space-y-1.5">
            <label className="block text-[12px] font-semibold text-text-primary">
              {t("screens.postSetup.wingetModal.sourceLabel")}
            </label>
            <FluentSegmentedControl<WingetSource>
              value={wingetSource}
              onChange={setWingetSource}
              variant="surface"
              size="sm"
              fullWidth
              options={[
                { value: "winget", label: "winget" },
                { value: "msstore", label: "msstore" },
              ]}
            />
          </div>
          <div className="space-y-1.5">
            <label className="block text-[12px] font-semibold text-text-primary">
              {t("screens.postSetup.wingetModal.scopeLabel")}
            </label>
            <FluentSegmentedControl<WingetScope>
              value={wingetScope}
              onChange={setWingetScope}
              variant="surface"
              size="sm"
              fullWidth
              options={[
                { value: "default", label: t("screens.postSetup.scopeDefault") },
                { value: "machine", label: t("screens.postSetup.scopeMachine") },
                { value: "user", label: t("screens.postSetup.scopeUser") },
              ]}
            />
          </div>
        </div>

        <div className="space-y-1.5">
          <label className="block text-[12px] font-semibold text-text-primary">
            {t("screens.postSetup.wingetModal.extraArgsLabel")}
          </label>
          <input
            type="text"
            value={extraArgs}
            onChange={(e) => setExtraArgs(e.target.value)}
            placeholder={t("screens.postSetup.wingetModal.extraArgsPlaceholder")}
            className="w-full px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
          />
        </div>

        {/* Popular quick-pick catalog */}
        <div className="space-y-2 pt-1">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <label className="text-[12px] font-semibold text-text-primary">
              {t("screens.postSetup.wingetModal.popularTitle")}
            </label>
            <div className="relative w-52">
              <Search
                className="w-3.5 h-3.5 text-text-secondary absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none"
                strokeWidth={1.6}
              />
              <input
                type="text"
                value={presetSearch}
                onChange={(e) => setPresetSearch(e.target.value)}
                placeholder={t("screens.postSetup.wingetModal.popularSearch")}
                className="w-full pl-8 pr-2.5 py-1 rounded-md bg-canvas border border-border-subtle text-[12px] text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
              />
            </div>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5 max-h-44 overflow-y-auto pr-1">
            {filteredPopular.map((app) => {
              const checked = selectedPresets.has(app.id);
              const alreadyQueued = existingSet.has(app.id.toLowerCase());
              return (
                <button
                  key={app.id}
                  type="button"
                  onClick={() => togglePresetApp(app.id)}
                  className={`flex items-center justify-between gap-2 px-3 py-2 rounded-md border text-left transition-all duration-150 active:scale-[0.99] ${
                    checked
                      ? "bg-accent/10 border-accent text-text-primary"
                      : "bg-canvas border-border-subtle hover:bg-subtle text-text-primary"
                  }`}
                >
                  <div className="min-w-0">
                    <div className="text-[12.5px] font-medium truncate">
                      {app.name}
                    </div>
                    <div className="text-[11px] font-mono text-text-secondary truncate">
                      {app.id}
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 shrink-0">
                    {alreadyQueued && (
                      <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-subtle text-text-secondary">
                        {t("screens.postSetup.wingetModal.inQueue")}
                      </span>
                    )}
                    {checked && (
                      <Check
                        className="animate-fluent-icon-pop w-4 h-4 text-accent"
                        strokeWidth={2}
                      />
                    )}
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Live command preview */}
        <div className="px-3 py-2 rounded-md bg-canvas border border-border-subtle space-y-1">
          <div className="text-[11px] font-semibold text-text-secondary uppercase tracking-wide">
            {t("screens.postSetup.wingetModal.cmdPreviewLabel")}
          </div>
          <div className="text-[11.5px] font-mono text-text-primary break-all">
            {previewCommand}
          </div>
        </div>
      </div>

      <div className="px-5 py-3.5 border-t border-border-subtle bg-canvas flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={requestClose}
          className="px-3.5 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
        >
          {t("screens.postSetup.wingetModal.cancel")}
        </button>
        <button
          type="button"
          disabled={totalToAdd === 0 || !manualValid}
          onClick={handleSubmit}
          className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-md text-[12.5px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Plus className="w-3.5 h-3.5" strokeWidth={1.7} />
          <span>
            {t("screens.postSetup.wingetModal.addBtn", { count: totalToAdd })}
          </span>
        </button>
      </div>
    </FluentModal>
  );
}
