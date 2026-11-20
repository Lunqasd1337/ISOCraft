import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  FileCode2,
  Plus,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import { EmptyStateCard, ScreenHeader } from "./ScreenShell";
import type {
  RegFilePreview,
  RegQueueItem,
  RegistryQueueState,
} from "../../types/studio";

interface RegistryScreenProps {
  hasValidImage: boolean;
  registryQueue: RegistryQueueState;
  onChangeRegistryQueue: (next: RegistryQueueState) => void;
}

export function RegistryScreen({
  hasValidImage,
  registryQueue,
  onChangeRegistryQueue,
}: RegistryScreenProps) {
  const { t } = useTranslation();
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);

  const handleAddRegFiles = async () => {
    setScanError(null);
    const selected = await open({
      multiple: true,
      filters: [{ name: "Registry Files", extensions: ["reg"] }],
    });
    if (!Array.isArray(selected) || selected.length === 0) return;

    const added: RegQueueItem[] = [];
    for (const p of selected) {
      try {
        const preview = await invoke<RegFilePreview>("preview_reg_file", {
          path: p,
        });
        added.push({
          path: p,
          enabled: true,
          preview,
        });
      } catch (err) {
        setScanError(t("screens.registry.scanError", { error: String(err) }));
      }
    }

    if (added.length > 0) {
      const existing = new Map(
        registryQueue.regFiles.map((item) => [item.path.toLowerCase(), item])
      );
      for (const item of added) {
        existing.set(item.path.toLowerCase(), item);
      }
      onChangeRegistryQueue({
        regFiles: Array.from(existing.values()),
      });
      setSelectedPath(added[added.length - 1].path);
    }
  };

  const moveItem = (index: number, delta: -1 | 1) => {
    const nextIdx = index + delta;
    if (nextIdx < 0 || nextIdx >= registryQueue.regFiles.length) return;
    const copy = [...registryQueue.regFiles];
    const [item] = copy.splice(index, 1);
    copy.splice(nextIdx, 0, item);
    onChangeRegistryQueue({ regFiles: copy });
  };

  const toggleEnabled = (path: string) => {
    onChangeRegistryQueue({
      regFiles: registryQueue.regFiles.map((r) =>
        r.path === path ? { ...r, enabled: !r.enabled } : r
      ),
    });
  };

  const removeItem = (path: string) => {
    const nextFiles = registryQueue.regFiles.filter((r) => r.path !== path);
    onChangeRegistryQueue({ regFiles: nextFiles });
    if (selectedPath === path) {
      setSelectedPath(nextFiles[0]?.path ?? null);
    }
  };

  const activeItem =
    registryQueue.regFiles.find((r) => r.path === selectedPath) ??
    registryQueue.regFiles[0] ??
    null;

  const opLabel = (kind: string) => {
    switch (kind) {
      case "create_key":
        return t("screens.registry.opCreateKey");
      case "delete_key":
        return t("screens.registry.opDeleteKey");
      case "set_value":
        return t("screens.registry.opSetValue");
      case "delete_value":
        return t("screens.registry.opDeleteValue");
      default:
        return kind;
    }
  };

  const opBadgeClass = (kind: string) => {
    if (kind === "delete_key" || kind === "delete_value") {
      return "bg-status-danger-bg text-status-danger";
    }
    if (kind === "set_value") {
      return "bg-status-success-bg text-status-success";
    }
    return "bg-subtle text-text-secondary";
  };

  return (
    <div className="space-y-4">
      <ScreenHeader
        title={t("screens.registry.title")}
        subtitle={t("screens.registry.subtitle")}
        actions={
          <button
            type="button"
            disabled={!hasValidImage}
            onClick={handleAddRegFiles}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Plus className="w-4 h-4" strokeWidth={1.7} />
            <span>{t("screens.registry.addBtn")}</span>
          </button>
        }
      />

      {!hasValidImage ? (
        <EmptyStateCard
          icon={FileCode2}
          title={t("screens.registry.emptyTitle")}
          description={t("screens.registry.emptyNeedImage")}
        />
      ) : (
        <>
          {scanError && (
            <div className="animate-fluent-item flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg bg-status-danger-bg text-status-danger text-[12.5px]">
              <div className="flex items-center gap-2 min-w-0">
                <AlertTriangle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                <span className="break-words">{scanError}</span>
              </div>
              <button
                type="button"
                onClick={() => setScanError(null)}
                className="p-1 rounded hover:bg-subtle shrink-0"
              >
                <X className="w-3.5 h-3.5" strokeWidth={1.7} />
              </button>
            </div>
          )}

          {registryQueue.regFiles.length === 0 ? (
            <EmptyStateCard
              icon={FileCode2}
              title={t("screens.registry.emptyTitle")}
              description={t("screens.registry.emptyDesc")}
              action={
                <button
                  type="button"
                  onClick={handleAddRegFiles}
                  className="inline-flex items-center gap-1.5 px-4 py-2 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-colors"
                >
                  <Plus className="w-4 h-4" strokeWidth={1.7} />
                  <span>{t("screens.registry.addBtn")}</span>
                </button>
              }
            />
          ) : (
            <>
              {/* Queue list */}
              <div className="bg-surface border border-border-subtle rounded-lg divide-y divide-border-subtle overflow-hidden">
                {registryQueue.regFiles.map((item, idx) => {
                  const baseName = item.path.split(/[\\/]/).pop() || item.path;
                  const isSelected = activeItem?.path === item.path;

                  return (
                    <div
                      key={item.path}
                      onClick={() => setSelectedPath(item.path)}
                      className={`animate-fluent-item px-4 py-3 flex items-center justify-between gap-3 cursor-pointer transition-colors ${
                        isSelected ? "bg-subtle" : "hover:bg-subtle"
                      } ${!item.enabled ? "opacity-60" : ""}`}
                    >
                      <div className="flex items-start gap-3 min-w-0">
                        <input
                          type="checkbox"
                          checked={item.enabled}
                          title={t("screens.registry.enableToggle")}
                          onClick={(e) => e.stopPropagation()}
                          onChange={() => toggleEnabled(item.path)}
                          className="mt-1 accent-accent shrink-0"
                        />
                        <div className="min-w-0 space-y-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="text-[13.5px] font-semibold text-text-primary truncate">
                              {baseName}
                            </span>
                            <span className="px-1.5 py-0.5 rounded text-[11px] font-mono bg-subtle text-text-secondary shrink-0">
                              {t("screens.registry.statsBadge", {
                                set: item.preview.totalSet,
                                del: item.preview.totalDelete,
                              })}
                            </span>
                            {!item.preview.valid && (
                              <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-danger-bg text-status-danger shrink-0">
                                <XCircle className="w-3 h-3" strokeWidth={1.75} />
                                {t("screens.registry.syntaxErrorBadge")}
                              </span>
                            )}
                            {item.preview.warnings.length > 0 && (
                              <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] font-medium bg-status-warning-bg text-status-warning shrink-0">
                                <AlertTriangle className="w-3 h-3" strokeWidth={1.75} />
                                {t("screens.registry.warningsBadge", {
                                  count: item.preview.warnings.length,
                                })}
                              </span>
                            )}
                          </div>
                          <div className="text-[11.5px] font-mono text-text-secondary truncate">
                            {item.path}
                          </div>
                          {item.preview.affectedHives.length > 0 && (
                            <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
                              {item.preview.affectedHives.map((h) => (
                                <span
                                  key={h}
                                  className="px-1.5 py-0.5 rounded text-[11px] font-mono bg-canvas border border-border-subtle text-text-secondary"
                                >
                                  {h}
                                </span>
                              ))}
                            </div>
                          )}
                        </div>
                      </div>

                      <div
                        className="flex items-center gap-1 shrink-0"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <button
                          type="button"
                          disabled={idx === 0}
                          onClick={() => moveItem(idx, -1)}
                          title={t("screens.registry.moveUp")}
                          className="p-1.5 rounded hover:bg-canvas text-text-secondary hover:text-text-primary disabled:opacity-35 disabled:cursor-not-allowed"
                        >
                          <ArrowUp className="w-3.5 h-3.5" strokeWidth={1.7} />
                        </button>
                        <button
                          type="button"
                          disabled={idx === registryQueue.regFiles.length - 1}
                          onClick={() => moveItem(idx, 1)}
                          title={t("screens.registry.moveDown")}
                          className="p-1.5 rounded hover:bg-canvas text-text-secondary hover:text-text-primary disabled:opacity-35 disabled:cursor-not-allowed"
                        >
                          <ArrowDown className="w-3.5 h-3.5" strokeWidth={1.7} />
                        </button>
                        <button
                          type="button"
                          onClick={() => removeItem(item.path)}
                          title={t("screens.registry.remove")}
                          className="p-1.5 rounded hover:bg-status-danger-bg text-text-secondary hover:text-status-danger transition-colors"
                        >
                          <Trash2 className="w-3.5 h-3.5" strokeWidth={1.6} />
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>

              {/* Dry-run Preview panel */}
              {activeItem && (
                <div
                  key={activeItem.path}
                  className="animate-fluent-item bg-surface border border-border-subtle rounded-lg overflow-hidden"
                >
                  <div className="px-4 py-3 border-b border-border-subtle flex flex-wrap items-center justify-between gap-2 bg-sidebar">
                    <div className="text-[13.5px] font-semibold text-text-primary">
                      {t("screens.registry.previewTitle")} —{" "}
                      <span className="font-mono text-[12.5px] font-normal text-text-secondary">
                        {activeItem.path.split(/[\\/]/).pop()}
                      </span>
                    </div>
                    <span className="text-[12px] font-mono text-text-secondary">
                      {t("screens.registry.statsBadge", {
                        set: activeItem.preview.totalSet,
                        del: activeItem.preview.totalDelete,
                      })}
                    </span>
                  </div>

                  <div className="p-4 space-y-3">
                    {activeItem.preview.errors.length > 0 && (
                      <div className="rounded-md bg-status-danger-bg text-status-danger p-3 space-y-1 text-[12px]">
                        <div className="font-semibold flex items-center gap-1.5">
                          <XCircle className="w-4 h-4 shrink-0" strokeWidth={1.75} />
                          {t("screens.registry.errorsTitle")}
                        </div>
                        <ul className="list-disc pl-5 space-y-0.5 font-mono">
                          {activeItem.preview.errors.map((err, i) => (
                            <li key={i}>{err}</li>
                          ))}
                        </ul>
                      </div>
                    )}

                    {activeItem.preview.warnings.length > 0 && (
                      <div className="rounded-md bg-status-warning-bg text-status-warning p-3 space-y-1 text-[12px]">
                        <div className="font-semibold flex items-center gap-1.5">
                          <AlertTriangle
                            className="w-4 h-4 shrink-0"
                            strokeWidth={1.75}
                          />
                          {t("screens.registry.warningsTitle")}
                        </div>
                        <ul className="list-disc pl-5 space-y-0.5 font-mono">
                          {activeItem.preview.warnings.map((w, i) => (
                            <li key={i}>{w}</li>
                          ))}
                        </ul>
                      </div>
                    )}

                    {activeItem.preview.operations.length === 0 ? (
                      <div className="text-[12.5px] text-text-secondary py-4 text-center">
                        {t("screens.registry.previewEmptyOps")}
                      </div>
                    ) : (
                      <div className="max-h-[320px] overflow-y-auto border border-border-subtle rounded-md divide-y divide-border-subtle">
                        <div className="grid grid-cols-12 gap-2 px-3 py-1.5 bg-sidebar text-[11px] font-semibold text-text-secondary sticky top-0 border-b border-border-subtle">
                          <div className="col-span-2">{t("screens.registry.colOp")}</div>
                          <div className="col-span-3">{t("screens.registry.colHive")}</div>
                          <div className="col-span-4">{t("screens.registry.colPath")}</div>
                          <div className="col-span-3">{t("screens.registry.colValue")}</div>
                        </div>
                        {activeItem.preview.operations.map((op, i) => {
                          const valLabel =
                            op.valueName === null
                              ? ""
                              : op.valueName === ""
                              ? t("screens.registry.defaultValueName")
                              : op.valueName;

                          return (
                            <div
                              key={`${op.line}-${i}`}
                              className="grid grid-cols-12 gap-2 px-3 py-2 text-[11.5px] items-start hover:bg-subtle"
                            >
                              <div className="col-span-2">
                                <span
                                  className={`inline-block px-1.5 py-0.5 rounded text-[10.5px] font-medium ${opBadgeClass(
                                    op.kind
                                  )}`}
                                >
                                  {opLabel(op.kind)}
                                </span>
                              </div>
                              <div className="col-span-3 font-mono text-text-secondary break-all">
                                {op.hiveDisplay}
                              </div>
                              <div className="col-span-4 font-mono text-text-primary break-all">
                                <div>{op.targetPath || "(root)"}</div>
                                {valLabel && (
                                  <div className="text-text-secondary">{valLabel}</div>
                                )}
                              </div>
                              <div className="col-span-3 font-mono text-text-secondary break-all">
                                {op.valueType && (
                                  <span className="text-[10.5px] opacity-80 block">
                                    {op.valueType}
                                  </span>
                                )}
                                {op.valuePreview && (
                                  <span className="text-text-primary">
                                    {op.valuePreview}
                                  </span>
                                )}
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    )}
                  </div>
                </div>
              )}
            </>
          )}
        </>
      )}
    </div>
  );
}
