import type { SectionId } from "../types/studio";

interface SidebarCategoryIconProps {
  id: SectionId;
  isHovered: boolean;
  playTick: number;
  className?: string;
}

const GRID_TILES = [
  { x: 3, y: 3, delay: "0ms" },
  { x: 14, y: 3, delay: "55ms" },
  { x: 14, y: 14, delay: "110ms" },
  { x: 3, y: 14, delay: "165ms" },
] as const;

export function SidebarCategoryIcon({
  id,
  isHovered,
  playTick,
  className = "",
}: SidebarCategoryIconProps) {
  const animKey = playTick > 0 ? playTick : 0;
  const active = playTick > 0;
  const svgClass = `overflow-visible ${className}`.trim();

  switch (id) {
    case "image":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey} className={active ? "animate-sidebar-spin" : undefined}>
            <circle cx="12" cy="12" r="10" />
            <circle cx="12" cy="12" r="2" />
            <path d="M6 12c0-1.7.7-3.2 1.8-4.2" opacity={0.7} />
            <path d="M18 12c0 1.7-.7 3.2-1.8 4.2" opacity={0.7} />
          </g>
        </svg>
      );

    case "apps":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey}>
            {GRID_TILES.map((tile) => (
              <rect
                key={`${tile.x}-${tile.y}`}
                width="7"
                height="7"
                x={tile.x}
                y={tile.y}
                rx="1"
                className={active ? "animate-sidebar-grid-tile" : undefined}
                style={active ? { animationDelay: tile.delay } : undefined}
              />
            ))}
          </g>
        </svg>
      );

    case "features":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <line x1="21" x2="3" y1="4" y2="4" />
          <line x1="21" x2="3" y1="12" y2="12" />
          <line x1="21" x2="3" y1="20" y2="20" />
          <line
            x1="15"
            x2="15"
            y1="2"
            y2="6"
            strokeWidth={2}
            className="sidebar-slider-knob"
            style={{ transform: isHovered ? "translateX(-6px)" : "translateX(0px)" }}
          />
          <line
            x1="9"
            x2="9"
            y1="10"
            y2="14"
            strokeWidth={2}
            className="sidebar-slider-knob"
            style={{ transform: isHovered ? "translateX(6px)" : "translateX(0px)" }}
          />
          <line
            x1="15"
            x2="15"
            y1="18"
            y2="22"
            strokeWidth={2}
            className="sidebar-slider-knob"
            style={{ transform: isHovered ? "translateX(-6px)" : "translateX(0px)" }}
          />
        </svg>
      );

    case "services":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey} className={active ? "animate-sidebar-gear" : undefined}>
            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
            <circle cx="12" cy="12" r="3" />
          </g>
        </svg>
      );

    case "updates":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey} className={active ? "animate-sidebar-spin" : undefined}>
            <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
            <path d="M21 3v5h-5" />
            <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
            <path d="M8 16H3v5" />
          </g>
        </svg>
      );

    case "drivers":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey}>
            <rect width="16" height="16" x="4" y="4" rx="2" />
            <rect
              width="6"
              height="6"
              x="9"
              y="9"
              rx="1"
              className={active ? "animate-sidebar-cpu-core" : undefined}
            />
            <g className={active ? "animate-sidebar-pin-up" : undefined}>
              <path d="M9 2v2" />
              <path d="M15 2v2" />
            </g>
            <g className={active ? "animate-sidebar-pin-down" : undefined}>
              <path d="M9 20v2" />
              <path d="M15 20v2" />
            </g>
            <g className={active ? "animate-sidebar-pin-left" : undefined}>
              <path d="M2 9h2" />
              <path d="M2 15h2" />
            </g>
            <g className={active ? "animate-sidebar-pin-right" : undefined}>
              <path d="M20 9h2" />
              <path d="M20 15h2" />
            </g>
          </g>
        </svg>
      );

    case "registry":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey}>
            <path d="M4 22h14a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v4" />
            <path d="M14 2v4a2 2 0 0 0 2 2h4" />
            <path
              d="m5 12-3 3 3 3"
              className={active ? "animate-sidebar-bracket-left" : undefined}
            />
            <path
              d="m9 18 3-3-3-3"
              className={active ? "animate-sidebar-bracket-right" : undefined}
            />
          </g>
        </svg>
      );

    case "files":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey} className={active ? "animate-sidebar-hop" : undefined}>
            <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
            <g className={active ? "animate-sidebar-plus-turn" : undefined}>
              <path d="M12 10v6" />
              <path d="M9 13h6" />
            </g>
          </g>
        </svg>
      );

    case "postSetup":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey}>
            <rect width="18" height="16" x="3" y="4" rx="2.5" />
            <polyline
              points="7 10 10.5 13 7 16"
              className={active ? "animate-sidebar-terminal-chevron" : undefined}
            />
            <line
              x1="12"
              x2="17"
              y1="16"
              y2="16"
              className={active ? "animate-sidebar-terminal-cursor" : undefined}
            />
          </g>
        </svg>
      );

    case "build":
      return (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.65}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          className={svgClass}
        >
          <g key={animKey}>
            <g className={active ? "animate-sidebar-hop" : undefined}>
              <path d="M21 10V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l2-1.14" />
              <path d="m7.5 4.27 9 5.15" />
              <polyline points="3.29 7 12 12 20.71 7" />
              <line x1="12" x2="12" y1="22" y2="12" />
            </g>
            <path
              d="m16 16 2 2 4-4"
              pathLength={1}
              className={active ? "animate-sidebar-check-draw" : undefined}
            />
          </g>
        </svg>
      );
  }
}
