import {
  useDeferredValue,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import {
  FolderOpen,
  Info,
  Loader2,
  RotateCcw,
  Search,
  SlidersHorizontal,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { FluentReveal, FluentSegmentedControl } from "../FluentMotion";
import type {
  FeaturesQueueState,
  ImageCatalog,
  ImageInspectionResult,
} from "../../types/studio";

type FeatureTab = "optional" | "capabilities";
type FeatureFilter = "all" | "enabled" | "disabled" | "changed";

const UNSAFE_KEYS = new Set(["__proto__", "constructor", "prototype"]);

function cloneSafeOverrides<T>(
  source: Record<string, T> | undefined,
  patch: (item: T) => T
): Record<string, T> {
  const next = Object.create(null) as Record<string, T>;
  if (!source) return next;
  for (const [k, v] of Object.entries(source)) {
    if (UNSAFE_KEYS.has(k)) continue;
    next[k] = patch(v);
  }
  return next;
}

interface FeaturesScreenProps {
  inspection: ImageInspectionResult | null;
  catalog: ImageCatalog | null;
  catalogLoading: boolean;
  featuresQueue: FeaturesQueueState;
  onChangeFeaturesQueue: Dispatch<SetStateAction<FeaturesQueueState>>;
  isBuilding?: boolean;
}

function isInitiallyEnabledFeature(state: string): boolean {
  const s = state.toLowerCase();
  return s === "enabled" || s === "enablepending";
}

function isInitiallyInstalledCap(state: string): boolean {
  const s = state.toLowerCase();
  return s === "installed" || s === "installpending" || s === "staged";
}

const DISM_STATE_KEYS = {
  enabled: "screens.features.stateEnabled",
  disabled: "screens.features.stateDisabled",
  enablepending: "screens.features.stateEnablePending",
  disablepending: "screens.features.stateDisablePending",
  disabledwithpayloadremoved: "screens.features.stateDisabledPayloadRemoved",
  installed: "screens.features.stateInstalled",
  notpresent: "screens.features.stateNotPresent",
  staged: "screens.features.stateStaged",
  installpending: "screens.features.stateInstallPending",
  uninstallpending: "screens.features.stateUninstallPending",
} as const;

function FluentSwitch({
  checked,
  disabled = false,
  label,
  onToggle,
}: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={onToggle}
      className={`group w-10 h-5 rounded-full border transition-all duration-200 ease-out relative flex items-center shrink-0 disabled:opacity-50 disabled:cursor-not-allowed ${
        checked
          ? "bg-accent border-accent"
          : "bg-switch-off-bg border-switch-off-border hover:border-text-primary/70"
      }`}
    >
      <span
        className={`block w-3 h-3 rounded-full origin-center transition-transform duration-200 ease-out group-active:scale-x-[1.18] ${
          checked
            ? "translate-x-[22px] bg-accent-fg"
            : "translate-x-[3px] bg-text-secondary"
        }`}
      />
    </button>
  );
}

export function FeaturesScreen({
  inspection,
  catalog,
  catalogLoading,
  featuresQueue,
  onChangeFeaturesQueue,
  isBuilding = false,
}: FeaturesScreenProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<FeatureTab>("optional");
  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  const [filter, setFilter] = useState<FeatureFilter>("all");

  const translateDismState = (state: string): string => {
    const norm = state.toLowerCase().replace(/[^a-z]/g, "");
    if (norm in DISM_STATE_KEYS) {
      return t(DISM_STATE_KEYS[norm as keyof typeof DISM_STATE_KEYS]);
    }
    return state;
  };

  const enableFeatSet = useMemo(
    () => new Set(featuresQueue.enableFeatures),
    [featuresQueue.enableFeatures]
  );
  const disableFeatSet = useMemo(
    () => new Set(featuresQueue.disableFeatures),
    [featuresQueue.disableFeatures]
  );
  const addCapSet = useMemo(
    () => new Set(featuresQueue.addCapabilities),
    [featuresQueue.addCapabilities]
  );
  const removeCapSet = useMemo(
    () => new Set(featuresQueue.removeCapabilities),
    [featuresQueue.removeCapabilities]
  );

  const toggleFeature = (name: string, baseEnabled: boolean) => {
    if (isBuilding) return;
    onChangeFeaturesQueue((prev) => {
      const nextEnable = new Set(prev.enableFeatures);
      const nextDisable = new Set(prev.disableFeatures);

      if (baseEnabled) {
        if (nextDisable.has(name)) {
          nextDisable.delete(name);
        } else {
          nextDisable.add(name);
        }
        nextEnable.delete(name);
      } else {
        if (nextEnable.has(name)) {
          nextEnable.delete(name);
        } else {
          nextEnable.add(name);
        }
        nextDisable.delete(name);
      }

      return {
        ...prev,
        enableFeatures: Array.from(nextEnable),
        disableFeatures: Array.from(nextDisable),
      };
    });
  };

  const toggleCapability = (name: string, baseInstalled: boolean) => {
    if (isBuilding) return;
    onChangeFeaturesQueue((prev) => {
      const nextAdd = new Set(prev.addCapabilities);
      const nextRemove = new Set(prev.removeCapabilities);

      if (baseInstalled) {
        if (nextRemove.has(name)) {
          nextRemove.delete(name);
        } else {
          nextRemove.add(name);
        }
        nextAdd.delete(name);
      } else {
        if (nextAdd.has(name)) {
          nextAdd.delete(name);
        } else {
          nextAdd.add(name);
        }
        nextRemove.delete(name);
      }

      return {
        ...prev,
        addCapabilities: Array.from(nextAdd),
        removeCapabilities: Array.from(nextRemove),
      };
    });
  };

  const updateSxsPath = (nextPath: string | null) => {
    if (isBuilding) return;
    onChangeFeaturesQueue((prev) => ({
      ...prev,
      customSxsPath: nextPath,
      featureOverrides: cloneSafeOverrides(prev.featureOverrides, (v) => ({
        ...v,
        customSxsPath: nextPath,
      })),
      capabilityOverrides: cloneSafeOverrides(prev.capabilityOverrides, (v) => ({
        ...v,
        customSxsPath: nextPath,
      })),
    }));
  };

  const handleBrowseSxs = async () => {
    if (isBuilding) return;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (typeof selected === "string" && selected.trim()) {
        updateSxsPath(selected);
      }
    } catch {
      // ignore dialog errors
    }
  };

  const filteredFeatures = useMemo(() => {
    if (!catalog) return [];
    const q = deferredSearch.trim().toLowerCase();
    return catalog.features.filter((f) => {
      const baseOn = isInitiallyEnabledFeature(f.state);
      const changed = enableFeatSet.has(f.name) || disableFeatSet.has(f.name);
      const effectiveOn = baseOn ? !disableFeatSet.has(f.name) : enableFeatSet.has(f.name);

      if (filter === "enabled" && !effectiveOn) return false;
      if (filter === "disabled" && effectiveOn) return false;
      if (filter === "changed" && !changed) return false;
      if (q && !f.name.toLowerCase().includes(q)) return false;
      return true;
    });
  }, [catalog, deferredSearch, disableFeatSet, enableFeatSet, filter]);

  const filteredCapabilities = useMemo(() => {
    if (!catalog) return [];
    const q = deferredSearch.trim().toLowerCase();
    return catalog.capabilities.filter((c) => {
      const baseOn = isInitiallyInstalledCap(c.state);
      const changed = addCapSet.has(c.name) || removeCapSet.has(c.name);
      const effectiveOn = baseOn ? !removeCapSet.has(c.name) : addCapSet.has(c.name);

      if (filter === "enabled" && !effectiveOn) return false;
      if (filter === "disabled" && effectiveOn) return false;
      if (filter === "changed" && !changed) return false;
      if (q && !c.name.toLowerCase().includes(q)) return false;
      return true;
    });
  }, [addCapSet, catalog, deferredSearch, filter, removeCapSet]);

  const showSourceCard =
    featuresQueue.enableFeatures.length > 0 || featuresQueue.addCapabilities.length > 0;

  const hasQueuedChanges =
    featuresQueue.enableFeatures.length > 0 ||
    featuresQueue.disableFeatures.length > 0 ||
    featuresQueue.addCapabilities.length > 0 ||
    featuresQueue.removeCapabilities.length > 0;

  const handleResetChanges = () => {
    if (isBuilding) return;
    onChangeFeaturesQueue((prev) => ({
      ...prev,
      enableFeatures: [],
      disableFeatures: [],
      addCapabilities: [],
      removeCapabilities: [],
      featureOverrides: Object.create(null) as Record<string, never>,
      capabilityOverrides: Object.create(null) as Record<string, never>,
    }));
  };

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.features.title")}
        subtitle={t("screens.features.subtitle")}
        actions={
          catalog && hasQueuedChanges ? (
            <button
              type="button"
              disabled={isBuilding}
              onClick={handleResetChanges}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <RotateCcw className="w-3.5 h-3.5" strokeWidth={1.7} />
              <span>{t("screens.features.resetQueueBtn")}</span>
            </button>
          ) : undefined
        }
      />

      {catalogLoading && (
        <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-10 flex flex-col items-center gap-3 text-center">
          <Loader2 className="w-6 h-6 animate-spin text-accent" strokeWidth={1.7} />
          <div className="text-[13.5px] font-medium text-text-primary">
            {t("screens.features.loading")}
          </div>
        </FluentReveal>
      )}

      {!catalogLoading && !catalog && (
        <EmptyStateCard
          icon={SlidersHorizontal}
          title={t("screens.features.emptyTitle")}
          description={t("screens.features.emptyDesc")}
        />
      )}

      {!catalogLoading && catalog && (
        <>
          {/* Tab + Search + Filter */}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <FluentSegmentedControl
              value={tab}
              onChange={setTab}
              variant="accent"
              size="md"
              options={[
                {
                  value: "optional",
                  label: `${t("screens.features.tabOptional")} (${catalog.features.length})`,
                },
                {
                  value: "capabilities",
                  label: `${t("screens.features.tabCapabilities")} (${catalog.capabilities.length})`,
                },
              ]}
            />

            <div className="flex flex-wrap items-center gap-2 flex-1 justify-end min-w-[min(100%,460px)]">
              <div className="relative flex-1 min-w-[200px]">
                <Search
                  className="w-4 h-4 text-text-secondary absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none"
                  strokeWidth={1.6}
                />
                <input
                  type="text"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder={t("screens.features.searchPlaceholder")}
                  className="w-full pl-9 pr-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[12.5px] text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent"
                />
              </div>

              <FluentSegmentedControl
                value={filter}
                onChange={setFilter}
                variant="accent"
                size="sm"
                options={[
                  { value: "all", label: t("screens.features.filterAll") },
                  { value: "enabled", label: t("screens.features.filterEnabled") },
                  { value: "disabled", label: t("screens.features.filterDisabled") },
                  { value: "changed", label: t("screens.features.filterChanged") },
                ]}
              />
            </div>
          </div>

          {/* Contextual sxs / EnableAll / LimitAccess card */}
          {showSourceCard && (
            <FluentReveal className="bg-surface border border-border-subtle rounded-lg p-4 space-y-3">
              <div className="text-[13px] font-semibold text-text-primary">
                {t("screens.features.sxsCardTitle")}
              </div>
              {inspection?.hasSxs && !featuresQueue.customSxsPath && (
                <div className="flex items-center gap-2 px-3 py-2 rounded-md bg-subtle text-[12px] text-text-secondary">
                  <Info className="w-4 h-4 text-accent shrink-0" strokeWidth={1.6} />
                  <span>{t("screens.features.sxsAutoDetected")}</span>
                </div>
              )}
              <div className="space-y-1">
                <label className="block text-[12px] text-text-secondary">
                  {t("screens.features.sxsManualLabel")}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    disabled={isBuilding}
                    value={featuresQueue.customSxsPath ?? ""}
                    placeholder={t("screens.features.sxsPlaceholder")}
                    onChange={(e) =>
                      updateSxsPath(
                        e.target.value.trim() ? e.target.value : null
                      )
                    }
                    className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12px] font-mono text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent disabled:opacity-50"
                  />
                  <button
                    type="button"
                    disabled={isBuilding}
                    onClick={handleBrowseSxs}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] shrink-0 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                    {t("screens.features.browse")}
                  </button>
                  {featuresQueue.customSxsPath && (
                    <button
                      type="button"
                      disabled={isBuilding}
                      onClick={() => updateSxsPath(null)}
                      className="px-2.5 py-1.5 rounded-md text-[12px] text-text-secondary hover:bg-subtle transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      {t("screens.features.clear")}
                    </button>
                  )}
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-5 pt-1 text-[12.5px]">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    disabled={isBuilding}
                    checked={featuresQueue.enableAll}
                    onChange={(e) => {
                      if (isBuilding) return;
                      const val = e.target.checked;
                      onChangeFeaturesQueue((prev) => ({
                        ...prev,
                        enableAll: val,
                        featureOverrides: cloneSafeOverrides(
                          prev.featureOverrides,
                          (v) => ({ ...v, enableAll: val })
                        ),
                      }));
                    }}
                    className="accent-accent disabled:opacity-50"
                  />
                  <span>{t("screens.features.enableAllLabel")}</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    disabled={isBuilding}
                    checked={featuresQueue.limitAccess}
                    onChange={(e) => {
                      if (isBuilding) return;
                      const val = e.target.checked;
                      onChangeFeaturesQueue((prev) => ({
                        ...prev,
                        limitAccess: val,
                        capabilityOverrides: cloneSafeOverrides(
                          prev.capabilityOverrides,
                          (v) => ({ ...v, limitAccess: val })
                        ),
                      }));
                    }}
                    className="accent-accent disabled:opacity-50"
                  />
                  <span>{t("screens.features.limitAccessLabel")}</span>
                </label>
              </div>
            </FluentReveal>
          )}

          {/* List */}
          {tab === "optional" ? (
            filteredFeatures.length === 0 ? (
              <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-8 text-center text-[13px] text-text-secondary">
                {t("screens.features.noMatches")}
              </FluentReveal>
            ) : (
              <FluentReveal
                key="tab-optional"
                className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden"
              >
                {filteredFeatures.map((feat) => {
                  const baseOn = isInitiallyEnabledFeature(feat.state);
                  const pendingOn = enableFeatSet.has(feat.name);
                  const pendingOff = disableFeatSet.has(feat.name);
                  const effectiveOn = baseOn ? !pendingOff : pendingOn;
                  const changed = pendingOn || pendingOff;

                  return (
                    <div
                      key={feat.name}
                      className={`px-4 py-2.5 flex items-center justify-between gap-4 transition-colors ${
                        changed ? "bg-subtle" : "hover:bg-subtle"
                      }`}
                    >
                      <div className="min-w-0">
                        <div className="text-[13px] font-mono font-medium text-text-primary truncate">
                          {feat.name}
                        </div>
                        <div className="flex items-center gap-2 mt-0.5 text-[11.5px]">
                          <span
                            className={
                              baseOn ? "text-status-success" : "text-text-secondary"
                            }
                          >
                            {translateDismState(feat.state)}
                          </span>
                          {changed && (
                            <span
                              className={`animate-fluent-icon-pop px-1.5 py-0.5 rounded font-medium ${
                                effectiveOn
                                  ? "bg-status-success-bg text-status-success"
                                  : "bg-status-warning-bg text-status-warning"
                              }`}
                            >
                              {effectiveOn
                                ? t("screens.features.pendingEnable")
                                : t("screens.features.pendingDisable")}
                            </span>
                          )}
                        </div>
                      </div>

                      <FluentSwitch
                        checked={effectiveOn}
                        disabled={isBuilding}
                        label={feat.name}
                        onToggle={() => toggleFeature(feat.name, baseOn)}
                      />
                    </div>
                  );
                })}
              </FluentReveal>
            )
          ) : filteredCapabilities.length === 0 ? (
            <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-8 text-center text-[13px] text-text-secondary">
              {t("screens.features.noMatches")}
            </FluentReveal>
          ) : (
            <FluentReveal
              key="tab-capabilities"
              className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden"
            >
              {filteredCapabilities.map((cap) => {
                const baseOn = isInitiallyInstalledCap(cap.state);
                const pendingOn = addCapSet.has(cap.name);
                const pendingOff = removeCapSet.has(cap.name);
                const effectiveOn = baseOn ? !pendingOff : pendingOn;
                const changed = pendingOn || pendingOff;

                return (
                  <div
                    key={cap.name}
                    className={`px-4 py-2.5 flex items-center justify-between gap-4 transition-colors ${
                      changed ? "bg-subtle" : "hover:bg-subtle"
                    }`}
                  >
                    <div className="min-w-0">
                      <div className="text-[13px] font-mono font-medium text-text-primary truncate">
                        {cap.name}
                      </div>
                      <div className="flex items-center gap-2 mt-0.5 text-[11.5px]">
                        <span
                          className={
                            baseOn ? "text-status-success" : "text-text-secondary"
                          }
                        >
                          {translateDismState(cap.state)}
                        </span>
                        {changed && (
                          <span
                            className={`animate-fluent-icon-pop px-1.5 py-0.5 rounded font-medium ${
                              effectiveOn
                                ? "bg-status-success-bg text-status-success"
                                : "bg-status-warning-bg text-status-warning"
                            }`}
                          >
                            {effectiveOn
                              ? t("screens.features.pendingEnable")
                              : t("screens.features.pendingDisable")}
                          </span>
                        )}
                      </div>
                    </div>

                    <FluentSwitch
                      checked={effectiveOn}
                      disabled={isBuilding}
                      label={cap.name}
                      onToggle={() => toggleCapability(cap.name, baseOn)}
                    />
                  </div>
                );
              })}
            </FluentReveal>
          )}
        </>
      )}
    </div>
  );
}
