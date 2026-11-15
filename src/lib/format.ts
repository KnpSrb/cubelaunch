import i18n from "@/i18n";
import type { Progress } from "./types";

export function formatBytes(n: number): string {
  const units = i18n.language === "ru" ? ["Б", "КБ", "МБ", "ГБ"] : ["B", "KB", "MB", "GB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

/** Доля выполнения 0..1 (по байтам, если они известны, иначе по файлам). */
export function progressRatio(p: Progress): number {
  if (p.bytesTotal > 0) return Math.min(1, p.bytesDone / p.bytesTotal);
  if (p.filesTotal > 0) return Math.min(1, p.filesDone / p.filesTotal);
  return 0;
}

export function formatDuration(secs: number): string {
  const ru = i18n.language === "ru";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return ru ? `${h} ч ${m} мин` : `${h} h ${m} min`;
  return ru ? `${m} мин` : `${m} min`;
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "";
  return new Date(iso).toLocaleDateString(i18n.language, { year: "numeric", month: "short", day: "numeric" });
}
