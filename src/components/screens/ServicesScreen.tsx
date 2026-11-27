import {
  memo,
  useCallback,
  useDeferredValue,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ChevronDown,
  Cog,
  Loader2,
  Lock,
  RotateCcw,
  Search,
  Users,
  Zap,
} from "lucide-react";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { FluentReveal, FluentSegmentedControl } from "../FluentMotion";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import type {
  ImageCatalog,
  ServiceCategory,
  ServiceRiskLevel,
  ServicesQueueState,
  ServiceStartType,
  WindowsServiceInfo,
} from "../../types/studio";

type ServicesFilter =
  | "all"
  | "changed"
  | "safe"
  | "medium"
  | "high"
  | "critical";

type CategoryFilter = "all" | ServiceCategory;

const START_OPTIONS: readonly ServiceStartType[] = [
  "automatic",
  "delayedAuto",
  "manual",
  "disabled",
];

const START_SHORT_KEYS = {
  automatic: "screens.services.startShort.automatic",
  delayedAuto: "screens.services.startShort.delayedAuto",
  manual: "screens.services.startShort.manual",
  disabled: "screens.services.startShort.disabled",
} as const;

const START_FULL_KEYS = {
  automatic: "screens.services.startFull.automatic",
  delayedAuto: "screens.services.startFull.delayedAuto",
  manual: "screens.services.startFull.manual",
  disabled: "screens.services.startFull.disabled",
} as const;

const RISK_LABEL_KEYS = {
  safe: "screens.services.risks.safe",
  medium: "screens.services.risks.medium",
  high: "screens.services.risks.high",
  critical: "screens.services.risks.critical",
} as const;

const CATEGORY_LABEL_KEYS = {
  core: "screens.services.categories.core",
  security: "screens.services.categories.security",
  network: "screens.services.categories.network",
  updates: "screens.services.categories.updates",
  telemetry: "screens.services.categories.telemetry",
  devices: "screens.services.categories.devices",
  gaming: "screens.services.categories.gaming",
  enterprise: "screens.services.categories.enterprise",
  other: "screens.services.categories.other",
} as const;

const PROTECT_REASON_KEYS = {
  core_rpc: "screens.services.protectReasons.core_rpc",
  core_boot: "screens.services.protectReasons.core_boot",
  servicing: "screens.services.protectReasons.servicing",
  network_core: "screens.services.protectReasons.network_core",
  security_core: "screens.services.protectReasons.security_core",
  windows_update: "screens.services.protectReasons.windows_update",
  system_important: "screens.services.protectReasons.system_important",
  network_important: "screens.services.protectReasons.network_important",
  wireless: "screens.services.protectReasons.wireless",
  audio: "screens.services.protectReasons.audio",
  security_important: "screens.services.protectReasons.security_important",
  dependency_of_critical: "screens.services.protectReasons.dependency_of_critical",
} as const;

const CATEGORY_ORDER: readonly ServiceCategory[] = [
  "telemetry",
  "network",
  "security",
  "updates",
  "devices",
  "gaming",
  "enterprise",
  "core",
  "other",
];

function riskBadgeClass(risk: ServiceRiskLevel): string {
  switch (risk) {
    case "safe":
      return "bg-status-success-bg text-status-success";
    case "medium":
      return "bg-accent/10 text-accent";
    case "high":
      return "bg-status-warning-bg text-status-warning";
    case "critical":
      return "bg-status-danger-bg text-status-danger";
  }
}

interface ServiceRowProps {
  service: WindowsServiceInfo;
  effectiveStart: ServiceStartType;
  isChanged: boolean;
  isExpanded: boolean;
  isBuilding: boolean;
  onToggleExpand: (name: string) => void;
  onSelectStartType: (service: WindowsServiceInfo, target: ServiceStartType) => void;
  onRevertService: (name: string) => void;
}

