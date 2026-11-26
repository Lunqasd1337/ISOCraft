import {
  Suspense,
  lazy,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  Download,
  Eye,
  Info,
  Package,
  Plus,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { FluentReveal, FluentSegmentedControl } from "../FluentMotion";
import { FluentSwitch } from "../FluentSwitch";
import { prefersReducedMotion } from "../../utils/motion";
import { localizeBackendError } from "../../utils/backendErrors";
import type {
  FirstLogonMode,
  PostSetupFileScanResult,
  PostSetupItem,
  PostSetupPhase,
  PostSetupQueueState,
  WingetScope,
  WingetSource,
} from "../../types/studio";

const AddWingetModal = lazy(() =>
  import("./AddWingetModal").then((m) => ({ default: m.AddWingetModal }))
);
const PostSetupPreviewModal = lazy(() =>
  import("./PostSetupPreviewModal").then((m) => ({
    default: m.PostSetupPreviewModal,
  }))
);

type PhaseFilter = "all" | "beforeLogon" | "firstLogon";

const MSI_ARG_PRESETS = ["/qn /norestart", "/qb! /norestart"] as const;
const EXE_ARG_PRESETS = ["/S", "/VERYSILENT /NORESTART", "/quiet", "--silent"] as const;

function makeItemId(seed: string): string {
  return typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${seed}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function getBaseName(raw: string | null): string {
  if (!raw) return "";
  return raw.split(/[\\/]/).pop() || raw;
}

function formatWingetPreview(item: PostSetupItem): string {
  const id = (item.wingetId ?? "").trim();
  const parts = [
    "winget install --id",
    id,
    "--exact --silent --accept-package-agreements --accept-source-agreements",
  ];
  parts.push(`--source ${item.wingetSource}`);
  if (item.wingetScope !== "default") parts.push(`--scope ${item.wingetScope}`);
  if (item.args.trim()) parts.push(item.args.trim());
  return parts.join(" ");
}

interface PostSetupScreenProps {
  hasValidImage: boolean;
  canTargetIso: boolean;
  autounattendPath: string | null;
  outputTarget: "iso" | "image";
  isBuilding?: boolean;
  postSetupQueue: PostSetupQueueState;
  onChangePostSetupQueue: Dispatch<SetStateAction<PostSetupQueueState>>;
}

export function PostSetupScreen({
  hasValidImage,
  canTargetIso,
  autounattendPath,
  outputTarget,
  isBuilding = false,
  postSetupQueue,
  onChangePostSetupQueue,
}: PostSetupScreenProps) {
  const { t } = useTranslation();
  const [phaseFilter, setPhaseFilter] = useState<PhaseFilter>("all");
  const [scanError, setScanError] = useState<string | null>(null);
  const [isWingetOpen, setIsWingetOpen] = useState(false);
  const [isPreviewOpen, setIsPreviewOpen] = useState(false);

  const rowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const prevTopsRef = useRef<Map<string, number>>(new Map());

  const orderKey = postSetupQueue.items.map((i) => i.id).join("|");

  // Match RegistryScreen FLIP reorder animation
  useLayoutEffect(() => {
    if (prevTopsRef.current.size === 0 || prefersReducedMotion()) {
      prevTopsRef.current.clear();
      return;
    }

    const prevTops = new Map(prevTopsRef.current);
    prevTopsRef.current.clear();

    const moved: HTMLDivElement[] = [];
    for (const item of postSetupQueue.items) {
      const el = rowRefs.current.get(item.id);
      const prevTop = prevTops.get(item.id);
      if (!el || prevTop === undefined) continue;
      const deltaY = prevTop - el.getBoundingClientRect().top;
      if (Math.abs(deltaY) < 0.5) continue;
      el.style.transition = "none";
      el.style.transform = `translate3d(0, ${deltaY}px, 0)`;
      moved.push(el);
    }

    if (moved.length === 0) return;
    moved[0].getBoundingClientRect();

    const cleanups: Array<{ el: HTMLDivElement; onEnd: (ev: TransitionEvent) => void }> = [];
    for (const el of moved) {
      el.style.transition = "transform 200ms cubic-bezier(0.22, 1, 0.36, 1)";
      el.style.transform = "translate3d(0, 0, 0)";
      const onEnd = (ev: TransitionEvent) => {
        if (ev.propertyName !== "transform") return;
        el.style.transition = "";
        el.style.transform = "";
        el.removeEventListener("transitionend", onEnd);
      };
      el.addEventListener("transitionend", onEnd);
      cleanups.push({ el, onEnd });
    }

    return () => {
      for (const { el, onEnd } of cleanups) {
        el.removeEventListener("transitionend", onEnd);
        el.style.transition = "";
        el.style.transform = "";
      }
    };
  }, [orderKey]);

  const addLocalFiles = async (paths: string[]) => {
    if (isBuilding) return;
    setScanError(null);
    const added: PostSetupItem[] = [];
    for (const p of paths) {
      try {
        const res = await invoke<PostSetupFileScanResult>("scan_post_setup_file", {
          path: p,
        });
        added.push({
          id: makeItemId(res.fileName),
          kind: res.kind,
          phase: res.kind === "installer" ? "beforeLogon" : "firstLogon",
          enabled: true,
          sourcePath: res.path,
          args: res.suggestedArgs,
          wingetId: null,
          displayName: res.fileName,
          wingetSource: "winget",
          wingetScope: "default",
        });
      } catch (err) {
        setScanError(String(err));
      }
    }

    if (added.length > 0) {
      onChangePostSetupQueue((prev) => ({
        ...prev,
        items: [...prev.items, ...added],
      }));
    }
  };

  const handleBrowseInstaller = async () => {
    if (isBuilding) return;
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: t("dialogs.postSetupInstallers"), extensions: ["exe", "msi"] }],
      });
      if (Array.isArray(selected) && selected.length > 0) {
        await addLocalFiles(selected);
      }
    } catch (err) {
      setScanError(String(err));
    }
  };

  const handleBrowseScript = async () => {
    if (isBuilding) return;
    try {
      const selected = await open({
        multiple: true,
        filters: [
          { name: t("dialogs.postSetupScripts"), extensions: ["ps1", "cmd", "bat"] },
        ],
      });
      if (Array.isArray(selected) && selected.length > 0) {
        await addLocalFiles(selected);
      }
    } catch (err) {
      setScanError(String(err));
    }
  };

  const updateItem = (id: string, patch: Partial<PostSetupItem>) => {
    if (isBuilding) return;
    onChangePostSetupQueue((prev) => ({
      ...prev,
      items: prev.items.map((item) => (item.id === id ? { ...item, ...patch } : item)),
    }));
  };

  const removeItem = (id: string) => {
    if (isBuilding) return;
    onChangePostSetupQueue((prev) => ({
      ...prev,
      items: prev.items.filter((item) => item.id !== id),
    }));
  };

  const visibleItems = useMemo(() => {
    if (phaseFilter === "all") return postSetupQueue.items;
    return postSetupQueue.items.filter((i) => i.phase === phaseFilter);
  }, [phaseFilter, postSetupQueue.items]);

  const moveVisibleItem = (visibleIndex: number, delta: -1 | 1) => {
    if (isBuilding) return;
    const targetVisibleIndex = visibleIndex + delta;
    if (targetVisibleIndex < 0 || targetVisibleIndex >= visibleItems.length) return;

    const aId = visibleItems[visibleIndex].id;
    const bId = visibleItems[targetVisibleIndex].id;

    prevTopsRef.current.clear();
    for (const item of postSetupQueue.items) {
      const el = rowRefs.current.get(item.id);
      if (el) {
        prevTopsRef.current.set(item.id, el.getBoundingClientRect().top);
      }
    }

    onChangePostSetupQueue((prev) => {
      const idxA = prev.items.findIndex((x) => x.id === aId);
      const idxB = prev.items.findIndex((x) => x.id === bId);
      if (idxA === -1 || idxB === -1) return prev;
      const copy = [...prev.items];
      const [movedItem] = copy.splice(idxA, 1);
      copy.splice(idxB, 0, movedItem);
      return { ...prev, items: copy };
    });
  };

  const beforeCount = postSetupQueue.items.filter(
    (i) => i.phase === "beforeLogon"
  ).length;
  const firstCount = postSetupQueue.items.filter(
    (i) => i.phase === "firstLogon"
  ).length;
  const hasEnabled = postSetupQueue.items.some((i) => i.enabled);
  const hasWinget = postSetupQueue.items.some((i) => i.kind === "winget");
  const hasFirstLogon = postSetupQueue.items.some(
    (i) => i.enabled && i.phase === "firstLogon"
  );

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.postSetup.title")}
        subtitle={t("screens.postSetup.subtitle")}
        actions={
          <>
            {hasEnabled && (
              <button
                type="button"
                disabled={!hasValidImage || isBuilding}
                onClick={() => setIsPreviewOpen(true)}
                className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Eye className="w-4 h-4" strokeWidth={1.6} />
                <span>{t("screens.postSetup.previewBtn")}</span>
              </button>
            )}
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleBrowseInstaller}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Plus className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.postSetup.addInstallerBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={handleBrowseScript}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Terminal className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.postSetup.addScriptBtn")}</span>
            </button>
            <button
              type="button"
              disabled={!hasValidImage || isBuilding}
              onClick={() => setIsWingetOpen(true)}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Download className="w-4 h-4" strokeWidth={1.6} />
              <span>{t("screens.postSetup.addWingetBtn")}</span>
            </button>
          </>
        }
      />

      {!hasValidImage ? (
        <EmptyStateCard
          icon={Terminal}
          title={t("screens.postSetup.emptyTitle")}
          description={t("screens.postSetup.emptyNeedImage")}
        />
      ) : (
        <>
          {/* Phase & First-Logon delivery settings card */}
          <FluentReveal className="bg-surface border border-border-subtle rounded-lg p-4 space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="space-y-0.5">
                <div className="text-[13px] font-semibold text-text-primary">
                  {t("screens.postSetup.deliveryTitle")}
                </div>
                <div className="text-[12px] text-text-secondary">
                  {t("screens.postSetup.deliverySubtitle")}
                </div>
              </div>
              <FluentSegmentedControl<FirstLogonMode>
                value={postSetupQueue.firstLogonMode}
                onChange={(mode) => {
                  if (isBuilding) return;
                  onChangePostSetupQueue((prev) => ({
                    ...prev,
                    firstLogonMode: mode,
                  }));
                }}
                variant="surface"
                size="sm"
                options={[
                  {
                    value: "runOnce",
                    label: t("screens.postSetup.modeRunOnce"),
                  },
                  {
                    value: "autounattend",
                    label: t("screens.postSetup.modeAutounattend"),
                    disabled: !canTargetIso && postSetupQueue.firstLogonMode !== "autounattend",
                    title: canTargetIso
                      ? undefined
                      : t("screens.postSetup.autounattendUnavailable"),
                  },
                ]}
              />
            </div>

            <div className="flex flex-wrap items-center gap-x-6 gap-y-2 pt-1 border-t border-border-subtle text-[12.5px]">
              <label className="flex items-center gap-2.5 cursor-pointer text-text-primary">
                <FluentSwitch
                  checked={postSetupQueue.cleanupTempFiles}
                  disabled={isBuilding}
                  label={t("screens.postSetup.cleanupPayloadLabel")}
                  onToggle={() =>
                    onChangePostSetupQueue((prev) => ({
                      ...prev,
                      cleanupTempFiles: !prev.cleanupTempFiles,
                    }))
                  }
                />
                <span>{t("screens.postSetup.cleanupPayloadLabel")}</span>
              </label>

              {hasWinget && (
                <label className="flex items-center gap-2.5 cursor-pointer text-text-primary">
                  <FluentSwitch
                    checked={postSetupQueue.waitForNetwork}
                    disabled={isBuilding}
                    label={t("screens.postSetup.waitForNetworkLabel")}
                    onToggle={() =>
                      onChangePostSetupQueue((prev) => ({
                        ...prev,
                        waitForNetwork: !prev.waitForNetwork,
                      }))
                    }
                  />
                  <span>{t("screens.postSetup.waitForNetworkLabel")}</span>
                </label>
              )}
            </div>

            <div className="flex items-start gap-2 px-3 py-2 rounded-md bg-canvas border border-border-subtle text-[12px] text-text-secondary">
              <Info className="w-4 h-4 text-accent shrink-0 mt-0.5" strokeWidth={1.6} />
              <span className="leading-relaxed">
                {t("screens.postSetup.oemNotice")}
              </span>
            </div>

            {hasFirstLogon &&
              postSetupQueue.firstLogonMode === "autounattend" &&
              !canTargetIso && (
                <div className="flex items-center gap-2 px-3 py-2 rounded-md bg-status-warning-bg text-status-warning text-[12px]">
                  <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                  <span>{t("screens.postSetup.autounattendUnavailable")}</span>
                </div>
              )}
          </FluentReveal>

          {scanError && (
            <FluentReveal className="flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px]">
              <div className="flex items-center gap-2 min-w-0">
                <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span className="break-words">
                  {t("screens.postSetup.scanError", {
                    error: localizeBackendError(scanError),
                  })}
                </span>
              </div>
              <button
                type="button"
                onClick={() => setScanError(null)}
                className="p-1 rounded hover:bg-subtle transition-colors shrink-0"
              >
                <X className="w-3.5 h-3.5" strokeWidth={1.7} />
              </button>
            </FluentReveal>
          )}

          {postSetupQueue.items.length === 0 ? (
            <EmptyStateCard
              icon={Terminal}
              title={t("screens.postSetup.emptyTitle")}
              description={t("screens.postSetup.emptyDesc")}
              action={
                <div className="flex flex-wrap items-center justify-center gap-2">
                  <button
                    type="button"
                    onClick={handleBrowseInstaller}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                  >
                    <Plus className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.postSetup.addInstallerBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleBrowseScript}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
                  >
                    <Terminal className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.postSetup.addScriptBtn")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => setIsWingetOpen(true)}
                    className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98]"
                  >
                    <Download className="w-4 h-4" strokeWidth={1.6} />
                    <span>{t("screens.postSetup.addWingetBtn")}</span>
                  </button>
                </div>
              }
            />
          ) : (
            <div className="space-y-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <FluentSegmentedControl<PhaseFilter>
                  value={phaseFilter}
                  onChange={setPhaseFilter}
                  variant="accent"
                  size="sm"
                  options={[
                    {
                      value: "all",
                      label: `${t("screens.postSetup.filterAll")} (${postSetupQueue.items.length})`,
                    },
                    {
                      value: "beforeLogon",
                      label: `${t("screens.postSetup.phaseBeforeLogon")} (${beforeCount})`,
                    },
                    {
                      value: "firstLogon",
                      label: `${t("screens.postSetup.phaseFirstLogon")} (${firstCount})`,
                    },
                  ]}
                />
              </div>

              {visibleItems.length === 0 ? (
                <div className="bg-surface border border-border-subtle rounded-lg px-5 py-8 text-center text-[13px] text-text-secondary">
                  {t("screens.postSetup.noPhaseMatches")}
                </div>
              ) : (
                <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
                  {visibleItems.map((item, idx) => {
                    const baseName =
                      item.kind === "winget"
                        ? item.displayName || item.wingetId || "winget"
                        : getBaseName(item.sourcePath) || item.displayName || "";
                    const ext = getBaseName(item.sourcePath)
                      .split(".")
                      .pop()
                      ?.toLowerCase();
                    const argPresets =
                      item.kind === "installer"
                        ? ext === "msi"
                          ? MSI_ARG_PRESETS
                          : EXE_ARG_PRESETS
                        : null;

                    return (
                      <div
                        key={item.id}
                        ref={(el) => {
                          if (el) rowRefs.current.set(item.id, el);
                          else rowRefs.current.delete(item.id);
                        }}
                      >
                        <FluentReveal
                          className={`px-4 py-3.5 space-y-3 transition-opacity ${
                            item.enabled ? "" : "opacity-60"
                          }`}
                        >
                          {/* Top row */}
                          <div className="flex items-start justify-between gap-3">
                            <div className="flex items-start gap-3 min-w-0">
                              <span className="px-1.5 py-0.5 rounded text-[11px] font-mono bg-subtle text-text-secondary shrink-0 mt-0.5">
                                #{idx + 1}
                              </span>
                              {item.kind === "installer" ? (
                                <Package
                                  className="w-4 h-4 text-accent shrink-0 mt-1"
                                  strokeWidth={1.7}
                                />
                              ) : item.kind === "winget" ? (
                                <Download
                                  className="w-4 h-4 text-accent shrink-0 mt-1"
                                  strokeWidth={1.7}
                                />
                              ) : (
                                <Terminal
                                  className="w-4 h-4 text-accent shrink-0 mt-1"
                                  strokeWidth={1.7}
                                />
                              )}
                              <div className="min-w-0">
                                <div className="flex flex-wrap items-center gap-2">
                                  <span className="text-[13.5px] font-semibold text-text-primary truncate">
                                    {baseName}
                                  </span>
                                  <span className="px-1.5 py-0.5 rounded text-[11px] font-medium bg-subtle text-text-secondary shrink-0">
                                    {item.kind === "installer"
                                      ? t("screens.postSetup.kindInstaller")
                                      : item.kind === "winget"
                                      ? t("screens.postSetup.kindWinget")
                                      : t("screens.postSetup.kindScript")}
                                  </span>
                                  {item.kind === "winget" && item.wingetId && (
                                    <span className="px-1.5 py-0.5 rounded text-[11px] font-mono bg-canvas border border-border-subtle text-text-secondary shrink-0">
                                      {item.wingetId}
                                    </span>
                                  )}
                                </div>
                                {item.sourcePath && (
                                  <div className="text-[11.5px] font-mono text-text-secondary truncate mt-0.5">
                                    {item.sourcePath}
                                  </div>
                                )}
                              </div>
                            </div>

                            <div className="flex items-center gap-1.5 shrink-0">
                              <FluentSwitch
                                checked={item.enabled}
                                disabled={isBuilding}
                                label={t("screens.postSetup.enableToggle")}
                                onToggle={() =>
                                  updateItem(item.id, { enabled: !item.enabled })
                                }
                              />
                              <button
                                type="button"
                                disabled={isBuilding || idx === 0}
                                onClick={() => moveVisibleItem(idx, -1)}
                                title={t("screens.postSetup.moveUp")}
                                className="p-1.5 rounded-md bg-subtle hover:bg-border-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.95] disabled:opacity-40 disabled:cursor-not-allowed"
                              >
                                <ArrowUp className="w-3.5 h-3.5" strokeWidth={1.7} />
                              </button>
                              <button
                                type="button"
                                disabled={isBuilding || idx === visibleItems.length - 1}
                                onClick={() => moveVisibleItem(idx, 1)}
                                title={t("screens.postSetup.moveDown")}
                                className="p-1.5 rounded-md bg-subtle hover:bg-border-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.95] disabled:opacity-40 disabled:cursor-not-allowed"
                              >
                                <ArrowDown className="w-3.5 h-3.5" strokeWidth={1.7} />
                              </button>
                              <button
                                type="button"
                                disabled={isBuilding}
                                onClick={() => removeItem(item.id)}
                                title={t("screens.postSetup.remove")}
                                className="p-1.5 rounded-md bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-all duration-150 active:scale-[0.95] disabled:opacity-40 disabled:cursor-not-allowed"
                              >
                                <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                              </button>
                            </div>
                          </div>

                          {/* Phase + options */}
                          <div className="pl-10 space-y-2.5">
                            <div className="flex flex-wrap items-center gap-3">
                              <span className="text-[12px] text-text-secondary shrink-0">
                                {t("screens.postSetup.phaseLabel")}:
                              </span>
                              <FluentSegmentedControl<PostSetupPhase>
                                value={item.phase}
                                onChange={(phase) => updateItem(item.id, { phase })}
                                variant="surface"
                                size="xs"
                                options={[
                                  {
                                    value: "beforeLogon",
                                    label: t("screens.postSetup.phaseBeforeLogonShort"),
                                    disabled: item.kind === "winget",
                                    title:
                                      item.kind === "winget"
                                        ? t("screens.postSetup.wingetUserOnlyHint")
                                        : undefined,
                                  },
                                  {
                                    value: "firstLogon",
                                    label: t("screens.postSetup.phaseFirstLogonShort"),
                                  },
                                ]}
                              />

                              {item.kind === "winget" && (
                                <>
                                  <FluentSegmentedControl<WingetSource>
                                    value={item.wingetSource}
                                    onChange={(wingetSource) =>
                                      updateItem(item.id, { wingetSource })
                                    }
                                    variant="surface"
                                    size="xs"
                                    options={[
                                      { value: "winget", label: "winget" },
                                      { value: "msstore", label: "msstore" },
                                    ]}
                                  />
                                  <FluentSegmentedControl<WingetScope>
                                    value={item.wingetScope}
                                    onChange={(wingetScope) =>
                                      updateItem(item.id, { wingetScope })
                                    }
                                    variant="surface"
                                    size="xs"
                                    options={[
                                      {
                                        value: "default",
                                        label: t("screens.postSetup.scopeDefault"),
                                      },
                                      {
                                        value: "machine",
                                        label: t("screens.postSetup.scopeMachine"),
                                      },
                                      {
                                        value: "user",
                                        label: t("screens.postSetup.scopeUser"),
                                      },
                                    ]}
                                  />
                                </>
                              )}
                            </div>

                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-[12px] text-text-secondary shrink-0">
                                {item.kind === "winget"
                                  ? t("screens.postSetup.extraArgsLabel")
                                  : t("screens.postSetup.argsLabel")}
                                :
                              </span>
                              <input
                                type="text"
                                disabled={isBuilding}
                                value={item.args}
                                onChange={(e) =>
                                  updateItem(item.id, { args: e.target.value })
                                }
                                placeholder={
                                  item.kind === "installer"
                                    ? t("screens.postSetup.argsPlaceholderInstaller")
                                    : t("screens.postSetup.argsPlaceholderOptional")
                                }
                                className="flex-1 min-w-[200px] px-2.5 py-1 rounded-md bg-canvas border border-border-subtle text-[12px] font-mono text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
                              />
                            </div>

                            {argPresets && (
                              <div className="flex flex-wrap items-center gap-1.5">
                                {argPresets.map((presetArg) => (
                                  <button
                                    key={presetArg}
                                    type="button"
                                    disabled={isBuilding}
                                    onClick={() => updateItem(item.id, { args: presetArg })}
                                    className="px-2 py-0.5 rounded text-[11px] font-mono bg-canvas border border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.97]"
                                  >
                                    {presetArg}
                                  </button>
                                ))}
                              </div>
                            )}

                            {item.kind === "winget" && (
                              <div className="px-2.5 py-1.5 rounded bg-canvas border border-border-subtle text-[11.5px] font-mono text-text-secondary break-all">
                                {formatWingetPreview(item)}
                              </div>
                            )}
                          </div>
                        </FluentReveal>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          )}
        </>
      )}

      {isWingetOpen && (
        <Suspense fallback={null}>
          <AddWingetModal
            existingIds={postSetupQueue.items
              .map((i) => i.wingetId ?? "")
              .filter(Boolean)}
            onAddItems={(newItems) => {
              onChangePostSetupQueue((prev) => ({
                ...prev,
                items: [...prev.items, ...newItems],
              }));
            }}
            onClose={() => setIsWingetOpen(false)}
          />
        </Suspense>
      )}

      {isPreviewOpen && (
        <Suspense fallback={null}>
          <PostSetupPreviewModal
            plan={postSetupQueue}
            autounattendPath={autounattendPath}
            target={outputTarget}
            onClose={() => setIsPreviewOpen(false)}
          />
        </Suspense>
      )}
    </div>
  );
}
