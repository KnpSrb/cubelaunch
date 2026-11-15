// Единый разбор ошибок: превращает что угодно (ошибку Rust, JS-исключение, строку)
// в понятное пользователю локализованное сообщение.

import i18n from "@/i18n";
import type { BackendError } from "./types";

export function isBackendError(e: unknown): e is BackendError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export interface UserError {
  title: string;
  details?: string;
}

export function toUserError(e: unknown): UserError {
  if (isBackendError(e)) {
    const key = `errors.${e.kind}`;
    const title = i18n.exists(key) ? i18n.t(key) : e.message;
    return { title, details: e.details ?? undefined };
  }
  if (e instanceof Error) return { title: i18n.t("errors.other"), details: e.message };
  return { title: i18n.t("errors.other"), details: String(e) };
}
