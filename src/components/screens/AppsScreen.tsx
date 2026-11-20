import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  LayoutGrid,
  Loader2,
  PackagePlus,
  Plus,
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

interface AppsScreenProps {
  catalog: ImageCatalog | null;
  catalogLoading: boolean;
  appsQueue: AppsQueueState;
  onToggleRemoveFamily: (family: string) => void;
  onAddPackage: (req: AddAppxRequest) => void;
  onRemoveAddedPackage: (packagePath: string) => void;
}

export function AppsScreen({
  catalog,
  catalogLoading,
  appsQueue,
  onToggleRemoveFamily,
  onAddPackage,
  onRemoveAddedPackage,
}: AppsScreenProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<AppsFilter>("all");
  const [isAddOpen, setIsAddOpen] = useState(false);
  const [confirmProtected, setConfirmProtected] = useState<ProvisionedAppxInfo | null>(
    null
  );

  const removeSet = useMemo(
    () => new Set(appsQueue.removeFamilies),
    [appsQueue.removeFamilies]
  );

  const filteredProvisioned = useMemo(() => {
    if (!catalog) return [];
    const q = search.trim().toLowerCase();
    return catalog.appxPackages.filter((pkg) => {
      if (filter === "added") return false;
      if (filter === "remove" && !removeSet.has(pkg.packageFamilyName)) return false;
      if (filter === "protected" && !pkg.isProtected) return false;
      if (!q) return true;
      return (
        pkg.displayName.toLowerCase().includes(q) ||
        pkg.packageName.toLowerCase().includes(q) ||
        pkg.packageFamilyName.toLowerCase().includes(q)
      );
    });
  }, [catalog, filter, removeSet, search]);

  const filteredAdded = useMemo(() => {
    if (filter === "remove" || filter === "protected") return [];
    const q = search.trim().toLowerCase();
    return appsQueue.addPackages.filter((pkg) => {
      if (!q) return true;
      return pkg.packagePath.toLowerCase().includes(q);
    });
  }, [appsQueue.addPackages, filter, search]);

  const handleToggleClick = (pkg: ProvisionedAppxInfo) => {
    const currentlyMarked = removeSet.has(pkg.packageFamilyName);
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
          <button
            type="button"
            disabled={!catalog && !catalogLoading}
            onClick={() => setIsAddOpen(true)}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Plus className="w-4 h-4" strokeWidth={1.7} />
            <span>{t("screens.apps.addBtn")}</span>
          </button>
        }
      />

      {catalogLoading && (
        <div className="bg-surface border border-border-subtle rounded-lg px-6 py-10 flex flex-col items-center gap-3 text-center">
          <Loader2 className="w-6 h-6 animate-spin text-accent" strokeWidth={1.7} />
          <div className="text-[13.5px] font-medium text-text-primary">
            {t("screens.apps.loading")}
          </div>
        </div>
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
                className="w-full pl-9 pr-3 py-1.5 rounded-md bg-surface border border-border-subtle text-[13px] text-text-primary focus:outline-none focus:border-accent"
              />
            </div>

            <div className="flex items-center gap-1 rounded-md border border-border-subtle bg-surface p-0.5 text-[12px] font-medium">
              {(
                [
                  ["all", t("screens.apps.filterAll")],
                  ["remove", t("screens.apps.filterRemove")],
                  ["added", t("screens.apps.filterAdded")],
                  ["protected", t("screens.apps.filterProtected")],
                ] as [AppsFilter, string][]
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

          {/* Queued Added Appx packages */}
          {filteredAdded.length > 0 && (
            <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
              {filteredAdded.map((item) => {
                const base = item.packagePath.split(/[\\/]/).pop() || item.packagePath;
                return (
                  <div
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
                          {item.packagePath} • deps: {item.dependencyPaths.length} •{" "}
                          {item.skipLicense ? "SkipLicense" : "LicenseXML"}
                        </div>
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => onRemoveAddedPackage(item.packagePath)}
                      className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[12px] font-medium bg-subtle hover:bg-status-danger-bg hover:text-status-danger text-text-secondary transition-colors shrink-0"
                    >
                      <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                      {t("screens.apps.removeFromQueue")}
                    </button>
                  </div>
                );
              })}
            </div>
          )}

          {/* Provisioned packages list */}
          {filteredProvisioned.length === 0 && filteredAdded.length === 0 ? (
            <div className="bg-surface border border-border-subtle rounded-lg px-6 py-8 text-center text-[13px] text-text-secondary">
              {t("screens.apps.noMatches")}
            </div>
          ) : (
            filteredProvisioned.length > 0 && (
              <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
                {filteredProvisioned.map((pkg) => {
                  const markedForRemoval = removeSet.has(pkg.packageFamilyName);
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

                      <button
                        type="button"
                        onClick={() => handleToggleClick(pkg)}
                        className={`px-3 py-1.5 rounded-md text-[12px] font-semibold border transition-colors shrink-0 ${
                          markedForRemoval
                            ? "bg-surface border-border-subtle text-text-primary hover:bg-subtle"
                            : "bg-surface border-border-subtle text-status-danger hover:bg-status-danger-bg"
                        }`}
                      >
                        {markedForRemoval
                          ? t("screens.apps.keepAction")
                          : t("screens.apps.removeAction")}
                      </button>
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
        <div className="animate-fluent-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="animate-fluent-modal w-full max-w-[440px] bg-surface border border-border-subtle rounded-lg shadow-xl overflow-hidden">
            <div className="p-5 flex items-start gap-3.5">
              <div className="w-9 h-9 rounded-full bg-status-warning-bg text-status-warning flex items-center justify-center shrink-0">
                <AlertTriangle className="w-5 h-5" strokeWidth={1.75} />
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
                {confirmProtected.protectionReason && (
                  <p className="text-[12px] text-status-warning font-medium">
                    {t(
                      `screens.apps.protectReasons.${confirmProtected.protectionReason}`
                    )}
                  </p>
                )}
              </div>
            </div>
            <div className="px-5 py-3 border-t border-border-subtle bg-sidebar flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => setConfirmProtected(null)}
                className="px-3.5 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle"
              >
                {t("screens.apps.cancel")}
              </button>
              <button
                type="button"
                onClick={() => {
                  onToggleRemoveFamily(confirmProtected.packageFamilyName);
                  setConfirmProtected(null);
                }}
                className="px-3.5 py-1.5 rounded-md text-[13px] font-semibold bg-status-danger text-white hover:opacity-90"
              >
                {t("screens.apps.confirmRemoveAnyway")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
