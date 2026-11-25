import {
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { prefersReducedMotion } from "../utils/motion";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "textarea:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

const modalStack: symbol[] = [];

interface UseAnimatedModalCloseOptions {
  locked?: boolean;
  enabled?: boolean;
  onBeforeEscape?: () => boolean;
}

export function useAnimatedModalClose(
  onClose: () => void,
  options?: UseAnimatedModalCloseOptions
) {
  const locked = options?.locked ?? false;
  const enabled = options?.enabled ?? true;
  const onBeforeEscape = options?.onBeforeEscape;

  const [isClosing, setIsClosing] = useState(false);
  const closeTimerRef = useRef<number | null>(null);
  const modalIdRef = useRef<symbol>(Symbol("fluent-modal"));

  const requestClose = useCallback(() => {
    if (isClosing || locked) return;
    if (prefersReducedMotion()) {
      onClose();
      return;
    }
    setIsClosing(true);
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
    }
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null;
      onClose();
      setIsClosing(false);
    }, 150);
  }, [isClosing, locked, onClose]);

  useEffect(() => {
    if (!enabled) return;
    const modalId = modalIdRef.current;
    modalStack.push(modalId);
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (modalStack[modalStack.length - 1] !== modalId) return;
        if (onBeforeEscape?.()) return;
        requestClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      const idx = modalStack.lastIndexOf(modalId);
      if (idx !== -1) {
        modalStack.splice(idx, 1);
      }
    };
  }, [enabled, onBeforeEscape, requestClose]);

  useEffect(() => {
    return () => {
      if (closeTimerRef.current !== null) {
        window.clearTimeout(closeTimerRef.current);
      }
    };
  }, []);

  return { isClosing, requestClose };
}

interface FluentModalProps {
  isClosing: boolean;
  onRequestClose: () => void;
  ariaLabel?: string;
  maxWidthClass?: string;
  backdropClass?: string;
  panelClassName?: string;
  children: ReactNode;
}

export function FluentModal({
  isClosing,
  onRequestClose,
  ariaLabel,
  maxWidthClass = "max-w-[600px]",
  backdropClass = "bg-black/45",
  panelClassName = "",
  children,
}: FluentModalProps) {
  const panelRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const previouslyFocused =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const panel = panelRef.current;
    if (!panel) return;

    const getFocusable = () =>
      Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
        (el) => el.offsetParent !== null || el === document.activeElement
      );

    const initialTarget = getFocusable()[0] ?? panel;
    initialTarget.focus({ preventScroll: true });

    const handleTab = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const focusables = getFocusable();
      if (focusables.length === 0) {
        e.preventDefault();
        panel.focus();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;

      if (e.shiftKey) {
        if (active === first || active === panel || !panel.contains(active)) {
          e.preventDefault();
          last.focus();
        }
      } else if (active === last || !panel.contains(active)) {
        e.preventDefault();
        first.focus();
      }
    };

    panel.addEventListener("keydown", handleTab);
    return () => {
      panel.removeEventListener("keydown", handleTab);
      if (previouslyFocused && previouslyFocused.isConnected) {
        previouslyFocused.focus({ preventScroll: true });
      }
    };
  }, []);

  return createPortal(
    <div
      onClick={(e) => {
        if (e.target === e.currentTarget) onRequestClose();
      }}
      className={`${
        isClosing ? "animate-fluent-backdrop-exit" : "animate-fluent-backdrop"
      } fixed inset-0 z-50 flex items-center justify-center ${backdropClass} p-4`}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel}
        tabIndex={-1}
        className={`${
          isClosing ? "animate-fluent-modal-exit" : "animate-fluent-modal"
        } w-full ${maxWidthClass} bg-surface border border-border-subtle rounded-lg shadow-xl overflow-hidden focus:outline-none ${panelClassName}`}
      >
        {children}
      </div>
    </div>,
    document.body
  );
}
