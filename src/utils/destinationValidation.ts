import type { FileScanResult, FileTarget } from "../types/studio";

export const INSTALL_PRESETS = [
  "Windows\\Setup\\Scripts",
  "Users\\Default\\Desktop",
  "ProgramData",
  "Tools",
];

export const ISO_PRESETS = ["sources\\$OEM$", "Soft"];

export const PROTECTED_ISO_FILES = new Set([
  "sources\\install.wim",
  "sources\\install.esd",
  "sources\\boot.wim",
  "boot\\etfsboot.com",
  "efi\\microsoft\\boot\\efisys.bin",
  "efi\\microsoft\\boot\\efisys_noprompt.bin",
  "bootmgr",
  "bootmgr.efi",
]);

export function getBaseName(fullPath: string): string {
  const parts = fullPath.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? fullPath;
}

export function defaultDestinationForScan(scan: FileScanResult): string {
  if (scan.isDirectory) {
    return "";
  }
  const base = getBaseName(scan.path).toLowerCase();
  if (base.endsWith(".cmd") || base.endsWith(".bat") || base.endsWith(".ps1")) {
    return "Windows\\Setup\\Scripts";
  }
  return "Tools";
}

export function isWindowsReservedName(segment: string): boolean {
  const trimmed = segment.replace(/[. ]+$/g, "");
  const stem = trimmed.split(".")[0]?.toUpperCase() ?? "";
  return /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/.test(stem);
}

export function validateDestinationClient(
  target: FileTarget,
  rawDest: string,
  baseName: string
): {
  errorKey: "invalidDestTraversal" | "invalidDestProtected" | null;
  cleanDir: string;
} {
  const unified = rawDest.trim().replace(/\//g, "\\");
  if (
    unified.startsWith("\\\\") ||
    unified.includes(":") ||
    /[<>"|?*\u0000-\u001f]/.test(unified)
  ) {
    return { errorKey: "invalidDestTraversal", cleanDir: unified };
  }

  const baseTrimmed = baseName.replace(/[. ]+$/g, "");
  if (
    baseName.length === 0 ||
    baseName === "." ||
    baseName === ".." ||
    baseTrimmed.length === 0 ||
    baseName !== baseTrimmed ||
    /[\\/:<>"|?*\u0000-\u001f]/.test(baseName) ||
    isWindowsReservedName(baseName)
  ) {
    return { errorKey: "invalidDestTraversal", cleanDir: unified };
  }

  const stripped = unified.replace(/^\\+|\\+$/g, "");
  if (stripped.length > 0) {
    const segments = stripped.split("\\");
    for (const seg of segments) {
      const segTrimmed = seg.replace(/[. ]+$/g, "");
      if (
        seg === "" ||
        seg === "." ||
        seg === ".." ||
        segTrimmed === "" ||
        seg !== segTrimmed ||
        isWindowsReservedName(seg)
      ) {
        return { errorKey: "invalidDestTraversal", cleanDir: stripped };
      }
    }
  }

  const fullRel = stripped ? `${stripped}\\${baseTrimmed}` : baseTrimmed;
  const norm = fullRel.toLowerCase();

  if (target === "installWim") {
    if (
      norm === "windows\\system32\\config" ||
      norm.startsWith("windows\\system32\\config\\") ||
      norm === "users\\default\\ntuser.dat" ||
      norm.startsWith("users\\default\\ntuser.dat\\")
    ) {
      return { errorKey: "invalidDestProtected", cleanDir: stripped };
    }
  } else {
    if (
      PROTECTED_ISO_FILES.has(norm) ||
      /^sources\\install[^\\]*\.swm$/i.test(norm)
    ) {
      return { errorKey: "invalidDestProtected", cleanDir: stripped };
    }
  }

  return { errorKey: null, cleanDir: stripped };
}
