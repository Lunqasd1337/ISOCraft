import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import {
  ClipboardPaste,
  Copy,
  Redo2,
  Scissors,
  TextSelect,
  Trash2,
  Undo2,
  type LucideIcon,
} from "lucide-react";
import {
  buildContextMenuItems,
  clampMenuPosition,
  isTextEditableInputType,
  shouldBlockBrowserShortcut,
  type ContextMenuActionId,
  type ContextMenuItem,
  type ContextMenuSnapshot,
} from "../utils/contextMenu";

type EditableInputElement = HTMLInputElement | HTMLTextAreaElement;

interface TargetContext {
  editableEl: EditableInputElement | null;
  contentEditableEl: HTMLElement | null;
  contentEditableRange: Range | null;
  selectionStart: number | null;
  selectionEnd: number | null;
  selectionDirection: "forward" | "backward" | "none" | null;
  selectedText: string;
  selectableContainer: HTMLElement | null;
}

interface MenuState {
  anchorX: number;
  anchorY: number;
  items: ContextMenuItem[];
  target: TargetContext;
}

const ACTION_ICONS: Record<ContextMenuActionId, LucideIcon> = {
  undo: Undo2,
  redo: Redo2,
  cut: Scissors,
  copy: Copy,
  paste: ClipboardPaste,
  delete: Trash2,
  selectAll: TextSelect,
};

function queryCommandEnabledSafe(command: string): boolean {
  try {
    return document.queryCommandEnabled(command);
  } catch {
    return false;
  }
}

function readSelectionOffsets(el: EditableInputElement): {
  start: number | null;
  end: number | null;
  direction: "forward" | "backward" | "none" | null;
} {
  try {
    const start = typeof el.selectionStart === "number" ? el.selectionStart : null;
    const end = typeof el.selectionEnd === "number" ? el.selectionEnd : null;
    const dir = el.selectionDirection ?? null;
    return { start, end, direction: dir };
  } catch {
    return { start: null, end: null, direction: null };
  }
}

function restoreInputSelection(
  el: EditableInputElement,
  target: TargetContext
): void {
  el.focus({ preventScroll: true });
  if (target.selectionStart !== null && target.selectionEnd !== null) {
    try {
      el.setSelectionRange(
        target.selectionStart,
        target.selectionEnd,
        target.selectionDirection ?? undefined
      );
    } catch {
      // Ignore elements that disallow setSelectionRange (e.g. input[type="number"])
    }
  }
}

function restoreContentEditableSelection(
  el: HTMLElement,
  range: Range | null
): void {
  el.focus({ preventScroll: true });
  if (!range) return;
  try {
    const sel = window.getSelection();
    if (sel) {
      sel.removeAllRanges();
      sel.addRange(range);
    }
  } catch {
    // Ignore detached ranges
  }
}

function setNativeInputValue(
  el: EditableInputElement,
  nextValue: string,
  caretPos: number
): void {
  const proto =
    el instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  const descriptor = Object.getOwnPropertyDescriptor(proto, "value");
  descriptor?.set?.call(el, nextValue);
  try {
    if (typeof el.selectionStart === "number") {
      el.setSelectionRange(caretPos, caretPos);
    }
  } catch {
    // Ignore elements without selection range support
  }
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

function deleteSelectedRange(
  el: EditableInputElement,
  target: TargetContext
): void {
  const prevValue = el.value;
  let executed = false;
  try {
    executed = document.execCommand("delete");
  } catch {
    executed = false;
  }
  if (executed && el.value !== prevValue) {
    return;
  }
  if (
    target.selectionStart !== null &&
    target.selectionEnd !== null &&
    target.selectionEnd > target.selectionStart
  ) {
    const nextValue =
      prevValue.slice(0, target.selectionStart) +
      prevValue.slice(target.selectionEnd);
    setNativeInputValue(el, nextValue, target.selectionStart);
  }
}

function insertTextAtSelection(
  el: EditableInputElement,
  target: TargetContext,
  rawText: string
): void {
  if (!rawText) return;
  const prevValue = el.value;
  const isSingleLine = el instanceof HTMLInputElement;
  const sanitized = isSingleLine ? rawText.replace(/[\r\n]+/g, "") : rawText;
  if (!sanitized) return;

  let executed = false;
  try {
    executed = document.execCommand("insertText", false, sanitized);
  } catch {
    executed = false;
  }
  if (executed && el.value !== prevValue) {
    return;
  }

  const start = target.selectionStart ?? prevValue.length;
  const end = target.selectionEnd ?? start;
  const before = prevValue.slice(0, start);
  const after = prevValue.slice(end);
  const inserted =
    el.maxLength > 0
      ? sanitized.slice(0, Math.max(0, el.maxLength - (before.length + after.length)))
      : sanitized;
  const nextValue = before + inserted + after;
  setNativeInputValue(el, nextValue, before.length + inserted.length);
}

async function writeClipboardText(text: string): Promise<void> {
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // Fallback below
  }
  try {
    document.execCommand("copy");
  } catch {
    // Ignore clipboard failure
  }
}

