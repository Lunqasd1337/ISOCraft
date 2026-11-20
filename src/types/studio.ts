export type SectionId =
  | "image"
  | "apps"
  | "features"
  | "updates"
  | "drivers"
  | "registry"
  | "files"
  | "build";

export type AppTheme = "light" | "dark" | "system";
export type AppLanguage = "ru" | "en";

export interface ToolStatus {
  found: boolean;
  path: string | null;
  source: "system" | "bundled" | "adk" | "settings" | "path" | "missing" | string;
}

export interface EnvironmentStatus {
  isAdmin: boolean;
  dism: ToolStatus;
  wimlib: ToolStatus;
  xorriso: ToolStatus;
  oscdimg: ToolStatus;
  workspaceDir: string;
  freeSpaceBytes: number;
  freeSpaceGb: number;
  lowSpaceWarning: boolean;
  hasDirtyWorkspace: boolean;
  dirtyWorkspaceDetails: string | null;
}

export interface AppSettings {
  workspaceDir: string;
  oscdimgPath: string | null;
  theme: AppTheme;
  language: AppLanguage;
}

export interface LogEntry {
  id: number;
  timestamp: string;
  level: "info" | "warn" | "error" | string;
  message: string;
}

export type SourceKind = "iso" | "wim" | "esd";

export interface WimEditionInfo {
  index: number;
  name: string;
  description: string;
  editionId: string;
  installationType: string;
  architecture: string;
  version: string;
  buildNumber: number;
  totalBytes: number;
  languages: string[];
  isSupported: boolean;
  rejectionReason: string | null;
}

export interface ImageInspectionResult {
  sourcePath: string;
  sourceKind: SourceKind;
  installFileFormat: string;
  hasBootWim: boolean;
  hasSxs: boolean;
  totalSizeBytes: number;
  editions: WimEditionInfo[];
  isValidWin11ClientX64: boolean;
  blockingReason: string | null;
}

export interface ProvisionedAppxInfo {
  displayName: string;
  packageName: string;
  packageFamilyName: string;
  version: string;
  publisherId: string;
  isProtected: boolean;
  protectionReason: string | null;
}

export type StubOption = "None" | "UseStub" | "UsePreference";

export interface AddAppxRequest {
  packagePath: string;
  dependencyPaths: string[];
  licensePath: string | null;
  skipLicense: boolean;
  regionAll: boolean;
  stubOption: StubOption;
}

export interface SkippedDependency {
  path: string;
  detectedArch: string;
}

export interface DependencyScanResult {
  compatible: string[];
  skippedIncompatible: SkippedDependency[];
}

export interface WindowsFeatureInfo {
  name: string;
  state: string;
}

export interface WindowsCapabilityInfo {
  name: string;
  state: string;
}

export interface ImageCatalog {
  sourcePath: string;
  editionIndex: number;
  fromCache: boolean;
  appxPackages: ProvisionedAppxInfo[];
  features: WindowsFeatureInfo[];
  capabilities: WindowsCapabilityInfo[];
}

export interface FeatureEnableItem {
  name: string;
  enableAll: boolean;
  customSxsPath: string | null;
}

export interface CapabilityAddItem {
  name: string;
  limitAccess: boolean;
  customSxsPath: string | null;
}

export interface AppsQueueState {
  removeFamilies: string[];
  addPackages: AddAppxRequest[];
}

export interface FeaturesQueueState {
  enableFeatures: string[];
  disableFeatures: string[];
  addCapabilities: string[];
  removeCapabilities: string[];
  enableAll: boolean;
  limitAccess: boolean;
  customSxsPath: string | null;
  featureOverrides?: Record<string, { enableAll: boolean; customSxsPath: string | null }>;
  capabilityOverrides?: Record<string, { limitAccess: boolean; customSxsPath: string | null }>;
}

export interface UpdateScanInfo {
  path: string;
  isDirectory: boolean;
  kind: "msu" | "cab" | "folder" | string;
  packageCount: number;
  totalBytes: number;
}

export type UpdateQueueItem = UpdateScanInfo;

export interface UpdatesQueueState {
  items: UpdateQueueItem[];
  runComponentCleanup: boolean;
  cleanupResetBase: boolean;
}

export interface CatalogUpdateEntry {
  updateId: string;
  title: string;
  kb: string | null;
  products: string;
  classification: string;
  lastUpdated: string;
  version: string;
  sizeText: string;
  sizeBytes: number;
}

export interface CatalogDownloadProgress {
  updateId: string;
  fileName: string;
  fileIndex: number;
  totalFiles: number;
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
}

export interface CatalogDownloadResult {
  updateId: string;
  targetDir: string;
  downloadedFiles: UpdateScanInfo[];
}

