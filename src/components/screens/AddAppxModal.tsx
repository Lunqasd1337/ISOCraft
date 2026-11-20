import { useState } from "react";
import { useTranslation } from "react-i18next";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, FolderOpen, Plus, Trash2, X } from "lucide-react";
import type {
  AddAppxRequest,
  DependencyScanResult,
  SkippedDependency,
  StubOption,
} from "../../types/studio";

interface AddAppxModalProps {
  onAdd: (req: AddAppxRequest) => void;
  onClose: () => void;
}

export function AddAppxModal({ onAdd, onClose }: AddAppxModalProps) {
  const { t } = useTranslation();
  const [packagePath, setPackagePath] = useState("");
  const [dependencyPaths, setDependencyPaths] = useState<string[]>([]);
  const [skippedDeps, setSkippedDeps] = useState<SkippedDependency[]>([]);
  const [skipLicense, setSkipLicense] = useState(true);
  const [licensePath, setLicensePath] = useState("");
  const [stubOption, setStubOption] = useState<StubOption>("None");
  const [regionAll, setRegionAll] = useState(true);
  const { isClosing, requestClose } = useAnimatedModalClose(onClose);

  const handleBrowsePackage = async () => {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "AppX / MSIX",
          extensions: ["appx", "appxbundle", "msix", "msixbundle"],
        },
      ],
    });
    if (typeof selected === "string" && selected.trim()) {
      setPackagePath(selected);
    }
  };

  const scanAndMergeDeps = async (rawPaths: string[]) => {
    if (rawPaths.length === 0) return;
    try {
      const scan = await invoke<DependencyScanResult>("scan_appx_dependencies", {
        paths: rawPaths,
      });
      setDependencyPaths((prev) =>
        Array.from(new Set([...prev, ...scan.compatible])).sort()
      );
      if (scan.skippedIncompatible.length > 0) {
        setSkippedDeps(scan.skippedIncompatible);
      }
    } catch {
      // ignore
    }
  };

  const handleAddDepFiles = async () => {
    const selected = await open({
      multiple: true,
      filters: [
        {
          name: "AppX / MSIX Dependencies",
          extensions: ["appx", "appxbundle", "msix", "msixbundle"],
        },
      ],
    });
    if (Array.isArray(selected) && selected.length > 0) {
      await scanAndMergeDeps(selected);
    }
  };

  const handleAddDepFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (typeof selected === "string" && selected.trim()) {
      await scanAndMergeDeps([selected]);
    }
  };

  const handleBrowseLicense = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "License XML", extensions: ["xml"] }],
    });
    if (typeof selected === "string" && selected.trim()) {
      setLicensePath(selected);
    }
  };

  const handleSubmit = () => {
    if (!packagePath.trim()) return;
    onAdd({
      packagePath: packagePath.trim(),
      dependencyPaths,
      licensePath: !skipLicense && licensePath.trim() ? licensePath.trim() : null,
      skipLicense,
      regionAll,
      stubOption,
    });
    requestClose();
  };

  const canSubmit =
    packagePath.trim().length > 0 && (skipLicense || licensePath.trim().length > 0);

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestClose}
      maxWidthClass="max-w-[600px]"
      panelClassName="flex flex-col max-h-[85vh]"
    >
      <div className="px-5 py-4 border-b border-border-subtle flex items-center justify-between">
          <h2 className="text-[15.5px] font-semibold text-text-primary">
            {t("screens.apps.modal.title")}
          </h2>
          <button
            type="button"
            onClick={requestClose}
            className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
          >
            <X className="w-4 h-4" strokeWidth={1.75} />
          </button>
        </div>

        <div className="p-5 space-y-4 overflow-y-auto">
          {/* Package file */}
          <div className="space-y-1.5">
            <label className="block text-[12.5px] font-semibold text-text-primary">
              {t("screens.apps.modal.packageLabel")}
            </label>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={packagePath}
                placeholder={t("screens.apps.modal.packagePlaceholder")}
                onChange={(e) => setPackagePath(e.target.value)}
                className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary focus:outline-none focus:border-accent"
              />
              <button
                type="button"
                onClick={handleBrowsePackage}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary shrink-0"
              >
                <FolderOpen className="w-3.5 h-3.5" strokeWidth={1.6} />
                {t("screens.apps.modal.browse")}
              </button>
            </div>
          </div>

          {/* Dependencies */}
          <div className="space-y-2">
            <div className="flex items-center justify-between gap-2">
              <label className="text-[12.5px] font-semibold text-text-primary">
                {t("screens.apps.modal.depsLabel")}
              </label>
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={handleAddDepFiles}
                  className="inline-flex items-center gap-1 px-2.5 py-1 rounded text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary"
                >
                  <Plus className="w-3 h-3" strokeWidth={1.7} />
                  {t("screens.apps.modal.addDepFiles")}
                </button>
                <button
                  type="button"
                  onClick={handleAddDepFolder}
                  className="inline-flex items-center gap-1 px-2.5 py-1 rounded text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary"
                >
                  <FolderOpen className="w-3 h-3" strokeWidth={1.6} />
                  {t("screens.apps.modal.addDepFolder")}
                </button>
              </div>
            </div>

            {dependencyPaths.length === 0 ? (
              <div className="px-3 py-2.5 rounded-md bg-subtle border border-border-subtle text-[12px] text-text-secondary">
                {t("screens.apps.modal.noDeps")}
              </div>
            ) : (
              <div className="max-h-28 overflow-y-auto rounded-md border border-border-subtle divide-y divide-border-subtle">
                {dependencyPaths.map((dep) => (
                  <div
                    key={dep}
                    className="flex items-center justify-between gap-2 px-3 py-1.5 text-[12px] font-mono text-text-primary"
                  >
                    <span className="truncate">{dep}</span>
                    <button
                      type="button"
                      onClick={() =>
                        setDependencyPaths((prev) => prev.filter((p) => p !== dep))
                      }
                      className="text-text-secondary hover:text-status-danger shrink-0"
                    >
                      <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                    </button>
                  </div>
                ))}
              </div>
            )}

            {skippedDeps.length > 0 && (
              <div className="animate-fluent-item px-3 py-2 rounded-md bg-status-warning-bg text-status-warning text-[12px] space-y-1">
                <div className="flex items-center gap-1.5 font-semibold">
                  <AlertTriangle className="w-3.5 h-3.5 shrink-0" strokeWidth={1.75} />
                  <span>
                    {t("screens.apps.modal.skippedDepsWarning", {
                      count: skippedDeps.length,
                    })}
                  </span>
                </div>
                <div className="font-mono text-[11px] opacity-90 truncate">
                  {skippedDeps
                    .map((s) => `${s.path.split(/[\\/]/).pop()} (${s.detectedArch})`)
                    .join(", ")}
                </div>
              </div>
            )}
          </div>

          {/* License */}
          <div className="space-y-2">
            <label className="block text-[12.5px] font-semibold text-text-primary">
              {t("screens.apps.modal.licenseLabel")}
            </label>
            <div className="flex items-center gap-4 text-[12.5px]">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  checked={skipLicense}
                  onChange={() => setSkipLicense(true)}
                  className="accent-accent"
                />
                <span>{t("screens.apps.modal.skipLicense")}</span>
              </label>
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  checked={!skipLicense}
                  onChange={() => setSkipLicense(false)}
                  className="accent-accent"
                />
                <span>{t("screens.apps.modal.useLicenseFile")}</span>
              </label>
            </div>
            {!skipLicense && (
              <div className="animate-fluent-item flex items-center gap-2">
                <input
                  type="text"
                  value={licensePath}
                  placeholder={t("screens.apps.modal.licensePlaceholder")}
                  onChange={(e) => setLicensePath(e.target.value)}
                  className="flex-1 px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary"
                />
                <button
                  type="button"
                  onClick={handleBrowseLicense}
                  className="px-3 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary shrink-0"
                >
                  {t("screens.apps.modal.browse")}
                </button>
              </div>
            )}
          </div>

          {/* Stub & Region */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 items-center">
            <div className="space-y-1">
              <label className="block text-[12.5px] font-semibold text-text-primary">
                {t("screens.apps.modal.stubLabel")}
              </label>
              <select
                value={stubOption}
                onChange={(e) => setStubOption(e.target.value as StubOption)}
                className="w-full px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] text-text-primary"
              >
                <option value="None">{t("screens.apps.modal.stubNone")}</option>
                <option value="UseStub">{t("screens.apps.modal.stubUseStub")}</option>
                <option value="UsePreference">
                  {t("screens.apps.modal.stubUsePreference")}
                </option>
              </select>
            </div>

            <label className="flex items-center gap-2 text-[12.5px] text-text-primary cursor-pointer pt-4">
              <input
                type="checkbox"
                checked={regionAll}
                onChange={(e) => setRegionAll(e.target.checked)}
                className="accent-accent"
              />
              <span>{t("screens.apps.modal.regionAll")}</span>
            </label>
          </div>
        </div>

        <div className="px-5 py-3.5 border-t border-border-subtle bg-sidebar flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={requestClose}
            className="px-4 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.98]"
          >
            {t("screens.apps.cancel")}
          </button>
          <button
            type="button"
            onClick={handleSubmit}
            disabled={!canSubmit}
            className="px-4 py-1.5 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50"
          >
            {t("screens.apps.modal.add")}
          </button>
        </div>
      </div>
    </FluentModal>
  );
}
