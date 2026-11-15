// Применение темы и акцентного цвета к документу.

import type { Settings } from "@/lib/types";

const media = window.matchMedia("(prefers-color-scheme: dark)");

/** Цвет текста поверх акцента: белый или почти чёрный — что контрастнее. */
function contrastText(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return lum > 0.4 ? "#111317" : "#ffffff";
}

export function applyAppearance(a: Settings["appearance"]) {
  const root = document.documentElement;
  const dark = a.theme === "dark" || (a.theme === "system" && media.matches);
  root.dataset.theme = dark ? "dark" : "light";
  if (/^#[0-9a-fA-F]{6}$/.test(a.accent)) {
    root.style.setProperty("--accent", a.accent);
    root.style.setProperty("--accent-fg", contrastText(a.accent));
  }
}

/** Подписка на смену системной темы; возвращает функцию отписки. */
export function watchSystemTheme(onChange: () => void): () => void {
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}

export const ACCENT_PRESETS = [
  "#4caf50",
  "#2e9bf0",
  "#7c5cff",
  "#e5484d",
  "#f2994a",
  "#e9b308",
  "#14b8a6",
  "#ec4899",
];
