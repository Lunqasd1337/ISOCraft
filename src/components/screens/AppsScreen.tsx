import { useCallback, useDeferredValue, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { FluentReveal, FluentSegmentedControl } from "../FluentMotion";
import {
  AlertTriangle,
  LayoutGrid,
  Loader2,
  PackagePlus,
  Plus,
  RotateCcw,
  Search,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import { AddAppxModal } from "./AddAppxModal";
import type {
  AddAppxRequest,
  AppsQueueState,
  ImageCatalog,
  ProvisionedAppxInfo,
} from "../../types/studio";

type AppsFilter = "all" | "remove" | "added" | "protected";

const PROTECT_REASON_KEYS = {
  store: "screens.apps.protectReasons.store",
  store_purchase: "screens.apps.protectReasons.store_purchase",
  app_installer: "screens.apps.protectReasons.app_installer",
  windows_security: "screens.apps.protectReasons.windows_security",
  webview: "screens.apps.protectReasons.webview",
  ui_xaml: "screens.apps.protectReasons.ui_xaml",
  vclibs: "screens.apps.protectReasons.vclibs",
  net_native: "screens.apps.protectReasons.net_native",
  windows_app_runtime: "screens.apps.protectReasons.windows_app_runtime",
} as const;

interface AppsScreenProps {
  catalog: ImageCatalog | null;
  catalogLoading: boolean;
  isBuilding?: boolean;
  appsQueue: AppsQueueState;
  onToggleRemoveFamily: (family: string) => void;
  onBulkRemoveFamilies: (families: string[]) => void;
  onResetQueue: () => void;
  onAddPackage: (req: AddAppxRequest) => void;
  onRemoveAddedPackage: (packagePath: string) => void;
}

export function AppsScreen({
  catalog,
  catalogLoading,
  isBuilding = false,
  appsQueue,
  onToggleRemoveFamily,
  onBulkRemoveFamilies,
  onResetQueue,
  onAddPackage,
  onRemoveAddedPackage,
}: AppsScreenProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  const [filter, setFilter] = useState<AppsFilter>("all");
  const [isAddOpen, setIsAddOpen] = useState(false);
  const [confirmProtected, setConfirmProtected] = useState<ProvisionedAppxInfo | null>(
    null
  );
  const handleCloseConfirm = useCallback(() => {
    setConfirmProtected(null);
  }, []);
  const { isClosing: closingConfirm, requestClose: requestCloseConfirm } =
    useAnimatedModalClose(handleCloseConfirm, {
      enabled: Boolean(confirmProtected),
    });

  const removeSet = useMemo(
    () => new Set(appsQueue.removeFamilies.map((f) => f.toLowerCase())),
    [appsQueue.removeFamilies]
  );

  const filteredProvisioned = useMemo(() => {
    if (!catalog) return [];
    const q = deferredSearch.trim().toLowerCase();
    return catalog.appxPackages.filter((pkg) => {
      if (filter === "added") return false;
      if (filter === "remove" && !removeSet.has(pkg.packageFamilyName.toLowerCase())) {
        return false;
      }
      if (filter === "protected" && !pkg.isProtected) return false;
      if (!q) return true;
      return (
        pkg.displayName.toLowerCase().includes(q) ||
        pkg.packageName.toLowerCase().includes(q) ||
        pkg.packageFamilyName.toLowerCase().includes(q)
      );
    });
  }, [catalog, deferredSearch, filter, removeSet]);

  const filteredAdded = useMemo(() => {
    if (filter === "remove" || filter === "protected") return [];
    const q = deferredSearch.trim().toLowerCase();
    return appsQueue.addPackages.filter((pkg) => {
      if (!q) return true;
      return pkg.packagePath.toLowerCase().includes(q);
    });
  }, [appsQueue.addPackages, deferredSearch, filter]);

  const bulkRemovableFamilies = useMemo(() => {
    return filteredProvisioned
      .filter(
        (pkg) => !pkg.isProtected && !removeSet.has(pkg.packageFamilyName.toLowerCase())
      )
      .map((pkg) => pkg.packageFamilyName);
  }, [filteredProvisioned, removeSet]);

  const hasQueuedChanges =
    appsQueue.removeFamilies.length > 0 || appsQueue.addPackages.length > 0;

  const handleToggleClick = (pkg: ProvisionedAppxInfo) => {
    if (isBuilding) return;
    const currentlyMarked = removeSet.has(pkg.packageFamilyName.toLowerCase());
    if (!currentlyMarked && pkg.isProtected) {
      setConfirmProtected(pkg);
      return;
    }
    onToggleRemoveFamily(pkg.packageFamilyName);
  };

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.apps.title")}
        subtitle={t("screens.apps.subtitle")}
        actions={
          <div className="flex flex-wrap items-center gap-2">
            {catalog && hasQueuedChanges && (
              <button
                type="button"
                disabled={isBuilding}
                onClick={onResetQueue}
                className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-secondary hover:text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
              >
                <RotateCcw className="w-3.5 h-3.5" strokeWidth={1.7} />
                <span>{t("screens.apps.resetQueueBtn")}</span>
              </button>
            )}
            {catalog && bulkRemovableFamilies.length > 0 && (
              <button
                type="button"
                disabled={isBuilding}
                onClick={() => onBulkRemoveFamilies(bulkRemovableFamilies)}
                className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
              >
                <Trash2 className="w-3.5 h-3.5" strokeWidth={1.65} />
                <span>
                  {t("screens.apps.removeFilteredBtn", {
                    count: bulkRemovableFamilies.length,
                  })}
                </span>
              </button>
            )}
            <button
              type="button"
              disabled={!catalog || catalogLoading || isBuilding}
              onClick={() => setIsAddOpen(true)}
              className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100"
            >
              <Plus className="w-4 h-4" strokeWidth={1.7} />
              <span>{t("screens.apps.addBtn")}</span>
            </button>
          </div>
        }
      />

      {catalogLoading && (
        <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-10 flex flex-col items-center gap-3 text-center">
          <Loader2 className="w-6 h-6 animate-spin text-accent" strokeWidth={1.7} />
          <div className="text-[13.5px] font-medium text-text-primary">
            {t("screens.apps.loading")}
          </div>
        </FluentReveal>
      )}

      {!catalogLoading && !catalog && (
        <EmptyStateCard
          icon={LayoutGrid}
          title={t("screens.apps.emptyTitle")}
          description={t("screens.apps.emptyDesc")}
        />
      )}

      {!catalogLoading && catalog && (
        <>
          {/* Search + Filter bar */}
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
                placeholder={t("screens.apps.searchPlaceholder")}
                className="w-full pl-9 pr-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[13px] text-text-primary transition-colors duration-150 focus:outline-none focus:border-accent"
              />
            </div>

            <FluentSegmentedControl
              value={filter}
              onChange={setFilter}
              variant="accent"
              size="sm"
              options={[
                { value: "all", label: t("screens.apps.filterAll") },
                { value: "remove", label: t("screens.apps.filterRemove") },
                { value: "added", label: t("screens.apps.filterAdded") },
                { value: "protected", label: t("screens.apps.filterProtected") },
              ]}
            />
          </div>

          {/* Queued Added Appx packages */}
          {filteredAdded.length > 0 && (
            <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {filteredAdded.map((item) => {
                const base = item.packagePath.split(/[\\/]/).pop() || item.packagePath;
                return (
                  <FluentReveal
                    key={item.packagePath}
                    className="px-4 py-3 flex items-center justify-between gap-4"
                  >
                    <div className="flex items-center gap-3 min-w-0">
                      <PackagePlus
                        className="w-4 h-4 text-status-success shrink-0"
                        strokeWidth={1.7}
                      />
                      <div className="min-w-0">
                        <div className="text-[13.5px] font-semibold text-text-primary truncate">
                          {base}
                        </div>
                        <div className="text-[11.5px] font-mono text-text-secondary truncate">
                          {item.packagePath} •{" "}
                          {t("screens.apps.addedMeta", {
                            count: item.dependencyPaths.length,
                            license: item.skipLicense
                              ? t("screens.apps.licenseSkipShort")
                              : t("screens.apps.licenseXmlShort"),
                          })}
                        </div>
                      </div>
                    </div>
                    <button
                      type="button"
                      disabled={isBuilding}
                      onClick={() => onRemoveAddedPackage(item.packagePath)}
                      className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100 shrink-0"
                    >
                      <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                      {t("screens.apps.removeFromQueue")}
                    </button>
                  </FluentReveal>
                );
              })}
            </div>
          )}

          {/* Provisioned packages list */}
          {filteredProvisioned.length === 0 && filteredAdded.length === 0 ? (
            <FluentReveal className="bg-surface border border-border-subtle rounded-lg px-6 py-8 text-center text-[13px] text-text-secondary">
              {t("screens.apps.noMatches")}
            </FluentReveal>
          ) : (
            filteredProvisioned.length > 0 && (
              <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
                {filteredProvisioned.map((pkg) => {
                  const markedForRemoval = removeSet.has(pkg.packageFamilyName.toLowerCase());
                  return (
                    <div
                      key={pkg.packageName}
                      className={`px-4 py-2.5 flex items-center justify-between gap-4 transition-colors ${
                        markedForRemoval ? "bg-status-danger-bg" : "hover:bg-subtle"
                      }`}
                    >
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <span
                            className={`text-[13.5px] font-semibold truncate ${
                              markedForRemoval
                                ? "line-through text-text-secondary"
                                : "text-text-primary"
                            }`}
                          >
                            {pkg.displayName}
                          </span>
                          {pkg.isProtected && (
                            <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-warning-bg text-status-warning shrink-0">
                              <ShieldAlert className="w-3 h-3" strokeWidth={1.75} />
                              {t("screens.apps.protectedBadge")}
                            </span>
                          )}
                        </div>
                        <div className="text-[11.5px] font-mono text-text-secondary truncate mt-0.5">
                          {pkg.packageName} • v{pkg.version}
                        </div>
                      </div>

                      <div className="flex items-center gap-1.5 shrink-0">
                        <button
                          type="button"
                          disabled={isBuilding}
                          onClick={() => handleToggleClick(pkg)}
                          className={`px-3 py-1.5 rounded-md text-[12px] font-semibold border transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100 shrink-0 ${
                            markedForRemoval
                              ? "bg-surface border-border-subtle text-text-primary hover:bg-subtle"
                              : "bg-surface border-border-subtle text-status-danger hover:bg-status-danger-bg"
                          }`}
                        >
                          {markedForRemoval
                            ? t("screens.apps.keepAction")
                            : t("screens.apps.removeAction")}
                        </button>
                        {markedForRemoval && (
                          <button
                            type="button"
                            disabled={isBuilding}
                            onClick={() => onToggleRemoveFamily(pkg.packageFamilyName)}
                            title={t("screens.apps.revertOneBtn")}
                            aria-label={t("screens.apps.revertOneBtn")}
                            className="p-1.5 rounded-md text-text-secondary hover:text-text-primary hover:bg-surface border border-transparent hover:border-border-subtle transition-all duration-150 active:scale-[0.94] disabled:opacity-40 disabled:cursor-not-allowed disabled:active:scale-100"
                          >
                            <RotateCcw className="w-3.5 h-3.5" strokeWidth={1.7} />
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )
          )}
        </>
      )}

      {isAddOpen && (
        <AddAppxModal onAdd={onAddPackage} onClose={() => setIsAddOpen(false)} />
      )}

      {confirmProtected && (
        <FluentModal
          isClosing={closingConfirm}
          onRequestClose={requestCloseConfirm}
          maxWidthClass="max-w-[440px]"
          backdropClass="bg-black/50"
        >
          <div className="p-5 flex items-start gap-3.5">
                <div className="w-9 h-9 rounded-full bg-status-warning-bg text-status-warning flex items-center justify-center shrink-0">
                  <AlertTriangle className="animate-fluent-icon-pop w-5 h-5" strokeWidth={1.75} />
                </div>
                <div className="space-y-1.5">
                  <h3 className="text-[15px] font-semibold text-text-primary">
                    {t("screens.apps.protectConfirmTitle")}
                  </h3>
                  <p className="text-[13px] text-text-secondary leading-relaxed">
                    {t("screens.apps.protectConfirmDesc", {
                      name: confirmProtected.displayName,
                    })}
                  </p>
                  {confirmProtected.protectionReason &&
                    confirmProtected.protectionReason in PROTECT_REASON_KEYS && (
                      <p className="text-[12px] text-status-warning font-medium">
                        {t(
                          PROTECT_REASON_KEYS[
                            confirmProtected.protectionReason as keyof typeof PROTECT_REASON_KEYS
                          ]
                        )}
                      </p>
                    )}
                </div>
              </div>
              <div className="px-5 py-3 border-t border-border-subtle bg-sidebar flex items-center justify-end gap-2">
                <button
                  type="button"
                  onClick={requestCloseConfirm}
                  className="px-3.5 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98]"
                >
                  {t("screens.apps.cancel")}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    onToggleRemoveFamily(confirmProtected.packageFamilyName);
                    requestCloseConfirm();
                  }}
                  className="px-3.5 py-1.5 rounded-md text-[13px] font-semibold bg-status-danger text-white hover:opacity-90 transition-all duration-150 active:scale-[0.98]"
                >
                  {t("screens.apps.confirmRemoveAnyway")}
                </button>
              </div>
        </FluentModal>
      )}
    </div>
  );
}
