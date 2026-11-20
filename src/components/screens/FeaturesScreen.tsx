import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Info, Loader2, Search, SlidersHorizontal } from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import type {
  FeaturesQueueState,
  ImageCatalog,
  ImageInspectionResult,
} from "../../types/studio";

type FeatureTab = "optional" | "capabilities";
type FeatureFilter = "all" | "enabled" | "disabled" | "changed";

interface FeaturesScreenProps {
  inspection: ImageInspectionResult | null;
  catalog: ImageCatalog | null;
  catalogLoading: boolean;
  featuresQueue: FeaturesQueueState;
  onChangeFeaturesQueue: (next: FeaturesQueueState) => void;
}

function isInitiallyEnabledFeature(state: string): boolean {
  const s = state.toLowerCase();
  return s === "enabled" || s === "enablepending";
}

function isInitiallyInstalledCap(state: string): boolean {
  const s = state.toLowerCase();
  return s === "installed" || s === "installpending" || s === "staged";
}

function FluentSwitch({
  checked,
  onToggle,
}: {
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={onToggle}
      className={`w-10 h-5 rounded-full border transition-all duration-200 ease-out relative flex items-center shrink-0 ${
        checked
          ? "bg-accent border-accent"
          : "bg-switch-off-bg border-switch-off-border hover:border-text-primary/70"
      }`}
    >
      <span
        className={`block w-3 h-3 rounded-full transition-transform duration-200 ease-out ${
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
}: FeaturesScreenProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<FeatureTab>("optional");
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<FeatureFilter>("all");

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
    const nextEnable = new Set(featuresQueue.enableFeatures);
    const nextDisable = new Set(featuresQueue.disableFeatures);

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

    onChangeFeaturesQueue({
      ...featuresQueue,
      enableFeatures: Array.from(nextEnable),
      disableFeatures: Array.from(nextDisable),
    });
  };

  const toggleCapability = (name: string, baseInstalled: boolean) => {
    const nextAdd = new Set(featuresQueue.addCapabilities);
    const nextRemove = new Set(featuresQueue.removeCapabilities);

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

    onChangeFeaturesQueue({
      ...featuresQueue,
      addCapabilities: Array.from(nextAdd),
      removeCapabilities: Array.from(nextRemove),
    });
  };

  const handleBrowseSxs = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (typeof selected === "string" && selected.trim()) {
      onChangeFeaturesQueue({ ...featuresQueue, customSxsPath: selected });
    }
  };

  const filteredFeatures = useMemo(() => {
    if (!catalog) return [];
    const q = search.trim().toLowerCase();
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
  }, [catalog, disableFeatSet, enableFeatSet, filter, search]);

  const filteredCapabilities = useMemo(() => {
    if (!catalog) return [];
    const q = search.trim().toLowerCase();
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
  }, [addCapSet, catalog, filter, removeCapSet, search]);

  const showSourceCard =
    featuresQueue.enableFeatures.length > 0 || featuresQueue.addCapabilities.length > 0;

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.features.title")}
        subtitle={t("screens.features.subtitle")}
      />

      {catalogLoading && (
        <div className="bg-surface border border-border-subtle rounded-lg px-6 py-10 flex flex-col items-center gap-3 text-center">
          <Loader2 className="w-6 h-6 animate-spin text-accent" strokeWidth={1.7} />
          <div className="text-[13.5px] font-medium text-text-primary">
            {t("screens.features.loading")}
          </div>
        </div>
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
            <div className="inline-flex rounded-md border border-border-subtle bg-surface p-0.5 text-[12.5px] font-medium">
              <button
                type="button"
                onClick={() => setTab("optional")}
                className={`px-3 py-1 rounded-[4px] transition-colors ${
                  tab === "optional"
                    ? "bg-accent text-accent-fg"
                    : "text-text-secondary hover:text-text-primary"
                }`}
              >
                {t("screens.features.tabOptional")} ({catalog.features.length})
              </button>
              <button
                type="button"
                onClick={() => setTab("capabilities")}
                className={`px-3 py-1 rounded-[4px] transition-colors ${
                  tab === "capabilities"
                    ? "bg-accent text-accent-fg"
                    : "text-text-secondary hover:text-text-primary"
                }`}
              >
                {t("screens.features.tabCapabilities")} ({catalog.capabilities.length})
              </button>
            </div>

            <div className="flex flex-wrap items-center gap-2 flex-1 justify-end min-w-[260px]">
              <div className="relative flex-1 max-w-[280px]">
                <Search
                  className="w-4 h-4 text-text-secondary absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none"
                  strokeWidth={1.6}
                />
                <input
                  type="text"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder={t("screens.features.searchPlaceholder")}
                  className="w-full pl-9 pr-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[12.5px] text-text-primary focus:outline-none focus:border-accent"
                />
              </div>

              <div className="flex items-center gap-1 rounded-md border border-border-subtle bg-surface p-0.5 text-[12px] font-medium">
                {(
                  [
                    ["all", t("screens.features.filterAll")],
                    ["enabled", t("screens.features.filterEnabled")],
                    ["disabled", t("screens.features.filterDisabled")],
                    ["changed", t("screens.features.filterChanged")],
                  ] as [FeatureFilter, string][]
                ).map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    onClick={() => setFilter(id)}
                    className={`px-2.5 py-1 rounded-[4px] transition-colors ${
                      filter === id
                        ? "bg-accent text-accent-fg"
                        : "text-text-secondary hover:text-text-primary"
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
          </div>

          {/* Contextual sxs / EnableAll / LimitAccess card */}
          {showSourceCard && (
            <div className="animate-fluent-item bg-surface border border-border-subtle rounded-lg p-4 space-y-3">
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
                    value={featuresQueue.customSxsPath ?? ""}
                    placeholder={t("screens.features.sxsPlaceholder")}
                    onChange={(e) =>
                      onChangeFeaturesQueue({
                        ...featuresQueue,
                        customSxsPath: e.target.value.trim() ? e.target.value : null,
                      })
                    }
                    className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12px] font-mono text-text-primary"
                  />
                  <button
                    type="button"
                    onClick={handleBrowseSxs}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary shrink-0"
                  >
                    <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                    {t("screens.features.browse")}
                  </button>
                  {featuresQueue.customSxsPath && (
                    <button
                      type="button"
                      onClick={() =>
                        onChangeFeaturesQueue({ ...featuresQueue, customSxsPath: null })
                      }
                      className="px-2.5 py-1.5 rounded-md text-[12px] text-text-secondary hover:bg-subtle"
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
                    checked={featuresQueue.enableAll}
                    onChange={(e) =>
                      onChangeFeaturesQueue({
                        ...featuresQueue,
                        enableAll: e.target.checked,
                      })
                    }
                    className="accent-accent"
                  />
                  <span>{t("screens.features.enableAllLabel")}</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={featuresQueue.limitAccess}
                    onChange={(e) =>
                      onChangeFeaturesQueue({
                        ...featuresQueue,
                        limitAccess: e.target.checked,
                      })
                    }
                    className="accent-accent"
                  />
                  <span>{t("screens.features.limitAccessLabel")}</span>
                </label>
              </div>
            </div>
          )}

          {/* List */}
          {tab === "optional" ? (
            filteredFeatures.length === 0 ? (
              <div className="bg-surface border border-border-subtle rounded-lg px-6 py-8 text-center text-[13px] text-text-secondary">
                {t("screens.features.noMatches")}
              </div>
            ) : (
              <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
                {filteredFeatures.map((feat) => {
                  const baseOn = isInitiallyEnabledFeature(feat.state);
                  const pendingOn = enableFeatSet.has(feat.name);
                  const pendingOff = disableFeatSet.has(feat.name);
                  const effectiveOn = baseOn ? !pendingOff : pendingOn;
                  const changed = pendingOn || pendingOff;

                  return (
                    <div
                      key={feat.name}
                      className={`px-4 py-2.5 flex items-center justify-between gap-4 ${
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
                            {feat.state}
                          </span>
                          {changed && (
                            <span
                              className={`px-1.5 py-0.5 rounded font-medium ${
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
                        onToggle={() => toggleFeature(feat.name, baseOn)}
                      />
                    </div>
                  );
                })}
              </div>
            )
          ) : filteredCapabilities.length === 0 ? (
            <div className="bg-surface border border-border-subtle rounded-lg px-6 py-8 text-center text-[13px] text-text-secondary">
              {t("screens.features.noMatches")}
            </div>
          ) : (
            <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {filteredCapabilities.map((cap) => {
                const baseOn = isInitiallyInstalledCap(cap.state);
                const pendingOn = addCapSet.has(cap.name);
                const pendingOff = removeCapSet.has(cap.name);
                const effectiveOn = baseOn ? !pendingOff : pendingOn;
                const changed = pendingOn || pendingOff;

                return (
                  <div
                    key={cap.name}
                    className={`px-4 py-2.5 flex items-center justify-between gap-4 ${
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
                          {cap.state}
                        </span>
                        {changed && (
                          <span
                            className={`px-1.5 py-0.5 rounded font-medium ${
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
                      onToggle={() => toggleCapability(cap.name, baseOn)}
                    />
                  </div>
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}