function selectContainerContents(container: HTMLElement): void {
  const sel = window.getSelection();
  if (!sel) return;
  const range = document.createRange();
  range.selectNodeContents(container);
  sel.removeAllRanges();
  sel.addRange(range);
}

function getActionIndices(items: ContextMenuItem[]): number[] {
  const indices: number[] = [];
  items.forEach((item, idx) => {
    if (item.type === "action" && !item.disabled) {
      indices.push(idx);
    }
  });
  return indices;
}

export function AppContextMenu() {
  const { t } = useTranslation();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [activeIndex, setActiveIndex] = useState<number>(-1);
  const [coords, setCoords] = useState<{
    left: number;
    top: number;
    originX: "left" | "right";
    originY: "top" | "bottom";
  } | null>(null);

  const menuRef = useRef<HTMLDivElement | null>(null);
  const menuStateRef = useRef<MenuState | null>(null);
  menuStateRef.current = menu;
  const activeIndexRef = useRef<number>(-1);
  activeIndexRef.current = activeIndex;

  const closeMenu = useCallback(() => {
    setMenu(null);
    setActiveIndex(-1);
    setCoords(null);
  }, []);

  const executeAction = useCallback(
    async (actionId: ContextMenuActionId, currentMenu: MenuState) => {
      const { target } = currentMenu;
      closeMenu();

      switch (actionId) {
        case "undo": {
          if (target.editableEl) {
            target.editableEl.focus({ preventScroll: true });
            try {
              document.execCommand("undo");
            } catch {
              // Ignore
            }
          } else if (target.contentEditableEl) {
            restoreContentEditableSelection(
              target.contentEditableEl,
              target.contentEditableRange
            );
            try {
              document.execCommand("undo");
            } catch {
              // Ignore
            }
          }
          break;
        }
        case "redo": {
          if (target.editableEl) {
            target.editableEl.focus({ preventScroll: true });
            try {
              document.execCommand("redo");
            } catch {
              // Ignore
            }
          } else if (target.contentEditableEl) {
            restoreContentEditableSelection(
              target.contentEditableEl,
              target.contentEditableRange
            );
            try {
              document.execCommand("redo");
            } catch {
              // Ignore
            }
          }
          break;
        }
        case "copy": {
          if (target.editableEl) {
            restoreInputSelection(target.editableEl, target);
          } else if (target.contentEditableEl) {
            restoreContentEditableSelection(
              target.contentEditableEl,
              target.contentEditableRange
            );
          }
          const text =
            target.selectedText || (window.getSelection()?.toString() ?? "");
          await writeClipboardText(text);
          break;
        }
        case "cut": {
          if (target.editableEl) {
            restoreInputSelection(target.editableEl, target);
            const text = target.selectedText;
            await writeClipboardText(text);
            restoreInputSelection(target.editableEl, target);
            deleteSelectedRange(target.editableEl, target);
          } else if (target.contentEditableEl) {
            restoreContentEditableSelection(
              target.contentEditableEl,
              target.contentEditableRange
            );
            const text =
              target.selectedText || (window.getSelection()?.toString() ?? "");
            await writeClipboardText(text);
            restoreContentEditableSelection(
              target.contentEditableEl,
              target.contentEditableRange
            );
            try {
              document.execCommand("delete");
            } catch {
              // Ignore
            }
          }
          break;
        }
        case "paste": {
          let text = "";
          try {
            text = await navigator.clipboard.readText();
          } catch {
            return;
          }
          if (!text) return;
          if (target.editableEl && target.editableEl.isConnected) {
            restoreInputSelection(target.editableEl, target);
            insertTextAtSelection(target.editableEl, target, text);
          } else if (
            target.contentEditableEl &&
            target.contentEditableEl.isConnected
          ) {
            restoreContentEditableSelection(
              target.contentEditableEl,
              target.contentEditableRange
            );
            try {
              document.execCommand("insertText", false, text);
            } catch {
              // Ignore
            }
          }
          break;
        }
        case "delete": {
          if (target.editableEl) {
            restoreInputSelection(target.editableEl, target);
            deleteSelectedRange(target.editableEl, target);
          } else if (target.contentEditableEl) {
            restoreContentEditableSelection(
              target.contentEditableEl,
              target.contentEditableRange
            );
            try {
              document.execCommand("delete");
            } catch {
              // Ignore
            }
          }
          break;
        }
        case "selectAll": {
          if (target.editableEl) {
            target.editableEl.focus({ preventScroll: true });
            target.editableEl.select();
          } else if (target.contentEditableEl) {
            target.contentEditableEl.focus({ preventScroll: true });
            selectContainerContents(target.contentEditableEl);
          } else if (target.selectableContainer) {
            selectContainerContents(target.selectableContainer);
          }
          break;
        }
      }
    },
    [closeMenu]
  );

  useEffect(() => {
    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();

      const rawTarget = e.target;
      const targetEl =
        rawTarget instanceof HTMLElement
          ? rawTarget
          : rawTarget instanceof Node
          ? rawTarget.parentElement
          : null;

      if (targetEl && menuRef.current?.contains(targetEl)) {
        return;
      }

      const inputOrTextarea = targetEl?.closest("input, textarea") as
        | EditableInputElement
        | null;

      if (
        inputOrTextarea &&
        !inputOrTextarea.disabled &&
        (inputOrTextarea instanceof HTMLTextAreaElement ||
          isTextEditableInputType(inputOrTextarea.type))
      ) {
        const isPassword =
          inputOrTextarea instanceof HTMLInputElement &&
          inputOrTextarea.type.toLowerCase() === "password";
        const { start, end, direction } = readSelectionOffsets(inputOrTextarea);
        const hasRangeSelection =
          start !== null && end !== null && end > start;
        const selectedText = hasRangeSelection
          ? inputOrTextarea.value.slice(start, end)
          : window.getSelection()?.toString() ?? "";
        const hasSelection = selectedText.length > 0;
        const hasValue = inputOrTextarea.value.length > 0;
        const isAllSelected =
          hasValue &&
          start === 0 &&
          end === inputOrTextarea.value.length;

        let snapshot: ContextMenuSnapshot;
        if (inputOrTextarea.readOnly) {
          snapshot = {
            kind: "readonly-input",
            canUndo: false,
            canRedo: false,
            hasSelection,
            hasValue,
            isAllSelected,
            isPassword,
            canSelectContainer: false,
          };
        } else {
          if (document.activeElement !== inputOrTextarea) {
            inputOrTextarea.focus({ preventScroll: true });
          }
          snapshot = {
            kind: "editable",
            canUndo: queryCommandEnabledSafe("undo"),
            canRedo: queryCommandEnabledSafe("redo"),
            hasSelection,
            hasValue,
            isAllSelected,
            isPassword,
            canSelectContainer: false,
          };
        }

        const items = buildContextMenuItems(snapshot);
        if (items.length === 0) {
          closeMenu();
          return;
        }

        setCoords(null);
        setActiveIndex(-1);
        setMenu({
          anchorX: e.clientX,
          anchorY: e.clientY,
          items,
          target: {
            editableEl: inputOrTextarea,
            contentEditableEl: null,
            contentEditableRange: null,
            selectionStart: start,
            selectionEnd: end,
            selectionDirection: direction,
            selectedText,
            selectableContainer: null,
          },
        });
        return;
      }

      const contentEditableEl = targetEl?.closest(
        '[contenteditable="true"], [contenteditable=""]'
      ) as HTMLElement | null;

      if (contentEditableEl && contentEditableEl.isContentEditable) {
        if (document.activeElement !== contentEditableEl) {
          contentEditableEl.focus({ preventScroll: true });
        }
        const sel = window.getSelection();
        const range =
          sel && sel.rangeCount > 0 ? sel.getRangeAt(0).cloneRange() : null;
        const selectedText = sel ? sel.toString() : "";
        const fullText = contentEditableEl.textContent ?? "";
        const hasSelection = selectedText.length > 0;
        const hasValue = fullText.length > 0;
        const isAllSelected = hasValue && selectedText === fullText;

        const items = buildContextMenuItems({
          kind: "editable",
          canUndo: queryCommandEnabledSafe("undo"),
          canRedo: queryCommandEnabledSafe("redo"),
          hasSelection,
          hasValue,
          isAllSelected,
          isPassword: false,
          canSelectContainer: false,
        });
        if (items.length === 0) {
          closeMenu();
          return;
        }

        setCoords(null);
        setActiveIndex(-1);
        setMenu({
          anchorX: e.clientX,
          anchorY: e.clientY,
          items,
          target: {
            editableEl: null,
            contentEditableEl,
            contentEditableRange: range,
            selectionStart: null,
            selectionEnd: null,
            selectionDirection: null,
            selectedText,
            selectableContainer: null,
          },
        });
        return;
      }

      const selectedText = window.getSelection()?.toString() ?? "";
      const selectableContainer = targetEl?.closest(
        ".select-text, pre"
      ) as HTMLElement | null;
      const canSelectContainer = Boolean(
        selectableContainer &&
          (selectableContainer.textContent ?? "").trim().length > 0
      );

      let snapshot: ContextMenuSnapshot | null = null;
      if (selectedText.length > 0) {
        snapshot = {
          kind: "selection",
          canUndo: false,
          canRedo: false,
          hasSelection: true,
          hasValue: true,
          isAllSelected: false,
          isPassword: false,
          canSelectContainer,
        };
      } else if (canSelectContainer) {
        snapshot = {
          kind: "selectable",
          canUndo: false,
          canRedo: false,
          hasSelection: false,
          hasValue: true,
          isAllSelected: false,
          isPassword: false,
          canSelectContainer: true,
        };
      }

      if (!snapshot) {
        closeMenu();
        return;
      }

      const items = buildContextMenuItems(snapshot);
      if (items.length === 0) {
        closeMenu();
        return;
      }

      setCoords(null);
      setActiveIndex(-1);
      setMenu({
        anchorX: e.clientX,
        anchorY: e.clientY,
        items,
        target: {
          editableEl: null,
          contentEditableEl: null,
          contentEditableRange: null,
          selectionStart: null,
          selectionEnd: null,
          selectionDirection: null,
          selectedText,
          selectableContainer: canSelectContainer ? selectableContainer : null,
        },
      });
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (shouldBlockBrowserShortcut(e, import.meta.env.DEV)) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }

      const currentMenu = menuStateRef.current;
      if (!currentMenu) return;

      if (e.key === "Escape") {
        e.preventDefault();
        e.stopImmediatePropagation();
        closeMenu();
        return;
      }

      if (e.key === "Tab") {
        closeMenu();
        return;
      }

      const enabledIndices = getActionIndices(currentMenu.items);
      if (enabledIndices.length === 0) return;

      if (e.key === "ArrowDown") {
        e.preventDefault();
        e.stopPropagation();
        const curPos = enabledIndices.indexOf(activeIndexRef.current);
        const nextIdx =
          curPos === -1
            ? enabledIndices[0]
            : enabledIndices[(curPos + 1) % enabledIndices.length];
        setActiveIndex(nextIdx);
        return;
      }

      if (e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        const curPos = enabledIndices.indexOf(activeIndexRef.current);
        const nextIdx =
          curPos <= 0
            ? enabledIndices[enabledIndices.length - 1]
            : enabledIndices[curPos - 1];
        setActiveIndex(nextIdx);
        return;
      }

      if (e.key === "Home") {
        e.preventDefault();
        e.stopPropagation();
        setActiveIndex(enabledIndices[0]);
        return;
      }

      if (e.key === "End") {
        e.preventDefault();
        e.stopPropagation();
        setActiveIndex(enabledIndices[enabledIndices.length - 1]);
        return;
      }

      if (e.key === "Enter" || e.key === " ") {
        const activeItem = currentMenu.items[activeIndexRef.current];
        if (activeItem && activeItem.type === "action" && !activeItem.disabled) {
          e.preventDefault();
          e.stopPropagation();
          void executeAction(activeItem.id, currentMenu);
        }
      }
    };

    const handlePointerDown = (e: PointerEvent) => {
      if (!menuStateRef.current) return;
      if (e.target instanceof Node && menuRef.current?.contains(e.target)) {
        return;
      }
      closeMenu();
    };

    const handleScroll = (e: Event) => {
      if (!menuStateRef.current) return;
      if (e.target instanceof Node && menuRef.current?.contains(e.target)) {
        return;
      }
      closeMenu();
    };

    const handleWindowDismiss = () => {
      if (menuStateRef.current) {
        closeMenu();
      }
    };

    window.addEventListener("contextmenu", handleContextMenu);
    window.addEventListener("keydown", handleKeyDown, { capture: true });
    window.addEventListener("pointerdown", handlePointerDown, { capture: true });
    window.addEventListener("scroll", handleScroll, { capture: true });
    window.addEventListener("blur", handleWindowDismiss);
    window.addEventListener("resize", handleWindowDismiss);

    return () => {
      window.removeEventListener("contextmenu", handleContextMenu);
      window.removeEventListener("keydown", handleKeyDown, { capture: true });
      window.removeEventListener("pointerdown", handlePointerDown, { capture: true });
      window.removeEventListener("scroll", handleScroll, { capture: true });
      window.removeEventListener("blur", handleWindowDismiss);
      window.removeEventListener("resize", handleWindowDismiss);
    };
  }, [closeMenu, executeAction]);

  useLayoutEffect(() => {
    if (!menu) return;
    const el = menuRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setCoords(
      clampMenuPosition(
        menu.anchorX,
        menu.anchorY,
        rect.width,
        rect.height,
        window.innerWidth,
        window.innerHeight
      )
    );
  }, [menu]);

  if (!menu) return null;

  const left = coords?.left ?? menu.anchorX;
  const top = coords?.top ?? menu.anchorY;
  const transformOrigin = coords
    ? `${coords.originY} ${coords.originX}`
    : "top left";

  return createPortal(
    <div
      ref={menuRef}
      role="menu"
      aria-label={t("contextMenu.ariaLabel")}
      onMouseDown={(e) => e.preventDefault()}
      style={{
        left,
        top,
        transformOrigin,
        visibility: coords ? "visible" : "hidden",
      }}
      className="animate-fluent-menu fixed z-[100] min-w-[200px] p-1 rounded-lg bg-surface border border-border-subtle shadow-xl select-none"
    >
      {menu.items.map((item, idx) => {
        if (item.type === "separator") {
          return (
            <div
              key={item.id}
              role="separator"
              className="my-1 mx-1.5 h-px bg-border-subtle"
            />
          );
        }

        const Icon = ACTION_ICONS[item.id];
        const isActive = idx === activeIndex;

        return (
          <button
            key={item.id}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onMouseEnter={() => {
              if (!item.disabled) setActiveIndex(idx);
            }}
            onMouseLeave={() => {
              if (activeIndex === idx) setActiveIndex(-1);
            }}
            onClick={() => {
              if (!item.disabled) {
                void executeAction(item.id, menu);
              }
            }}
            className={`w-full flex items-center justify-between gap-4 px-2.5 py-1.5 rounded-md text-[12.5px] transition-colors ${
              item.disabled
                ? "text-text-secondary opacity-45 cursor-default"
                : isActive
                ? "bg-subtle text-text-primary"
                : "text-text-primary hover:bg-subtle"
            }`}
          >
            <span className="flex items-center gap-2.5 min-w-0">
              <Icon className="w-3.5 h-3.5 text-text-secondary shrink-0" strokeWidth={1.7} />
              <span className="truncate">{t(item.labelKey)}</span>
            </span>
            <span className="text-[11px] font-mono text-text-secondary shrink-0">
              {item.shortcut}
            </span>
          </button>
        );
      })}
    </div>,
    document.body
  );
}
