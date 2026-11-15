import { create } from "zustand";
import { toUserError } from "@/lib/errors";

export type ToastKind = "info" | "success" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  details?: string;
  action?: { label: string; onClick: () => void };
}

interface ToastState {
  toasts: Toast[];
  push: (t: Omit<Toast, "id">) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToasts = create<ToastState>((set, get) => ({
  toasts: [],
  push: (t) => {
    const id = nextId++;
    set({ toasts: [...get().toasts, { ...t, id }] });
    // Ошибки висят дольше, чтобы успеть прочитать и скопировать подробности.
    setTimeout(() => get().dismiss(id), t.kind === "error" ? 12000 : 3500);
  },
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}));

export const notify = {
  success: (title: string) => useToasts.getState().push({ kind: "success", title }),
  info: (title: string) => useToasts.getState().push({ kind: "info", title }),
  /** Единая точка показа ошибок пользователю. */
  error: (e: unknown, action?: Toast["action"]) => {
    console.error(e);
    useToasts.getState().push({ kind: "error", ...toUserError(e), action });
  },
};
