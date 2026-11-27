export type ContextMenuActionId =
  | "undo"
  | "redo"
  | "cut"
  | "copy"
  | "paste"
  | "delete"
  | "selectAll";

export type ContextMenuLabelKey =
  | "contextMenu.undo"
  | "contextMenu.redo"
  | "contextMenu.cut"
  | "contextMenu.copy"
  | "contextMenu.paste"
  | "contextMenu.delete"
  | "contextMenu.selectAll";

export type ContextMenuItem =
  | {
      type: "action";
      id: ContextMenuActionId;
      labelKey: ContextMenuLabelKey;
      shortcut: string;
      disabled: boolean;
    }
  | {
      type: "separator";
      id: string;
    };

export interface ContextMenuSnapshot {
  kind: "editable" | "readonly-input" | "selection" | "selectable";
  canUndo: boolean;
  canRedo: boolean;
  hasSelection: boolean;
  hasValue: boolean;
  isAllSelected: boolean;
  isPassword: boolean;
  canSelectContainer: boolean;
}

const TEXT_EDITABLE_INPUT_TYPES = new Set([
  "",
  "text",
  "search",
  "url",
  "tel",
  "email",
  "password",
  "number",
]);

/**
 * Returns true for <input> types that behave as text-editable fields.
 */
export function isTextEditableInputType(type: string | null | undefined): boolean {
  const normalized = (type ?? "").trim().toLowerCase();
  return TEXT_EDITABLE_INPUT_TYPES.has(normalized);
}

/**
 * Builds the ordered context menu items for a snapshotted interaction target.
 */
export function buildContextMenuItems(
  snapshot: ContextMenuSnapshot
): ContextMenuItem[] {
  if (snapshot.kind === "editable") {
    const canCopyOrCut = snapshot.hasSelection && !snapshot.isPassword;
    const items: ContextMenuItem[] = [
      {
        type: "action",
        id: "undo",
        labelKey: "contextMenu.undo",
        shortcut: "Ctrl+Z",
        disabled: !snapshot.canUndo,
      },
    ];
    if (snapshot.canRedo) {
      items.push({
        type: "action",
        id: "redo",
        labelKey: "contextMenu.redo",
        shortcut: "Ctrl+Y",
        disabled: false,
      });
    }
    items.push(
      { type: "separator", id: "sep-history" },
      {
        type: "action",
        id: "cut",
        labelKey: "contextMenu.cut",
        shortcut: "Ctrl+X",
        disabled: !canCopyOrCut,
      },
      {
        type: "action",
        id: "copy",
        labelKey: "contextMenu.copy",
        shortcut: "Ctrl+C",
        disabled: !canCopyOrCut,
      },
      {
        type: "action",
        id: "paste",
        labelKey: "contextMenu.paste",
        shortcut: "Ctrl+V",
        disabled: false,
      },
      {
        type: "action",
        id: "delete",
        labelKey: "contextMenu.delete",
        shortcut: "Del",
        disabled: !snapshot.hasSelection,
      },
      { type: "separator", id: "sep-select" },
      {
        type: "action",
        id: "selectAll",
        labelKey: "contextMenu.selectAll",
        shortcut: "Ctrl+A",
        disabled: !snapshot.hasValue || snapshot.isAllSelected,
      }
    );
    return items;
  }

  if (snapshot.kind === "readonly-input") {
    if (!snapshot.hasValue && !snapshot.hasSelection) {
      return [];
    }
    return [
      {
        type: "action",
        id: "copy",
        labelKey: "contextMenu.copy",
        shortcut: "Ctrl+C",
        disabled: !snapshot.hasSelection || snapshot.isPassword,
      },
      {
        type: "action",
        id: "selectAll",
        labelKey: "contextMenu.selectAll",
        shortcut: "Ctrl+A",
        disabled: !snapshot.hasValue || snapshot.isAllSelected,
      },
    ];
  }

  if (snapshot.kind === "selection") {
    const items: ContextMenuItem[] = [
      {
        type: "action",
        id: "copy",
        labelKey: "contextMenu.copy",
        shortcut: "Ctrl+C",
        disabled: false,
      },
    ];
    if (snapshot.canSelectContainer) {
      items.push({
        type: "action",
        id: "selectAll",
        labelKey: "contextMenu.selectAll",
        shortcut: "Ctrl+A",
        disabled: false,
      });
    }
    return items;
  }

  // "selectable"
  if (!snapshot.canSelectContainer) {
    return [];
  }
  return [
    {
      type: "action",
      id: "copy",
      labelKey: "contextMenu.copy",
      shortcut: "Ctrl+C",
      disabled: true,
    },
    {
      type: "action",
      id: "selectAll",
      labelKey: "contextMenu.selectAll",
      shortcut: "Ctrl+A",
      disabled: false,
    },
  ];
}

