import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import type { AppLanguage } from "../types/studio";
import en from "./locales/en.json";
import ru from "./locales/ru.json";

export const LANGUAGE_STORAGE_KEY = "isocraft.language";

export function detectSystemLanguage(): AppLanguage {
  if (typeof navigator === "undefined") return "en";
  const candidate = navigator.languages?.[0] ?? navigator.language ?? "en";
  return candidate.toLowerCase().startsWith("ru") ? "ru" : "en";
}

export function resolveInitialLanguage(): AppLanguage {
  try {
    if (typeof window !== "undefined" && window.localStorage) {
      const cached = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
      if (cached === "ru" || cached === "en") {
        return cached;
      }
    }
  } catch {
    // ignore storage errors
  }
  return detectSystemLanguage();
}

export function persistCachedLanguage(lang: AppLanguage): void {
  try {
    if (typeof window !== "undefined" && window.localStorage) {
      window.localStorage.setItem(LANGUAGE_STORAGE_KEY, lang);
    }
  } catch {
    // ignore storage errors
  }
}

function syncDocumentLang(lang: string): void {
  if (typeof document !== "undefined") {
    document.documentElement.lang = lang === "en" ? "en" : "ru";
  }
}

const initialLanguage = resolveInitialLanguage();

i18n.use(initReactI18next).init({
  resources: {
    ru: { translation: ru },
    en: { translation: en },
  },
  lng: initialLanguage,
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

syncDocumentLang(initialLanguage);
i18n.on("languageChanged", syncDocumentLang);

export default i18n;
