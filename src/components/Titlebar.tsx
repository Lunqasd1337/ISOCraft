import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { type UnlistenFn } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import {
  Minus,
  Monitor,
  Moon,
  Settings,
  Square,
  Copy,
  Sun,
  X,
} from "lucide-react";
import { FluentSegmentedControl } from "./FluentMotion";
import type { AppLanguage, AppTheme } from "../types/studio";

interface TitlebarProps {
  language: AppLanguage;
  onChangeLanguage: (lang: AppLanguage) => void;
  theme: AppTheme;
  onCycleTheme: () => void;
  onOpenSettings: () => void;
  availableAppVersion?: string | null;
  isBusy?: boolean;
}

const LANGUAGE_OPTIONS = [
  { value: "ru", label: "RU" },
  { value: "en", label: "EN" },
] as const;

export function Titlebar({
  language,
  onChangeLanguage,
  theme,
  onCycleTheme,
  onOpenSettings,
  availableAppVersion = null,
  isBusy = false,
}: TitlebarProps) {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const [themeSpinTick, setThemeSpinTick] = useState(0);

  useEffect(() => {
    let active = true;
    let unlistenResize: UnlistenFn | null = null;
    let resizeTimer: number | null = null;
    const win = getCurrentWindow();
    win
      .isMaximized()
      .then((val) => {
        if (active) setMaximized(val);
      })
      .catch(() => {});
    win
      .onResized(() => {
        if (resizeTimer !== null) {
          window.clearTimeout(resizeTimer);
        }
        resizeTimer = window.setTimeout(() => {
          resizeTimer = null;
          win
            .isMaximized()
            .then((val) => {
              if (active) setMaximized(val);
            })
            .catch(() => {});
        }, 100);
      })
      .then((fn) => {
        if (!active) {
          fn();
        } else {
          unlistenResize = fn;
        }
      })
      .catch(() => {});
    return () => {
      active = false;
      if (resizeTimer !== null) {
        window.clearTimeout(resizeTimer);
      }
      unlistenResize?.();
    };
  }, []);

  const iconSpinClass = themeSpinTick > 0 ? "animate-fluent-icon-spin" : "";
  const themeIcon =
    theme === "light" ? (
      <Sun key={`light-${themeSpinTick}`} className={`w-4 h-4 ${iconSpinClass}`} strokeWidth={1.6} />
    ) : theme === "dark" ? (
      <Moon key={`dark-${themeSpinTick}`} className={`w-4 h-4 ${iconSpinClass}`} strokeWidth={1.6} />
    ) : (
      <Monitor key={`system-${themeSpinTick}`} className={`w-4 h-4 ${iconSpinClass}`} strokeWidth={1.6} />
    );

  const themeTitle =
    theme === "light"
      ? t("titlebar.themeLight")
      : theme === "dark"
      ? t("titlebar.themeDark")
      : t("titlebar.themeSystem");

  const handleThemeClick = () => {
    setThemeSpinTick((v) => v + 1);
    onCycleTheme();
  };

  const handleMinimize = () => {
    getCurrentWindow().minimize().catch(() => {});
  };

  const handleToggleMaximize = () => {
    getCurrentWindow().toggleMaximize().catch(() => {});
  };

  const handleClose = () => {
    getCurrentWindow().close().catch(() => {});
  };

  return (
    <header className="h-[40px] shrink-0 flex items-center justify-between bg-sidebar border-b border-border-subtle px-3 select-none">
      {/* Left: Brand + drag region */}
      <div
        data-tauri-drag-region
        className="flex items-center gap-2.5 flex-1 h-full min-w-0 cursor-default"
      >
        <svg
          viewBox="0 0 24 24"
          className="w-6 h-6 shrink-0 pointer-events-none"
          aria-hidden="true"
        >
          {/* Bottom sapphire layer */}
          <path
            d="M12 13.5L21 17.5L12 21.5L3 17.5L12 13.5Z"
            fill="#005FB8"
          />
          {/* Middle Windows blue layer */}
          <path
            d="M12 9L21 13L12 17L3 13L12 9Z"
            fill="#0078D4"
          />
          {/* Top sky-cyan glass layer */}
          <path
            d="M12 4.5L21 8.5L12 12.5L3 8.5L12 4.5Z"
            fill="#4CC2FF"
          />
        </svg>
        <span
          data-tauri-drag-region
          className="text-[13px] font-semibold tracking-tight text-text-primary truncate"
        >
          {t("app.name")}
        </span>
      </div>

      {/* Right: Controls */}
      <div className="flex items-center gap-1.5 h-full">
        {/* Language Toggle */}
        <FluentSegmentedControl
          options={LANGUAGE_OPTIONS.map((opt) => ({
            ...opt,
            disabled: isBusy && opt.value !== language,
          }))}
          value={language}
          onChange={onChangeLanguage}
          variant="accent"
          size="xs"
          ariaLabel={t("titlebar.languageAriaLabel")}
        />

        {/* Theme Button */}
        <button
          type="button"
          onClick={handleThemeClick}
          title={themeTitle}
          aria-label={themeTitle}
          className="w-8 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.95]"
        >
          {themeIcon}
        </button>

        {/* Settings Button */}
        <button
          type="button"
          onClick={onOpenSettings}
          title={
            availableAppVersion
              ? t("titlebar.settingsUpdateAvailable", {
                  version: availableAppVersion,
                })
              : t("titlebar.settings")
          }
          aria-label={t("titlebar.settings")}
          className="group relative w-8 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-all duration-150 active:scale-[0.95]"
        >
          <Settings
            className="w-4 h-4 transition-transform duration-200 group-hover:rotate-45"
            strokeWidth={1.6}
          />
          {availableAppVersion && (
            <span
              aria-hidden="true"
              className="animate-fluent-icon-pop absolute top-1 right-1 w-2 h-2 rounded-full bg-accent ring-2 ring-sidebar"
            />
          )}
        </button>

        <div className="w-px h-4 bg-border-subtle mx-1" />

        {/* Window Controls */}
        <button
          type="button"
          onClick={handleMinimize}
          title={t("titlebar.minimize")}
          aria-label={t("titlebar.minimize")}
          className="w-9 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
        >
          <Minus className="w-3.5 h-3.5" strokeWidth={1.75} />
        </button>
        <button
          type="button"
          onClick={handleToggleMaximize}
          title={maximized ? t("titlebar.restore") : t("titlebar.maximize")}
          aria-label={maximized ? t("titlebar.restore") : t("titlebar.maximize")}
          className="w-9 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
        >
          {maximized ? (
            <Copy className="w-3 h-3" strokeWidth={1.75} />
          ) : (
            <Square className="w-3 h-3" strokeWidth={1.75} />
          )}
        </button>
        <button
          type="button"
          onClick={handleClose}
          title={t("titlebar.close")}
          aria-label={t("titlebar.close")}
          className="w-9 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-white hover:bg-[#C42B1C] transition-colors"
        >
          <X className="w-4 h-4" strokeWidth={1.75} />
        </button>
      </div>
    </header>
  );
}