export interface ShortcutKeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * Returns true if the keyboard event corresponds to a browser-only shortcut
 * (reload, history navigation, print, save page, view source, etc.) that
 * should be blocked inside the desktop WebView2 shell.
 */
export function shouldBlockBrowserShortcut(
  e: ShortcutKeyLike,
  isDev: boolean
): boolean {
  const mod = e.ctrlKey || e.metaKey;
  const key = (e.key ?? "").toLowerCase();
  const code = e.code ?? "";

  // Reload: F5, Ctrl+R, Ctrl+Shift+R, BrowserRefresh
  if (
    key === "f5" ||
    code === "F5" ||
    key === "browserrefresh" ||
    (mod && !e.altKey && (code === "KeyR" || key === "r"))
  ) {
    return true;
  }

  // History navigation: Alt+Left/Right, BrowserBack/Forward
  if (
    key === "browserback" ||
    key === "browserforward" ||
    (e.altKey &&
      !mod &&
      (key === "arrowleft" ||
        key === "arrowright" ||
        code === "ArrowLeft" ||
        code === "ArrowRight"))
  ) {
    return true;
  }

  // Print: Ctrl+P / Ctrl+Shift+P
  if (mod && !e.altKey && (code === "KeyP" || key === "p")) {
    return true;
  }

  // Save page, View source, Downloads, History, Open file
  if (
    mod &&
    !e.altKey &&
    !e.shiftKey &&
    (code === "KeyS" ||
      key === "s" ||
      code === "KeyU" ||
      key === "u" ||
      code === "KeyJ" ||
      key === "j" ||
      code === "KeyH" ||
      key === "h" ||
      code === "KeyO" ||
      key === "o")
  ) {
    return true;
  }

  // Caret browsing toggle: F7
  if (key === "f7" || code === "F7") {
    return true;
  }

  // DevTools in production builds: F12, Ctrl+Shift+I/J/C
  if (!isDev) {
    if (key === "f12" || code === "F12") {
      return true;
    }
    if (
      mod &&
      e.shiftKey &&
      !e.altKey &&
      (code === "KeyI" ||
        key === "i" ||
        code === "KeyJ" ||
        key === "j" ||
        code === "KeyC" ||
        key === "c")
    ) {
      return true;
    }
  }

  return false;
}

export interface ClampedMenuPosition {
  left: number;
  top: number;
  originX: "left" | "right";
  originY: "top" | "bottom";
}

/**
 * Clamps and flips a floating context menu within the viewport bounds.
 */
export function clampMenuPosition(
  anchorX: number,
  anchorY: number,
  menuWidth: number,
  menuHeight: number,
  viewportWidth: number,
  viewportHeight: number,
  margin = 8
): ClampedMenuPosition {
  let left = anchorX;
  let originX: "left" | "right" = "left";
  if (anchorX + menuWidth + margin > viewportWidth) {
    if (anchorX - menuWidth >= margin) {
      left = anchorX - menuWidth;
      originX = "right";
    } else {
      left = Math.max(margin, viewportWidth - menuWidth - margin);
    }
  } else if (left < margin) {
    left = margin;
  }

  let top = anchorY;
  let originY: "top" | "bottom" = "top";
  if (anchorY + menuHeight + margin > viewportHeight) {
    if (anchorY - menuHeight >= margin) {
      top = anchorY - menuHeight;
      originY = "bottom";
    } else {
      top = Math.max(margin, viewportHeight - menuHeight - margin);
    }
  } else if (top < margin) {
    top = margin;
  }

  return { left, top, originX, originY };
}
