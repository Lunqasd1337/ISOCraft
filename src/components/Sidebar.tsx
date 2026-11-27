import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SectionId } from "../types/studio";
import { SidebarCategoryIcon } from "./SidebarIcons";

interface SidebarProps {
  activeSection: SectionId;
  onSelectSection: (section: SectionId) => void;
  badges?: Partial<Record<SectionId, string>>;
}

interface NavItem {
  id: SectionId;
  labelKey: `nav.${SectionId}`;
  dividerAbove?: boolean;
}

interface IndicatorRect {
  top: number;
  height: number;
}

const NAV_ITEMS: NavItem[] = [
  { id: "image", labelKey: "nav.image" },
  { id: "apps", labelKey: "nav.apps" },
  { id: "features", labelKey: "nav.features" },
  { id: "services", labelKey: "nav.services" },
  { id: "updates", labelKey: "nav.updates" },
  { id: "drivers", labelKey: "nav.drivers" },
  { id: "registry", labelKey: "nav.registry" },
  { id: "files", labelKey: "nav.files" },
  { id: "postSetup", labelKey: "nav.postSetup" },
  { id: "build", labelKey: "nav.build", dividerAbove: true },
];

export function Sidebar({ activeSection, onSelectSection, badges = {} }: SidebarProps) {
  const { t, i18n } = useTranslation();
  const [iconTicks, setIconTicks] = useState<Partial<Record<SectionId, number>>>({});

  const navRef = useRef<HTMLElement | null>(null);
  const buttonRefs = useRef<Partial<Record<SectionId, HTMLButtonElement | null>>>({});

  const [activeRect, setActiveRect] = useState<IndicatorRect | null>(null);
  const [activeAnimated, setActiveAnimated] = useState(false);
  const prevActiveSectionRef = useRef<SectionId>(activeSection);

  const [hoverRect, setHoverRect] = useState<IndicatorRect>({ top: 0, height: 0 });
  const [hoverVisible, setHoverVisible] = useState(false);
  const [hoverCanSlide, setHoverCanSlide] = useState(false);
  const [hoveredId, setHoveredId] = useState<SectionId | null>(null);
  const hoverVisibleRef = useRef(false);
  const hoveredIdRef = useRef<SectionId | null>(null);
  const pointerInsideRef = useRef(false);
  const lastIconAnimAtRef = useRef<Partial<Record<SectionId, number>>>({});

  const triggerIconAnimation = useCallback((id: SectionId) => {
    const now = performance.now();
    const prev = lastIconAnimAtRef.current[id] ?? 0;
    if (now - prev < 420) return;
    lastIconAnimAtRef.current[id] = now;
    setIconTicks((prevTicks) => ({
      ...prevTicks,
      [id]: (prevTicks[id] ?? 0) + 1,
    }));
  }, []);

  const measureItem = useCallback((id: SectionId): IndicatorRect | null => {
    const navEl = navRef.current;
    const btnEl = buttonRefs.current[id];
    if (!navEl || !btnEl) return null;
    const navRect = navEl.getBoundingClientRect();
    const btnRect = btnEl.getBoundingClientRect();
    return {
      top: btnRect.top - navRect.top,
      height: btnRect.height,
    };
  }, []);

  useLayoutEffect(() => {
    const rect = measureItem(activeSection);
    if (prevActiveSectionRef.current !== activeSection) {
      prevActiveSectionRef.current = activeSection;
      setActiveAnimated(true);
    }
    if (rect) {
      setActiveRect(rect);
    }
  }, [activeSection, i18n.language, measureItem]);

  useEffect(() => {
    const navEl = navRef.current;
    if (!navEl || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(() => {
      const nextActive = measureItem(activeSection);
      if (nextActive) {
        setActiveRect((prev) =>
          prev && prev.top === nextActive.top && prev.height === nextActive.height
            ? prev
            : nextActive
        );
      }
      if (hoverVisibleRef.current && hoveredIdRef.current) {
        const nextHover = measureItem(hoveredIdRef.current);
        if (nextHover) {
          setHoverRect((prev) =>
            prev.top === nextHover.top && prev.height === nextHover.height ? prev : nextHover
          );
        }
      }
    });

    observer.observe(navEl);
    return () => observer.disconnect();
  }, [activeSection, measureItem]);

  const showHoverFor = useCallback(
    (id: SectionId) => {
      const rect = measureItem(id);
      if (!rect) return;
      const changedTarget = hoveredIdRef.current !== id;
      hoveredIdRef.current = id;
      setHoveredId(id);
      if (changedTarget) {
        triggerIconAnimation(id);
      }
      if (hoverVisibleRef.current) {
        setHoverCanSlide(true);
      } else {
        // Snap onto the hovered item on first entry so it fades in place instead of flying in
        setHoverCanSlide(false);
        hoverVisibleRef.current = true;
        setHoverVisible(true);
      }
      setHoverRect(rect);
    },
    [measureItem, triggerIconAnimation]
  );

  const hideHover = useCallback(() => {
    hoveredIdRef.current = null;
    setHoveredId(null);
    hoverVisibleRef.current = false;
    setHoverVisible(false);
    setHoverCanSlide(false);
  }, []);

  const handleItemClick = (id: SectionId) => {
    triggerIconAnimation(id);
    onSelectSection(id);
  };

  return (
    <aside className="w-[220px] shrink-0 bg-sidebar border-r border-border-subtle flex flex-col py-3 px-2.5 select-none">
      <nav
        ref={navRef}
        onMouseEnter={() => {
          pointerInsideRef.current = true;
        }}
        onMouseLeave={() => {
          pointerInsideRef.current = false;
          hideHover();
        }}
        onBlur={(e) => {
          if (
            !pointerInsideRef.current &&
            !navRef.current?.contains(e.relatedTarget as Node | null)
          ) {
            hideHover();
          }
        }}
        className="relative flex flex-col gap-1 flex-1"
        aria-label={t("nav.ariaLabel")}
      >
        {/* Sliding hover highlight pill */}
        <div
          aria-hidden="true"
          className={`pointer-events-none absolute inset-x-0 top-0 z-0 rounded-[6px] bg-subtle ${
            hoverCanSlide ? "sidebar-hover-pill-sliding" : "sidebar-hover-pill"
          }`}
          style={{
            transform: `translate3d(0, ${hoverRect.top}px, 0)`,
            height: `${hoverRect.height}px`,
            opacity: hoverVisible ? 1 : 0,
          }}
        />

        {/* Sliding active card + left Fluent accent indicator */}
        {activeRect && (
          <div
            aria-hidden="true"
            className={`pointer-events-none absolute inset-x-0 top-0 z-[1] rounded-[6px] bg-surface shadow-xs border border-border-subtle ${
              activeAnimated ? "sidebar-active-pill-animated" : "sidebar-active-pill"
            }`}
            style={{
              transform: `translate3d(0, ${activeRect.top}px, 0)`,
              height: `${activeRect.height}px`,
            }}
          >
            <span className="absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-[18px] rounded-full bg-accent" />
          </div>
        )}

        {NAV_ITEMS.map((item) => {
          const active = activeSection === item.id;
          const badge = badges[item.id];
          const tick = iconTicks[item.id] ?? 0;

          return (
            <div key={item.id}>
              {item.dividerAbove && (
                <div className="my-2 border-t border-border-subtle" />
              )}
              <button
                ref={(el) => {
                  buttonRefs.current[item.id] = el;
                }}
                type="button"
                onClick={() => handleItemClick(item.id)}
                onMouseEnter={() => showHoverFor(item.id)}
                onFocus={(e) => {
                  if (e.currentTarget.matches(":focus-visible")) {
                    showHoverFor(item.id);
                  }
                }}
                className={`group relative z-10 w-full flex items-center justify-between gap-2.5 px-3 py-2.5 rounded-[6px] text-[14px] border border-transparent transition-colors duration-150 focus:outline-none focus-visible:ring-1 focus-visible:ring-accent/50 ${
                  active
                    ? "text-text-primary font-semibold"
                    : "text-text-secondary hover:text-text-primary font-normal"
                }`}
              >
                <span className="flex items-center gap-3 min-w-0">
                  <SidebarCategoryIcon
                    id={item.id}
                    isHovered={hoveredId === item.id}
                    playTick={tick}
                    className={`w-[18px] h-[18px] shrink-0 transition-colors duration-150 ${
                      active ? "text-accent" : "text-text-secondary group-hover:text-text-primary"
                    }`}
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
