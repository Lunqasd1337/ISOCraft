import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

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

  const requestClose = useCallback(() => {
    if (isClosing || locked) return;
    setIsClosing(true);
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
    }
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null;
      onClose();
      setIsClosing(false);
    }, 160);
  }, [isClosing, locked, onClose]);

  useEffect(() => {
    if (!enabled) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (onBeforeEscape?.()) return;
        requestClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
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
  maxWidthClass?: string;
  backdropClass?: string;
  panelClassName?: string;
  children: ReactNode;
}

export function FluentModal({
  isClosing,
  onRequestClose,
  maxWidthClass = "max-w-[600px]",
  backdropClass = "bg-black/45",
  panelClassName = "",
  children,
}: FluentModalProps) {
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
        className={`${
          isClosing ? "animate-fluent-modal-exit" : "animate-fluent-modal"
        } w-full ${maxWidthClass} bg-surface border border-border-subtle rounded-lg shadow-xl overflow-hidden ${panelClassName}`}
      >
        {children}
      </div>
    </div>,
    document.body
  );
}
