import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
import type { AppLanguage, AppTheme } from "../types/studio";

interface TitlebarProps {
  language: AppLanguage;
  onChangeLanguage: (lang: AppLanguage) => void;
  theme: AppTheme;
  onCycleTheme: () => void;
  onOpenSettings: () => void;
}

export function Titlebar({
  language,
  onChangeLanguage,
  theme,
  onCycleTheme,
  onOpenSettings,
}: TitlebarProps) {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    win.isMaximized().then(setMaximized).catch(() => {});
    const unlisten = win.onResized(() => {
      win.isMaximized().then(setMaximized).catch(() => {});
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const themeIcon =
    theme === "light" ? (
      <Sun className="w-4 h-4" strokeWidth={1.6} />
    ) : theme === "dark" ? (
      <Moon className="w-4 h-4" strokeWidth={1.6} />
    ) : (
      <Monitor className="w-4 h-4" strokeWidth={1.6} />
    );

  const themeTitle =
    theme === "light"
      ? t("titlebar.themeLight")
      : theme === "dark"
      ? t("titlebar.themeDark")
      : t("titlebar.themeSystem");

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
        <div
          className="flex items-center rounded-md border border-border-subtle bg-surface p-0.5 text-[11px] font-semibold"
          role="group"
          aria-label="Language"
        >
          <button
            type="button"
            onClick={() => onChangeLanguage("ru")}
            className={`px-2 py-0.5 rounded-[4px] transition-colors ${
              language === "ru"
                ? "bg-accent text-accent-fg"
                : "text-text-secondary hover:text-text-primary"
            }`}
          >
            RU
          </button>
          <button
            type="button"
            onClick={() => onChangeLanguage("en")}
            className={`px-2 py-0.5 rounded-[4px] transition-colors ${
              language === "en"
                ? "bg-accent text-accent-fg"
                : "text-text-secondary hover:text-text-primary"
            }`}
          >
            EN
          </button>
        </div>

        {/* Theme Button */}
        <button
          type="button"
          onClick={onCycleTheme}
          title={themeTitle}
          className="w-8 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
        >
          {themeIcon}
        </button>

        {/* Settings Button */}
        <button
          type="button"
          onClick={onOpenSettings}
          title={t("titlebar.settings")}
          className="w-8 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
        >
          <Settings className="w-4 h-4" strokeWidth={1.6} />
        </button>

        <div className="w-px h-4 bg-border-subtle mx-1" />

        {/* Window Controls */}
        <button
          type="button"
          onClick={handleMinimize}
          title={t("titlebar.minimize")}
          className="w-9 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-subtle transition-colors"
        >
          <Minus className="w-3.5 h-3.5" strokeWidth={1.75} />
        </button>
        <button
          type="button"
          onClick={handleToggleMaximize}
          title={maximized ? t("titlebar.restore") : t("titlebar.maximize")}
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
          className="w-9 h-7 rounded-md flex items-center justify-center text-text-secondary hover:text-white hover:bg-[#C42B1C] transition-colors"
        >
          <X className="w-4 h-4" strokeWidth={1.75} />
        </button>
      </div>
    </header>
  );
}