export interface DriverScanResult {
  path: string;
  isDirectory: boolean;
  infCount: number;
  sampleInfs: string[];
  totalBytes: number;
}

export interface DriverQueueItem {
  path: string;
  isDirectory: boolean;
  infCount: number;
  sampleInfs: string[];
  totalBytes: number;
  recurse: boolean;
  forceUnsigned: boolean;
  injectToInstallWim: boolean;
  injectToBootWim: boolean;
}

export interface DriversQueueState {
  items: DriverQueueItem[];
}

export interface RegPreviewOperation {
  line: number;
  kind: "create_key" | "delete_key" | "set_value" | "delete_value" | string;
  hive: string;
  hiveDisplay: string;
  targetPath: string;
  valueName: string | null;
  valueType: string | null;
  valuePreview: string | null;
  usesCurrentControlSet: boolean;
}

export interface RegFilePreview {
  path: string;
  valid: boolean;
  totalSet: number;
  totalDelete: number;
  affectedHives: string[];
  operations: RegPreviewOperation[];
  warnings: string[];
  errors: string[];
}

export interface RegQueueItem {
  path: string;
  enabled: boolean;
  preview: RegFilePreview;
}

export interface RegistryQueueState {
  regFiles: RegQueueItem[];
}

export type FileTarget = "installWim" | "isoRoot";
export type ConflictPolicy = "overwrite" | "skip";

export interface FileScanResult {
  path: string;
  isDirectory: boolean;
  fileCount: number;
  totalBytes: number;
}

export interface FileQueueItem {
  id: string;
  sourcePath: string;
  isDirectory: boolean;
  fileCount: number;
  totalBytes: number;
  target: FileTarget;
  destinationPath: string;
  conflictPolicy: ConflictPolicy;
}

export interface FilesQueueState {
  items: FileQueueItem[];
}

export interface AutounattendValidation {
  valid: boolean;
  rootElement: string | null;
  passes: string[];
  error: string | null;
}

export type OutputTarget = "iso" | "image";
export type InstallOutputFormat = "wim" | "esd" | "swm";

export interface OutputOptions {
  target: OutputTarget;
  installFormat: InstallOutputFormat;
  splitSizeMb: number;
  outputPath: string;
  autounattendPath: string | null;
  addGenericEiCfg: boolean;
  uefiNoPrompt: boolean;
  volumeLabel: string;
}

export interface BuildProgressEvent {
  step: number;
  totalSteps: number;
  phaseKey: string;
  percent: number;
  message: string;
}

export interface PresetMeta {
  name: string;
  description: string;
  createdAt: string;
  targetArch: string;
  targetEditionHint: string | null;
}

export type PresetRemoveAppx =
  | {
      packageFamilyName: string;
      displayNameHint?: string | null;
    }
  | string;

export interface PresetDriverItem {
  path: string;
  isDirectory?: boolean;
  recurse: boolean;
  forceUnsigned: boolean;
  injectToInstallWim: boolean;
  injectToBootWim: boolean;
}

export interface PresetOutput {
  target: OutputTarget;
  installFormat: InstallOutputFormat;
  splitSizeMb: number;
  autounattendPath: string | null;
  addGenericEiCfg: boolean;
  uefiNoPrompt: boolean;
  volumeLabel: string;
}

export interface StudioPreset {
  schemaVersion: string;
  meta: PresetMeta;
  apps: {
    removePackages: PresetRemoveAppx[];
    addPackages: AddAppxRequest[];
  };
  features: {
    enable: FeatureEnableItem[];
    disable: string[];
    capabilitiesAdd: CapabilityAddItem[];
    capabilitiesRemove: string[];
    defaultSxsPath: string | null;
  };
  updates: {
    items: { path: string; isDirectory: boolean }[];
    runComponentCleanup: boolean;
    cleanupResetBase: boolean;
  };
  drivers: {
    items: PresetDriverItem[];
  };
  registry: {
    regFiles: { path: string; enabled: boolean }[];
  };
  files?: {
    items: {
      sourcePath: string;
      isDirectory: boolean;
      target: FileTarget;
      destinationPath: string;
      conflictPolicy: ConflictPolicy;
    }[];
  };
  output: PresetOutput;
}

export type MissingPathKind =
  | "appxPackage"
  | "appxDependency"
  | "appxLicense"
  | "featuresSxs"
  | "updateItem"
  | "driverItem"
  | "regFile"
  | "customFile"
  | "autounattendXml";

export interface MissingPresetPath {
  id: string;
  kind: MissingPathKind;
  expectedDirectory: boolean;
  path: string;
  parentPath: string | null;
}

export interface PresetImportResult {
  preset: StudioPreset;
  missingPaths: MissingPresetPath[];
}