const ServiceRow = memo(function ServiceRow({
  service,
  effectiveStart,
  isChanged,
  isExpanded,
  isBuilding,
  onToggleExpand,
  onSelectStartType,
  onRevertService,
}: ServiceRowProps) {
  const { t } = useTranslation();

  const reasonText =
    service.protectionReason && service.protectionReason in PROTECT_REASON_KEYS
      ? t(
          PROTECT_REASON_KEYS[
            service.protectionReason as keyof typeof PROTECT_REASON_KEYS
          ]
        )
      : null;

  return (
    <div
      className={`px-4 py-2.5 transition-colors duration-150 ${
        isChanged ? "bg-subtle" : "hover:bg-subtle"
      }`}
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <button
          type="button"
          onClick={() => onToggleExpand(service.name)}
          aria-expanded={isExpanded}
          className="flex items-start gap-2.5 min-w-0 flex-1 text-left group focus:outline-none focus-visible:ring-1 focus-visible:ring-accent/50 rounded"
        >
          <ChevronDown
            className={`w-4 h-4 text-text-secondary shrink-0 mt-0.5 transition-transform duration-200 ${
              isExpanded ? "rotate-180 text-text-primary" : ""
            }`}
            strokeWidth={1.65}
          />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="text-[13.5px] font-semibold text-text-primary truncate select-text">
                {service.displayName}
              </span>
              <span
                className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10.5px] font-medium shrink-0 ${riskBadgeClass(
                  service.riskLevel
                )}`}
                title={reasonText ?? undefined}
              >
                {service.isLockedCritical && (
                  <Lock className="w-2.5 h-2.5" strokeWidth={1.9} />
                )}
                {t(RISK_LABEL_KEYS[service.riskLevel])}
              </span>
              {service.isPerUserTemplate && (
                <span
                  className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10.5px] font-medium bg-subtle text-text-secondary shrink-0"
                  title={t("screens.services.perUserTooltip")}
                >
                  <Users className="w-2.5 h-2.5" strokeWidth={1.7} />
                  {t("screens.services.perUserBadge")}
                </span>
              )}
              {service.hasTriggerStart && (
                <span
                  className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10.5px] font-medium bg-subtle text-text-secondary shrink-0"
                  title={t("screens.services.triggerTooltip")}
                >
                  <Zap className="w-2.5 h-2.5" strokeWidth={1.7} />
                  {t("screens.services.triggerBadge")}
                </span>
              )}
              {isChanged && (
                <span className="animate-fluent-icon-pop px-1.5 py-0.5 rounded text-[10.5px] font-medium bg-accent/15 text-accent shrink-0">
                  {t(START_FULL_KEYS[service.startType])} →{" "}
                  {t(START_FULL_KEYS[effectiveStart])}
                </span>
              )}
            </div>
            <div className="text-[11.5px] font-mono text-text-secondary truncate mt-0.5 select-text">
              {service.name}
              {service.group ? ` • ${service.group}` : ""}
              {` • ${t(CATEGORY_LABEL_KEYS[service.category])}`}
            </div>
          </div>
        </button>

        <div className="flex items-center gap-1.5 shrink-0">
          <div
            role="group"
            aria-label={service.name}
            className="inline-flex items-center p-0.5 rounded-md bg-canvas border border-border-subtle"
          >
            {START_OPTIONS.map((opt) => {
              const active = effectiveStart === opt;
              const lockedDisable =
                service.isLockedCritical && opt === "disabled";
              const btnDisabled = isBuilding || lockedDisable;

              let activeClass =
                "text-text-secondary hover:text-text-primary";
              if (active) {
                if (isChanged && opt === "disabled") {
                  activeClass =
                    "bg-status-danger text-white shadow-xs font-semibold";
                } else if (isChanged) {
                  activeClass =
                    "bg-accent text-accent-fg shadow-xs font-semibold";
                } else {
                  activeClass =
                    "bg-surface text-text-primary shadow-xs font-semibold";
                }
              }

              return (
                <button
                  key={opt}
                  type="button"
                  disabled={btnDisabled}
                  aria-pressed={active}
                  title={
                    lockedDisable
                      ? t("screens.services.lockedCriticalTooltip")
                      : t(START_FULL_KEYS[opt])
                  }
                  onClick={() => onSelectStartType(service, opt)}
                  className={`px-2 py-1 rounded-[4px] text-[11px] transition-all duration-150 active:scale-[0.97] whitespace-nowrap disabled:opacity-40 disabled:cursor-not-allowed disabled:active:scale-100 ${activeClass}`}
                >
                  {t(START_SHORT_KEYS[opt])}
                </button>
              );
            })}
          </div>

          {isChanged && (
            <button
              type="button"
              disabled={isBuilding}
              onClick={() => onRevertService(service.name)}
              title={t("screens.services.revertOneBtn")}
              aria-label={t("screens.services.revertOneBtn")}
              className="p-1.5 rounded-md text-text-secondary hover:text-text-primary hover:bg-surface border border-transparent hover:border-border-subtle transition-all duration-150 active:scale-[0.94] disabled:opacity-40 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <RotateCcw className="w-3.5 h-3.5" strokeWidth={1.7} />
            </button>
          )}
        </div>
      </div>

      {/* Smooth Fluent CSS-grid accordion for service details */}
      <div
        className={`grid transition-[grid-template-rows,opacity] duration-200 ease-out ${
          isExpanded
            ? "grid-rows-[1fr] opacity-100 mt-2.5"
            : "grid-rows-[0fr] opacity-0 pointer-events-none"
        }`}
      >
        <div className="overflow-hidden">
          <div className="px-3.5 py-2.5 rounded-md bg-canvas border border-border-subtle space-y-2 text-[12px]">
            <p className="text-text-primary leading-relaxed select-text">
              {service.description || t("screens.services.noDescription")}
            </p>
            {reasonText && (
              <p
                className={`text-[11.5px] font-medium ${
                  service.isLockedCritical
                    ? "text-status-danger"
                    : "text-status-warning"
                }`}
              >
                {reasonText}
              </p>
            )}
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px] text-text-secondary">
              <span>
                {t("screens.services.originalStartLabel")}{" "}
                <strong className="font-medium text-text-primary">
                  {t(START_FULL_KEYS[service.startType])}
                </strong>
              </span>
              {service.dependsOn.length > 0 && (
                <span className="font-mono select-text">
                  {t("screens.services.dependsOnLabel")}{" "}
                  {service.dependsOn.join(", ")}
                </span>
              )}
              {service.requiredBy.length > 0 && (
                <span className="font-mono text-status-warning select-text">
                  {t("screens.services.requiredByLabel")}{" "}
                  {service.requiredBy.join(", ")}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});

interface ServicesScreenProps {
  catalog: ImageCatalog | null;
  catalogLoading: boolean;
  isBuilding?: boolean;
  servicesQueue: ServicesQueueState;
  onChangeServicesQueue: Dispatch<SetStateAction<ServicesQueueState>>;
}

export function ServicesScreen({
  catalog,
  catalogLoading,
  isBuilding = false,
  servicesQueue,
  onChangeServicesQueue,
}: ServicesScreenProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  const [filter, setFilter] = useState<ServicesFilter>("all");
  const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>("all");
  const [expandedNames, setExpandedNames] = useState<Set<string>>(
    () => new Set()
  );
  const [confirmDisable, setConfirmDisable] =
    useState<WindowsServiceInfo | null>(null);

  const handleCloseConfirm = useCallback(() => {
    setConfirmDisable(null);
  }, []);
  const { isClosing: closingConfirm, requestClose: requestCloseConfirm } =
    useAnimatedModalClose(handleCloseConfirm, {
      enabled: Boolean(confirmDisable),
    });

  const allServices = useMemo(
    () => catalog?.services ?? [],
    [catalog]
  );

  const changeMap = useMemo(() => {
    const map = new Map<string, ServiceStartType>();
    for (const ch of servicesQueue.changes) {
      map.set(ch.name.toLowerCase(), ch.startType);
    }
    return map;
  }, [servicesQueue.changes]);

  const availableCategories = useMemo(() => {
    const present = new Set(allServices.map((s) => s.category));
    return CATEGORY_ORDER.filter((c) => present.has(c));
  }, [allServices]);

  const filteredServices = useMemo(() => {
    if (allServices.length === 0) return [];
    const q = deferredSearch.trim().toLowerCase();
    return allServices.filter((svc) => {
      const changed = changeMap.has(svc.name.toLowerCase());
      if (filter === "changed" && !changed) return false;
      if (
        filter !== "all" &&
        filter !== "changed" &&
        svc.riskLevel !== filter
      ) {
        return false;
      }
      if (categoryFilter !== "all" && svc.category !== categoryFilter) {
        return false;
      }
      if (!q) return true;
      return (
        svc.displayName.toLowerCase().includes(q) ||
        svc.name.toLowerCase().includes(q) ||
        svc.description.toLowerCase().includes(q) ||
        svc.group.toLowerCase().includes(q)
      );
    });
  }, [allServices, categoryFilter, changeMap, deferredSearch, filter]);

  const applyServiceChange = useCallback(
    (svc: WindowsServiceInfo, target: ServiceStartType) => {
      const key = svc.name.toLowerCase();
      onChangeServicesQueue((prev) => {
        const filtered = prev.changes.filter(
          (c) => c.name.toLowerCase() !== key
        );
        if (target === svc.startType) {
          return { changes: filtered };
        }
        return {
          changes: [...filtered, { name: svc.name, startType: target }],
        };
      });
    },
    [onChangeServicesQueue]
  );

  const handleSelectStartType = useCallback(
    (svc: WindowsServiceInfo, target: ServiceStartType) => {
      if (isBuilding) return;
      if (svc.isLockedCritical && target === "disabled") return;
      const currentEffective =
        changeMap.get(svc.name.toLowerCase()) ?? svc.startType;
      if (currentEffective === target) return;

      if (
        target === "disabled" &&
        (svc.riskLevel === "high" || svc.requiredBy.length > 0)
      ) {
        setConfirmDisable(svc);
        return;
      }

      applyServiceChange(svc, target);
    },
    [applyServiceChange, changeMap, isBuilding]
  );

  const handleRevertService = useCallback(
    (name: string) => {
      if (isBuilding) return;
      const key = name.toLowerCase();
      onChangeServicesQueue((prev) => ({
        changes: prev.changes.filter((c) => c.name.toLowerCase() !== key),
      }));
    },
    [isBuilding, onChangeServicesQueue]
  );

  const handleToggleExpand = useCallback((name: string) => {
    setExpandedNames((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const handleResetQueue = () => {
    if (isBuilding) return;
    onChangeServicesQueue({ changes: [] });
  };

  const confirmReasonText =
    confirmDisable?.protectionReason &&
    confirmDisable.protectionReason in PROTECT_REASON_KEYS
      ? t(
          PROTECT_REASON_KEYS[
            confirmDisable.protectionReason as keyof typeof PROTECT_REASON_KEYS
          ]
        )
      : null;

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.services.title")}
        subtitle={t("screens.services.subtitle")}
        actions={
          catalog && servicesQueue.changes.length > 0 ? (
            <button
              type="button"
              disabled={isBuilding}
              onClick={handleResetQueue}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <RotateCcw className="w-3.5 h-3.5" strokeWidth={1.7} />
              <span>{t("screens.services.resetQueueBtn")}</span>
            </button>
          ) : undefined
        }
      />

      {catalogLoading && (
        <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-10 flex flex-col items-center gap-3 text-center">
          <Loader2
            className="w-6 h-6 animate-spin text-accent"
            strokeWidth={1.7}
          />
          <div className="text-[13.5px] font-medium text-text-primary">
            {t("screens.services.loading")}
          </div>
        </FluentReveal>
      )}

      {!catalogLoading && !catalog && (
        <EmptyStateCard
          icon={Cog}
          title={t("screens.services.emptyTitle")}
          description={t("screens.services.emptyDesc")}
        />
      )}

      {!catalogLoading && catalog && allServices.length === 0 && (
        <EmptyStateCard
          icon={Cog}
          title={t("screens.services.noCatalogServicesTitle")}
          description={t("screens.services.noCatalogServicesDesc")}
        />
      )}

      {!catalogLoading && catalog && allServices.length > 0 && (
        <>
          {/* Search + Risk Filter */}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="relative flex-1 min-w-[220px]">
              <Search
                className="w-4 h-4 text-text-secondary absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none"
                strokeWidth={1.6}
              />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("screens.services.searchPlaceholder")}
                className="w-full pl-9 pr-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[13px] text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent"
              />
            </div>

            <FluentSegmentedControl<ServicesFilter>
              value={filter}
              onChange={setFilter}
              variant="accent"
              size="sm"
              options={[
                { value: "all", label: t("screens.services.filterAll") },
                { value: "changed", label: t("screens.services.filterChanged") },
                { value: "safe", label: t("screens.services.risks.safe") },
                { value: "medium", label: t("screens.services.risks.medium") },
                { value: "high", label: t("screens.services.risks.high") },
                {
                  value: "critical",
                  label: t("screens.services.risks.critical"),
                },
              ]}
            />
          </div>

          {/* Category filter chips + count */}
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-1.5">
              <button
                type="button"
                onClick={() => setCategoryFilter("all")}
                className={`px-2.5 py-1 rounded-md text-[11.5px] font-medium border transition-all duration-150 active:scale-[0.98] ${
                  categoryFilter === "all"
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle"
                }`}
              >
                {t("screens.services.categoryAll")}
              </button>
              {availableCategories.map((cat) => {
                const active = categoryFilter === cat;
                return (
                  <button
                    key={cat}
                    type="button"
                    onClick={() => setCategoryFilter(cat)}
                    className={`px-2.5 py-1 rounded-md text-[11.5px] font-medium border transition-all duration-150 active:scale-[0.98] ${
                      active
                        ? "bg-accent text-accent-fg border-accent"
                        : "bg-surface border-border-subtle text-text-secondary hover:text-text-primary hover:bg-subtle"
                    }`}
                  >
                    {t(CATEGORY_LABEL_KEYS[cat])}
                  </button>
                );
              })}
            </div>

            <span className="text-[12px] text-text-secondary font-mono shrink-0">
              {t("screens.services.countSummary", {
                shown: filteredServices.length,
                total: allServices.length,
              })}
            </span>
          </div>

          {filteredServices.length === 0 ? (
            <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-8 text-center text-[13px] text-text-secondary">
              {t("screens.services.noMatches")}
            </FluentReveal>
          ) : (
            <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {filteredServices.map((svc) => {
                const overridden = changeMap.get(svc.name.toLowerCase());
                const effectiveStart = overridden ?? svc.startType;
                const isChanged =
                  overridden !== undefined && overridden !== svc.startType;
                return (
                  <ServiceRow
                    key={svc.name}
                    service={svc}
                    effectiveStart={effectiveStart}
                    isChanged={isChanged}
                    isExpanded={expandedNames.has(svc.name)}
                    isBuilding={isBuilding}
                    onToggleExpand={handleToggleExpand}
                    onSelectStartType={handleSelectStartType}
                    onRevertService={handleRevertService}
                  />
                );
              })}
            </div>
          )}
        </>
      )}

      {confirmDisable && (
        <FluentModal
          isClosing={closingConfirm}
          onRequestClose={requestCloseConfirm}
          maxWidthClass="max-w-[460px]"
          backdropClass="bg-black/50"
        >
          <div className="p-5 flex items-start gap-3.5">
            <div className="w-9 h-9 rounded-full bg-status-warning-bg text-status-warning flex items-center justify-center shrink-0">
              <AlertTriangle
                className="animate-fluent-icon-pop w-5 h-5"
                strokeWidth={1.75}
              />
            </div>
            <div className="space-y-1.5 min-w-0">
              <h3 className="text-[15px] font-semibold text-text-primary">
                {t("screens.services.disableConfirmTitle")}
              </h3>
              <p className="text-[13px] text-text-secondary leading-relaxed">
                {t("screens.services.disableConfirmDesc", {
                  name: confirmDisable.displayName,
                  serviceName: confirmDisable.name,
                })}
              </p>
              {confirmReasonText && (
                <p className="text-[12px] text-status-warning font-medium">
                  {confirmReasonText}
                </p>
              )}
              {confirmDisable.requiredBy.length > 0 && (
                <div className="rounded-md bg-status-warning-bg px-3 py-2 text-[12px] text-status-warning">
                  <div className="font-semibold">
                    {t("screens.services.requiredByWarningTitle")}
                  </div>
                  <div className="font-mono mt-0.5 break-words">
                    {confirmDisable.requiredBy.join(", ")}
                  </div>
                </div>
              )}
            </div>
          </div>
          <div className="px-5 py-3 border-t border-border-subtle bg-sidebar flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={requestCloseConfirm}
              className="px-3.5 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98]"
            >
              {t("screens.services.cancel")}
            </button>
            <button
              type="button"
              onClick={() => {
                applyServiceChange(confirmDisable, "disabled");
                requestCloseConfirm();
              }}
              className="px-3.5 py-1.5 rounded-md text-[13px] font-semibold bg-status-danger text-white hover:opacity-90 transition-all duration-150 active:scale-[0.98]"
            >
              {t("screens.services.confirmDisableAnyway")}
            </button>
          </div>
        </FluentModal>
      )}
    </div>
  );
}
