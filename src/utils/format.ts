import i18n from "../i18n";

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} ${i18n.t("common.units.gb")}`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} ${i18n.t("common.units.mb")}`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} ${i18n.t("common.units.kb")}`;
  }
  return `${bytes} ${i18n.t("common.units.b")}`;
}
