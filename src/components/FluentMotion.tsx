import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithRef,
  type MutableRefObject,
  type ReactNode,
} from "react";

const FALLBACK_READY_REF: MutableRefObject<boolean> = { current: true };
const ScreenRevealReadyContext =
  createContext<MutableRefObject<boolean>>(FALLBACK_READY_REF);

interface ScreenTransitionProps {
  children: ReactNode;
  className?: string;
}

export function ScreenTransition({
  children,
  className = "",
}: ScreenTransitionProps) {
  const readyRef = useRef(false);

  useEffect(() => {
    readyRef.current = true;
    return () => {
      readyRef.current = false;
    };
  }, []);

  return (
    <ScreenRevealReadyContext.Provider value={readyRef}>
      <div className={`animate-fluent-screen ${className}`.trim()}>
        {children}
      </div>
    </ScreenRevealReadyContext.Provider>
  );
}

export function FluentReveal({
  className = "",
  children,
  ref,
  ...rest
}: ComponentPropsWithRef<"div">) {
  const readyRef = useContext(ScreenRevealReadyContext);
  const shouldAnimate = useRef(readyRef.current).current;

  return (
    <div
      ref={ref}
      {...rest}
      className={`${shouldAnimate ? "animate-fluent-item" : ""} ${className}`.trim()}
    >
      {children}
    </div>
  );
}

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
  title?: string;
}

interface FluentSegmentedControlProps<T extends string> {
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  variant?: "accent" | "surface";
  size?: "xs" | "sm" | "md";
  fullWidth?: boolean;
  ariaLabel?: string;
  className?: string;
}

interface PillRect {
  left: number;
  width: number;
}

export function FluentSegmentedControl<T extends string>({
  options,
  value,
  onChange,
  variant = "accent",
  size = "sm",
  fullWidth = false,
  ariaLabel,
  className = "",
}: FluentSegmentedControlProps<T>) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const buttonRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const prevValueRef = useRef<T>(value);

  const [pillRect, setPillRect] = useState<PillRect | null>(null);
  const [animated, setAnimated] = useState(false);

  const labelsKey = options
    .map((o) => `${o.value}:${o.label}`)
    .join("|");

  const measureActive = () => {
    const btn = buttonRefs.current[value];
    if (!btn) return null;
    return {
      left: btn.offsetLeft,
      width: btn.offsetWidth,
    };
  };

  useLayoutEffect(() => {
    if (prevValueRef.current !== value) {
      prevValueRef.current = value;
      setAnimated(true);
    }
    const next = measureActive();
    if (next) {
      setPillRect((prev) =>
        prev && prev.left === next.left && prev.width === next.width
          ? prev
          : next
      );
    }
  }, [value, fullWidth, labelsKey]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(() => {
      const next = measureActive();
      if (next) {
        setPillRect((prev) =>
          prev && prev.left === next.left && prev.width === next.width
            ? prev
            : next
        );
      }
    });

    observer.observe(container);
    return () => observer.disconnect();
  }, [value, labelsKey]);

  const containerColors =
    variant === "accent"
      ? "bg-surface border-border-subtle"
      : "bg-canvas border-border-subtle";

  const sizeContainer =
    size === "xs"
      ? "p-0.5 text-[11px] font-semibold"
      : size === "sm"
      ? "p-0.5 text-[12px] font-medium"
      : "p-0.5 text-[12.5px] font-medium";

  const sizeButton =
    size === "xs"
      ? "px-2 py-0.5 rounded-[4px]"
      : size === "sm"
      ? "px-2.5 py-1 rounded-[4px]"
      : "px-3 py-1.5 rounded-[4px]";

  const pillColors =
    variant === "accent" ? "bg-accent" : "bg-surface shadow-xs";

  return (
    <div
      ref={containerRef}
      role="group"
      aria-label={ariaLabel}
      className={`relative inline-flex items-center rounded-md border ${containerColors} ${sizeContainer} ${
        fullWidth ? "w-full" : ""
      } ${className}`.trim()}
    >
      {pillRect && (
        <span
          aria-hidden="true"
          className={`pointer-events-none absolute top-0.5 bottom-0.5 left-0 z-0 rounded-[4px] ${pillColors} ${
            animated ? "segmented-pill-animated" : "segmented-pill"
          }`}
          style={{
            transform: `translate3d(${pillRect.left}px, 0, 0)`,
            width: `${pillRect.width}px`,
          }}
        />
      )}

      {options.map((option) => {
        const active = option.value === value;
        const textClass = active
          ? variant === "accent"
            ? "text-accent-fg"
            : "text-text-primary"
          : "text-text-secondary hover:text-text-primary";

        return (
          <button
            key={option.value}
            ref={(el) => {
              buttonRefs.current[option.value] = el;
            }}
            type="button"
            disabled={option.disabled}
            title={option.title}
            aria-pressed={active}
            onClick={() => onChange(option.value)}
            className={`relative z-10 ${
              fullWidth ? "flex-1" : ""
            } ${sizeButton} ${textClass} transition-colors duration-150 disabled:opacity-45 disabled:cursor-not-allowed whitespace-nowrap`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
