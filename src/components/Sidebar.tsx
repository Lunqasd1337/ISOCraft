import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Disc,
  LayoutGrid,
  SlidersHorizontal,
  RefreshCw,
  Cpu,
  FileCode2,
  FolderPlus,
  PackageCheck,
} from "lucide-react";
import type { SectionId } from "../types/studio";

interface SidebarProps {
  activeSection: SectionId;
  onSelectSection: (section: SectionId) => void;
  badges?: Partial<Record<SectionId, string>>;
}

interface NavItem {
  id: SectionId;
  labelKey: string;
  icon: typeof Disc;
  dividerAbove?: boolean;
}

const NAV_ITEMS: NavItem[] = [
  { id: "image", labelKey: "nav.image", icon: Disc },
  { id: "apps", labelKey: "nav.apps", icon: LayoutGrid },
  { id: "features", labelKey: "nav.features", icon: SlidersHorizontal },
  { id: "updates", labelKey: "nav.updates", icon: RefreshCw },
  { id: "drivers", labelKey: "nav.drivers", icon: Cpu },
  { id: "registry", labelKey: "nav.registry", icon: FileCode2 },
  { id: "files", labelKey: "nav.files", icon: FolderPlus },
  { id: "build", labelKey: "nav.build", icon: PackageCheck, dividerAbove: true },
];

export function Sidebar({ activeSection, onSelectSection, badges = {} }: SidebarProps) {
  const { t } = useTranslation();
  const [iconTicks, setIconTicks] = useState<Partial<Record<SectionId, number>>>({});

  const handleItemClick = (id: SectionId) => {
    setIconTicks((prev) => ({
      ...prev,
      [id]: (prev[id] ?? 0) + 1,
    }));
    onSelectSection(id);
  };

  return (
    <aside className="w-[220px] shrink-0 bg-sidebar border-r border-border-subtle flex flex-col py-3 px-2.5 select-none">
      <nav className="flex flex-col gap-1 flex-1" aria-label="Main navigation">
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          const active = activeSection === item.id;
          const badge = badges[item.id];
          const tick = iconTicks[item.id] ?? 0;

          return (
            <div key={item.id}>
              {item.dividerAbove && (
                <div className="my-2 border-t border-border-subtle" />
              )}
              <button
                type="button"
                onClick={() => handleItemClick(item.id)}
                className={`group relative w-full flex items-center justify-between gap-2.5 px-3 py-2 rounded-[6px] text-[13.5px] transition-all duration-150 ${
                  active
                    ? "bg-surface text-text-primary font-semibold shadow-sm border border-border-subtle"
                    : "text-text-secondary hover:text-text-primary hover:bg-subtle font-normal border border-transparent"
                }`}
              >
                {active && (
                  <span
                    className="animate-fluent-pill absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-[16px] rounded-full bg-accent"
                    aria-hidden="true"
                  />
                )}
                <span className="flex items-center gap-2.5 min-w-0">
                  <Icon
                    key={tick > 0 ? `${item.id}-${tick}` : item.id}
                    className={`w-4 h-4 shrink-0 transition-transform duration-150 group-hover:scale-105 group-active:scale-90 ${
                      tick > 0
                        ? item.id === "image" || item.id === "updates"
                          ? "animate-fluent-icon-spin"
                          : "animate-fluent-icon-pop"
                        : ""
                    } ${active ? "text-accent" : "text-text-secondary"}`}
                    strokeWidth={1.6}
                  />
                  <span className="truncate">{t(item.labelKey)}</span>
                </span>
                {badge && (
                  <span
                    className={`px-1.5 py-0.5 text-[11px] font-medium rounded shrink-0 transition-all duration-150 ${
                      item.id === "build" && !active
                        ? "bg-accent text-accent-fg font-semibold"
                        : item.id === "build" && active
                        ? "bg-accent/15 text-accent font-semibold"
                        : "bg-subtle text-text-secondary"
                    }`}
                  >
                    {badge}
                  </span>
                )}
              </button>
            </div>
          );
        })}
      </nav>
    </aside>
  );
}
