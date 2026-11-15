import { create } from "zustand";
import { api } from "@/lib/ipc";
import type { Settings } from "@/lib/types";
import i18n from "@/i18n";
import { applyAppearance } from "@/app/theme";

interface SettingsState {
  /** Сохранённые настройки (как на диске). */
  settings: Settings | null;
  load: () => Promise<void>;
  save: (s: Settings) => Promise<Settings>;
  reset: () => Promise<Settings>;
  /** Применить настройки к UI без сохранения (живой предпросмотр). */
  preview: (s: Settings) => void;
  /** Принять настройки, пришедшие из события бэкенда. */
  receive: (s: Settings) => void;
}

function applyToUi(s: Settings) {
  applyAppearance(s.appearance);
  if (i18n.language !== s.language) void i18n.changeLanguage(s.language);
  document.documentElement.lang = s.language;
}

export const useSettings = create<SettingsState>((set) => ({
  settings: null,
  load: async () => {
    const s = await api.getSettings();
    applyToUi(s);
    set({ settings: s });
  },
  save: async (s) => {
    const saved = await api.saveSettings(s);
    applyToUi(saved);
    set({ settings: saved });
    return saved;
  },
  reset: async () => {
    const s = await api.resetSettings();
    applyToUi(s);
    set({ settings: s });
    return s;
  },
  preview: applyToUi,
  receive: (s) => {
    applyToUi(s);
    set({ settings: s });
  },
}));
