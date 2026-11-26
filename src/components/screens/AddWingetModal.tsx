import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { FluentModal, useAnimatedModalClose } from "../FluentModal";
import { FluentSegmentedControl } from "../FluentMotion";
import { localizeBackendError } from "../../utils/backendErrors";
import type {
  PostSetupItem,
  WingetScope,
  WingetSearchEntry,
  WingetSearchResponse,
  WingetSource,
} from "../../types/studio";

interface AddWingetModalProps {
  existingIds: string[];
  onAddItems: (items: PostSetupItem[]) => void;
  onClose: () => void;
}

const QUICK_SEARCH_SUGGESTIONS = [
  "7-Zip",
  "Firefox",
  "Chrome",
  "VLC",
  "Notepad++",
  "VS Code",
  "Git",
  "PowerToys",
  "OBS",
  "Telegram",
] as const;

const WINGET_ID_RE = /^[A-Za-z0-9._+-]{1,128}$/;

function isValidWingetIdClient(raw: string): boolean {
  const trimmed = raw.trim();
  return Boolean(trimmed) && !trimmed.startsWith("-") && WINGET_ID_RE.test(trimmed);
}

function makeItemId(seed: string): string {
  return typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${seed}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function entryKey(entry: Pick<WingetSearchEntry, "source" | "id">): string {
  return `${entry.source}:${entry.id.toLowerCase()}`;
}

function formatIndexDate(iso: string, locale: string): string {
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return iso;
  try {
    return new Intl.DateTimeFormat(locale === "ru" ? "ru-RU" : "en-US", {
      dateStyle: "short",
      timeStyle: "short",
    }).format(parsed);
  } catch {
    return parsed.toLocaleString();
  }
}

export function AddWingetModal({
  existingIds,
  onAddItems,
  onClose,
}: AddWingetModalProps) {
  const { t, i18n } = useTranslation();

  const [searchQuery, setSearchQuery] = useState("");
  const [wingetSource, setWingetSource] = useState<WingetSource>("winget");
  const [wingetScope, setWingetScope] = useState<WingetScope>("default");

  const [results, setResults] = useState<WingetSearchEntry[]>([]);
  const [searching, setSearching] = useState(false);
  const [refreshingIndex, setRefreshingIndex] = useState(false);
  const [hasSearched, setHasSearched] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [indexUpdatedAt, setIndexUpdatedAt] = useState<string | null>(null);

  const [selectedPackages, setSelectedPackages] = useState<Map<string, WingetSearchEntry>>(
    () => new Map()
  );

  const [manualExpanded, setManualExpanded] = useState(false);
  const [wingetId, setWingetId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [extraArgs, setExtraArgs] = useState("");

  const searchSeqRef = useRef(0);
  const mountedRef = useRef(true);
  const skipNextDebounceRef = useRef(false);
  const prevSourceRef = useRef<WingetSource>(wingetSource);

  const { isClosing, requestClose } = useAnimatedModalClose(onClose);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      searchSeqRef.current += 1;
    };
  }, []);

  // Lightweight probe on mount to show cached Winget index timestamp if present
  useEffect(() => {
    let active = true;
    invoke<WingetSearchResponse>("search_winget_packages", {
      query: "",
      source: "winget",
      forceRefresh: false,
      limit: 1,
    })
      .then((res) => {
        if (!active || !mountedRef.current) return;
        if (res.indexUpdatedAt) {
          setIndexUpdatedAt(res.indexUpdatedAt);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const executeSearch = useCallback(
    async (queryText: string, source: WingetSource, forceRefresh = false) => {
      const trimmed = queryText.trim();
      if (!trimmed && !forceRefresh) {
        searchSeqRef.current += 1;
        setSearching(false);
        setRefreshingIndex(false);
        setResults([]);
        setHasSearched(false);
        setSearchError(null);
        return;
      }

      const reqId = ++searchSeqRef.current;
      setRefreshingIndex(forceRefresh);
      setSearching(true);
      setSearchError(null);

      try {
        const res = await invoke<WingetSearchResponse>("search_winget_packages", {
          query: trimmed,
          source,
          forceRefresh,
          limit: 40,
        });
        if (!mountedRef.current || searchSeqRef.current !== reqId) return;
        if (res.indexUpdatedAt) {
          setIndexUpdatedAt(res.indexUpdatedAt);
        }
        if (trimmed) {
          setResults(res.items);
          setHasSearched(true);
        } else {
          setResults([]);
          setHasSearched(false);
        }
      } catch (err) {
        if (!mountedRef.current || searchSeqRef.current !== reqId) return;
        setSearchError(String(err));
        if (trimmed) {
          setResults([]);
          setHasSearched(true);
        }
      } finally {
        if (mountedRef.current && searchSeqRef.current === reqId) {
          setSearching(false);
          setRefreshingIndex(false);
        }
      }
    },
    []
  );

  // Debounced live search when query >= 2 chars or immediate on source switch
  useEffect(() => {
    const sourceChanged = prevSourceRef.current !== wingetSource;
    prevSourceRef.current = wingetSource;

    if (skipNextDebounceRef.current && !sourceChanged) {
      skipNextDebounceRef.current = false;
      return;
    }
    skipNextDebounceRef.current = false;

    const trimmed = searchQuery.trim();
    if (!trimmed || trimmed.length < 2) {
      searchSeqRef.current += 1;
      setSearching(false);
      setRefreshingIndex(false);
      setResults([]);
      setHasSearched(false);
      setSearchError(null);
      return;
    }

    if (sourceChanged) {
      setResults([]);
      executeSearch(trimmed, wingetSource, false);
      return;
    }

    const timer = window.setTimeout(() => {
      executeSearch(trimmed, wingetSource, false);
    }, 350);

    return () => window.clearTimeout(timer);
  }, [executeSearch, searchQuery, wingetSource]);

  const existingSet = useMemo(
    () => new Set(existingIds.map((id) => id.toLowerCase())),
    [existingIds]
  );

  const togglePackage = (entry: WingetSearchEntry) => {
    const key = entryKey(entry);
    setSelectedPackages((prev) => {
      const next = new Map(prev);
      if (next.has(key)) next.delete(key);
      else next.set(key, entry);
      return next;
    });
  };

  const removeSelected = (key: string) => {
    setSelectedPackages((prev) => {
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
  };

  const handleQuickChip = (suggestion: string) => {
    skipNextDebounceRef.current = true;
    setSearchQuery(suggestion);
    executeSearch(suggestion, wingetSource, false);
  };

  const handleSearchSubmit = (e: FormEvent) => {
    e.preventDefault();
    const trimmed = searchQuery.trim();
    if (trimmed) {
      skipNextDebounceRef.current = true;
      executeSearch(trimmed, wingetSource, false);
    }
  };

  const handleRefreshIndex = () => {
    if (searching || refreshingIndex || wingetSource !== "winget") return;
    executeSearch(searchQuery, "winget", true);
  };

  const manualTrimmed = wingetId.trim();
  const manualValid = manualTrimmed.length === 0 || isValidWingetIdClient(manualTrimmed);
  const canAddManual = manualTrimmed.length > 0 && manualValid;
  const totalToAdd = useMemo(() => {
    const seen = new Set<string>();
    if (canAddManual) {
      seen.add(manualTrimmed.toLowerCase());
    }
    for (const entry of selectedPackages.values()) {
      seen.add(entry.id.toLowerCase());
    }
    return seen.size;
  }, [canAddManual, manualTrimmed, selectedPackages]);

  const previewCommand = useMemo(() => {
    const firstSelected = selectedPackages.values().next().value as
      | WingetSearchEntry
      | undefined;
    const sampleId = manualTrimmed || firstSelected?.id || "Vendor.App";
    const sampleSource = manualTrimmed
      ? wingetSource
      : firstSelected?.source ?? wingetSource;

    const parts = [
      "winget install --id",
      sampleId,
      "--exact --silent --accept-package-agreements --accept-source-agreements",
    ];
    parts.push(`--source ${sampleSource}`);
    if (wingetScope !== "default") parts.push(`--scope ${wingetScope}`);
    if (extraArgs.trim()) parts.push(extraArgs.trim());
    return parts.join(" ");
  }, [extraArgs, manualTrimmed, selectedPackages, wingetScope, wingetSource]);

  const handleSubmit = () => {
    if (totalToAdd === 0 || !manualValid) return;
    const toAdd: PostSetupItem[] = [];
    const seen = new Set<string>();

    if (canAddManual) {
      seen.add(manualTrimmed.toLowerCase());
      toAdd.push({
        id: makeItemId(manualTrimmed),
        kind: "winget",
        phase: "firstLogon",
        enabled: true,
        sourcePath: null,
        args: extraArgs.trim(),
        wingetId: manualTrimmed,
        displayName: displayName.trim() || manualTrimmed,
        wingetSource,
        wingetScope,
      });
    }

    for (const entry of selectedPackages.values()) {
      const idKey = entry.id.toLowerCase();
      if (seen.has(idKey)) continue;
      seen.add(idKey);
      toAdd.push({
        id: makeItemId(entry.id),
        kind: "winget",
        phase: "firstLogon",
        enabled: true,
        sourcePath: null,
        args: extraArgs.trim(),
        wingetId: entry.id,
        displayName: entry.name,
        wingetSource: entry.source,
        wingetScope,
      });
    }

    if (toAdd.length > 0) {
      onAddItems(toAdd);
    }
    requestClose();
  };

  const formattedIndexDate = indexUpdatedAt
    ? formatIndexDate(indexUpdatedAt, i18n.language)
    : null;
  const hasManualInputs = Boolean(manualTrimmed || extraArgs.trim());

  return (
    <FluentModal
      isClosing={isClosing}
      onRequestClose={requestClose}
      ariaLabel={t("screens.postSetup.wingetModal.title")}
      maxWidthClass="max-w-[680px]"
      panelClassName="flex flex-col max-h-[88vh]"
    >
      <div className="px-5 py-4 border-b border-border-subtle flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="text-[15.5px] font-semibold text-text-primary flex items-center gap-2">
            <Download className="w-4 h-4 text-accent shrink-0" strokeWidth={1.75} />
            <span>{t("screens.postSetup.wingetModal.title")}</span>
          </h2>
          <p className="text-[12px] text-text-secondary mt-0.5">
            {t("screens.postSetup.wingetModal.subtitle")}
          </p>
        </div>
        <button
          type="button"
          onClick={requestClose}
          aria-label={t("modals.closeAria")}
          className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.95] shrink-0"
        >
          <X className="w-4 h-4" strokeWidth={1.75} />
        </button>
      </div>

      <div className="p-5 space-y-4 overflow-y-auto overflow-x-hidden">
        {/* Search bar + refresh index */}
        <div className="space-y-1.5">
          <form onSubmit={handleSearchSubmit} className="flex items-center gap-2">
            <div className="relative flex-1 min-w-0">
              <Search
                className="w-4 h-4 text-text-secondary absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none"
                strokeWidth={1.65}
              />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={t("screens.postSetup.wingetModal.searchPlaceholder")}
                className="w-full pl-9 pr-8 py-2 rounded-md bg-canvas border border-border-subtle text-[13px] text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
              />
              {searchQuery && (
                <button
                  type="button"
                  onClick={() => setSearchQuery("")}
                  aria-label={t("screens.postSetup.wingetModal.clearSearchAria")}
                  className="w-6 h-6 rounded flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle absolute right-1.5 top-1/2 -translate-y-1/2 transition-colors duration-150"
                >
                  <X className="w-3.5 h-3.5" strokeWidth={1.75} />
                </button>
              )}
            </div>

            {wingetSource === "winget" && (
              <button
                type="button"
                onClick={handleRefreshIndex}
                disabled={searching || refreshingIndex}
                title={t("screens.postSetup.wingetModal.refreshIndexTooltip")}
                className="inline-flex items-center gap-1.5 px-3 py-2 rounded-md text-[12px] font-medium bg-canvas border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed shrink-0"
              >
                <RefreshCw
                  className={`w-3.5 h-3.5 ${refreshingIndex ? "animate-fluent-spin-loop text-accent" : ""}`}
                  strokeWidth={1.7}
                />
                <span>{t("screens.postSetup.wingetModal.refreshIndexBtn")}</span>
              </button>
            )}
          </form>

          <div className="flex flex-wrap items-center justify-between gap-2 text-[11.5px] text-text-secondary px-0.5">
            <span>
              {wingetSource === "winget"
                ? formattedIndexDate
                  ? t("screens.postSetup.wingetModal.indexUpdated", {
                      date: formattedIndexDate,
                    })
                  : t("screens.postSetup.wingetModal.indexNotLoaded")
                : t("screens.postSetup.wingetModal.msstoreHint")}
            </span>
            {hasSearched && !searching && !searchError && results.length > 0 && (
              <span className="font-medium text-text-primary">
                {t("screens.postSetup.wingetModal.resultsCount", {
                  count: results.length,
                })}
              </span>
            )}
          </div>
        </div>

        {/* Source & Scope */}
        <div className="grid grid-cols-1 sm:grid-cols-[minmax(0,200px)_minmax(0,1fr)] gap-3">
          <div className="space-y-1.5 min-w-0">
            <label className="block text-[12px] font-semibold text-text-primary">
              {t("screens.postSetup.wingetModal.sourceLabel")}
            </label>
            <FluentSegmentedControl<WingetSource>
              value={wingetSource}
              onChange={setWingetSource}
              variant="surface"
              size="sm"
              fullWidth
              options={[
                { value: "winget", label: "winget" },
                { value: "msstore", label: "msstore" },
              ]}
            />
          </div>
          <div className="space-y-1.5 min-w-0">
            <label className="block text-[12px] font-semibold text-text-primary">
              {t("screens.postSetup.wingetModal.scopeLabel")}
            </label>
            <FluentSegmentedControl<WingetScope>
              value={wingetScope}
              onChange={setWingetScope}
              variant="surface"
              size="sm"
              fullWidth
              options={[
                { value: "default", label: t("screens.postSetup.scopeDefault") },
                { value: "machine", label: t("screens.postSetup.scopeMachine") },
                { value: "user", label: t("screens.postSetup.scopeUser") },
              ]}
            />
          </div>
        </div>

        {/* Search results / empty / loading / error */}
        <div className="h-56 rounded-md border border-border-subtle bg-canvas/50 p-2.5 flex flex-col">
          {searching ? (
            <div className="flex-1 flex flex-col items-center justify-center gap-2 text-center text-text-secondary">
              <Loader2 className="w-5 h-5 text-accent animate-fluent-spin-loop" strokeWidth={1.75} />
              <span className="text-[12.5px]">
                {!indexUpdatedAt && wingetSource === "winget"
                  ? t("screens.postSetup.wingetModal.searchingIndex")
                  : t("screens.postSetup.wingetModal.searching")}
              </span>
            </div>
          ) : searchError ? (
            <div className="flex-1 flex flex-col items-center justify-center gap-2.5 text-center px-4">
              <AlertTriangle className="w-5 h-5 text-status-danger" strokeWidth={1.75} />
              <div className="text-[12px] text-status-danger max-w-md break-words">
                {localizeBackendError(searchError)}
              </div>
              <button
                type="button"
                onClick={() =>
                  searchQuery.trim()
                    ? executeSearch(searchQuery, wingetSource, false)
                    : executeSearch("", "winget", true)
                }
                className="px-3 py-1 rounded-md text-[12px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150"
              >
                {t("screens.postSetup.wingetModal.retryBtn")}
              </button>
            </div>
          ) : !searchQuery.trim() ? (
            <div className="flex-1 flex flex-col items-center justify-center gap-3 text-center px-3">
              <div className="text-[12.5px] font-medium text-text-secondary">
                {t("screens.postSetup.wingetModal.emptyPromptTitle")}
              </div>
              <div className="space-y-1.5">
                <div className="text-[11px] text-text-secondary">
                  {t("screens.postSetup.wingetModal.emptyPromptHint")}
                </div>
                <div className="flex flex-wrap items-center justify-center gap-1.5">
                  {QUICK_SEARCH_SUGGESTIONS.map((suggestion) => (
                    <button
                      key={suggestion}
                      type="button"
                      onClick={() => handleQuickChip(suggestion)}
                      className="px-2.5 py-1 rounded-full text-[11.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle hover:border-accent text-text-primary transition-all duration-150 active:scale-[0.97]"
                    >
                      {suggestion}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          ) : hasSearched && results.length === 0 ? (
            <div className="flex-1 flex items-center justify-center text-center px-4">
              <span className="text-[12.5px] text-text-secondary">
                {t("screens.postSetup.wingetModal.noResults", {
                  query: searchQuery.trim(),
                })}
              </span>
            </div>
          ) : (
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5 overflow-y-auto overflow-x-hidden pr-1 content-start">
              {results.map((entry) => {
                const key = entryKey(entry);
                const checked = selectedPackages.has(key);
                const alreadyQueued = existingSet.has(entry.id.toLowerCase());
                return (
                  <button
                    key={key}
                    type="button"
                    onClick={() => togglePackage(entry)}
                    className={`flex items-center justify-between gap-2 px-3 py-2 rounded-md border text-left transition-all duration-150 active:scale-[0.99] min-w-0 ${
                      checked
                        ? "bg-accent/10 border-accent text-text-primary"
                        : "bg-surface border-border-subtle hover:bg-subtle text-text-primary"
                    }`}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="text-[12.5px] font-medium truncate">
                        {entry.name}
                      </div>
                      <div className="text-[11px] font-mono text-text-secondary truncate">
                        {entry.id}
                      </div>
                      {entry.publisher && (
                        <div className="text-[10.5px] text-text-secondary truncate">
                          {entry.publisher}
                        </div>
                      )}
                    </div>
                    <div className="flex items-center gap-1.5 shrink-0">
                      {entry.version && (
                        <span className="px-1.5 py-0.5 rounded text-[10px] font-mono bg-subtle text-text-secondary max-w-[84px] truncate">
                          {entry.version}
                        </span>
                      )}
                      {alreadyQueued && (
                        <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-subtle text-text-secondary">
                          {t("screens.postSetup.wingetModal.inQueue")}
                        </span>
                      )}
                      {checked && (
                        <Check
                          className="animate-fluent-icon-pop w-4 h-4 text-accent"
                          strokeWidth={2}
                        />
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </div>

        {/* Selected packages pill bar */}
        {selectedPackages.size > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[11.5px] font-semibold text-text-secondary mr-0.5">
              {t("screens.postSetup.wingetModal.selectedLabel")}
            </span>
            {Array.from(selectedPackages.entries()).map(([key, entry]) => (
              <span
                key={key}
                className="inline-flex items-center gap-1 pl-2.5 pr-1.5 py-0.5 rounded-full text-[11.5px] font-medium bg-accent/10 border border-accent text-text-primary max-w-[220px]"
              >
                <span className="truncate">{entry.name}</span>
                <button
                  type="button"
                  onClick={() => removeSelected(key)}
                  aria-label={t("screens.postSetup.wingetModal.removeSelectedAria", {
                    name: entry.name,
                  })}
                  className="w-4 h-4 rounded-full flex items-center justify-center hover:bg-subtle text-text-secondary hover:text-text-primary transition-colors duration-150 shrink-0"
                >
                  <X className="w-3 h-3" strokeWidth={2} />
                </button>
              </span>
            ))}
          </div>
        )}

        {/* Collapsible manual ID & extra arguments */}
        <div className="rounded-md border border-border-subtle bg-canvas/50">
          <button
            type="button"
            onClick={() => setManualExpanded((prev) => !prev)}
            aria-expanded={manualExpanded}
            className="w-full px-3 py-2 flex items-center justify-between gap-2 text-[12px] font-semibold text-text-primary hover:bg-subtle transition-colors duration-150 rounded-md"
          >
            <span className="flex items-center gap-1.5 min-w-0">
              {manualExpanded ? (
                <ChevronDown className="w-3.5 h-3.5 text-text-secondary shrink-0" strokeWidth={1.75} />
              ) : (
                <ChevronRight className="w-3.5 h-3.5 text-text-secondary shrink-0" strokeWidth={1.75} />
              )}
              <span className="truncate">
                {t("screens.postSetup.wingetModal.manualSectionToggle")}
              </span>
            </span>
            {hasManualInputs && (
              <span className="w-2 h-2 rounded-full bg-accent shrink-0" />
            )}
          </button>

          {manualExpanded && (
            <div className="px-3 pb-3 pt-2 border-t border-border-subtle space-y-3">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div className="space-y-1.5 min-w-0">
                  <label className="block text-[12px] font-semibold text-text-primary">
                    {t("screens.postSetup.wingetModal.idLabel")}
                  </label>
                  <input
                    type="text"
                    value={wingetId}
                    onChange={(e) => setWingetId(e.target.value)}
                    placeholder={t("screens.postSetup.wingetModal.idPlaceholder")}
                    className={`w-full px-3 py-1.5 rounded-md bg-canvas border text-[12.5px] font-mono text-text-primary focus:outline-none transition-colors duration-150 ${
                      manualValid
                        ? "border-border-subtle focus:border-accent"
                        : "border-status-danger"
                    }`}
                  />
                  {!manualValid && (
                    <div className="text-[11.5px] text-status-danger">
                      {t("screens.postSetup.wingetModal.invalidIdHint")}
                    </div>
                  )}
                </div>
                <div className="space-y-1.5 min-w-0">
                  <label className="block text-[12px] font-semibold text-text-primary">
                    {t("screens.postSetup.wingetModal.nameLabel")}
                  </label>
                  <input
                    type="text"
                    value={displayName}
                    onChange={(e) => setDisplayName(e.target.value)}
                    placeholder={t("screens.postSetup.wingetModal.namePlaceholder")}
                    className="w-full px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
                  />
                </div>
              </div>

              <div className="space-y-1.5">
                <label className="block text-[12px] font-semibold text-text-primary">
                  {t("screens.postSetup.wingetModal.extraArgsLabel")}
                </label>
                <input
                  type="text"
                  value={extraArgs}
                  onChange={(e) => setExtraArgs(e.target.value)}
                  placeholder={t("screens.postSetup.wingetModal.extraArgsPlaceholder")}
                  className="w-full px-3 py-1.5 rounded-md bg-canvas border border-border-subtle text-[12.5px] font-mono text-text-primary focus:outline-none focus:border-accent transition-colors duration-150"
                />
              </div>
            </div>
          )}
        </div>

        {/* Live command preview */}
        <div className="px-3 py-2 rounded-md bg-canvas border border-border-subtle space-y-1">
          <div className="text-[11px] font-semibold text-text-secondary uppercase tracking-wide">
            {t("screens.postSetup.wingetModal.cmdPreviewLabel")}
          </div>
          <div className="text-[11.5px] font-mono text-text-primary break-all">
            {previewCommand}
          </div>
        </div>
      </div>

      <div className="px-5 py-3.5 border-t border-border-subtle bg-canvas flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={requestClose}
          className="px-3.5 py-1.5 rounded-md text-[12.5px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98]"
        >
          {t("screens.postSetup.wingetModal.cancel")}
        </button>
        <button
          type="button"
          disabled={totalToAdd === 0 || !manualValid}
          onClick={handleSubmit}
          className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-md text-[12.5px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Plus className="w-3.5 h-3.5" strokeWidth={1.7} />
          <span>
            {t("screens.postSetup.wingetModal.addBtn", { count: totalToAdd })}
          </span>
        </button>
      </div>
    </FluentModal>
  );
}
