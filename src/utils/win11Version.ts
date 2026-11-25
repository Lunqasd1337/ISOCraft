export const WIN11_VERSIONS = ["26H2", "25H2", "24H2", "23H2", "22H2", "21H2"] as const;

export function resolveWin11Version(buildNumber: number | null): string {
  if (buildNumber === null) return "26H2";
  if (buildNumber >= 26300) return "26H2";
  if (buildNumber >= 26200) return "25H2";
  if (buildNumber >= 26100) return "24H2";
  if (buildNumber >= 22631) return "23H2";
  if (buildNumber >= 22621) return "22H2";
  if (buildNumber >= 22000) return "21H2";
  return "26H2";
}
