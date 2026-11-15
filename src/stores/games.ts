import { create } from "zustand";
import { api } from "@/lib/ipc";
import type { GameStatus, LogLine } from "@/lib/types";
import i18n from "@/i18n";
import { notify, useToasts } from "./toasts";

const MAX_LINES = 50_000;

interface GamesState {
  status: Record<string, GameStatus>;
  logs: Record<string, LogLine[]>;
  /** Загружен ли лог инстанса с бэкенда (после этого живём на событиях). */
  loaded: Record<string, boolean>;
  init: () => Promise<void>;
  loadLog: (id: string) => Promise<void>;
  onLog: (instanceId: string, lines: LogLine[]) => void;
  onState: (s: GameStatus, instanceName?: string) => void;
  clearView: (id: string) => void;
}

export const useGames = create<GamesState>((set, get) => ({
  status: {},
  logs: {},
  loaded: {},
  init: async () => {
    const running = await api.runningGames();
    set({ status: Object.fromEntries(running.map((s) => [s.instanceId, s])) });
  },
  loadLog: async (id) => {
    if (get().loaded[id]) return;
    const { status, lines } = await api.getGameLog(id);
    set((st) => ({
      logs: { ...st.logs, [id]: lines },
      loaded: { ...st.loaded, [id]: true },
      status: status ? { ...st.status, [id]: status } : st.status,
    }));
  },
  onLog: (id, lines) =>
    set((st) => {
      const prev = st.logs[id] ?? [];
      const next = prev.length + lines.length > MAX_LINES ? [...prev, ...lines].slice(-MAX_LINES) : [...prev, ...lines];
      return { logs: { ...st.logs, [id]: next } };
    }),
  onState: (s, instanceName) => {
    const prev = get().status[s.instanceId];
    // Новый запуск — начинаем лог с чистого листа (бэкенд пришлёт его заново).
    if (s.state === "running" && prev?.startedAt !== s.startedAt) {
      set((st) => ({ logs: { ...st.logs, [s.instanceId]: [] }, loaded: { ...st.loaded, [s.instanceId]: false } }));
      void get().loadLog(s.instanceId).catch(notify.error);
    }
    set((st) => ({ status: { ...st.status, [s.instanceId]: s } }));
    if (s.state === "crashed" && prev?.state === "running") {
      useToasts.getState().push({
        kind: "error",
        title: i18n.t("game.crashedToast", { name: instanceName ?? s.instanceId }),
        details: s.crashReport?.text.slice(0, 2000),
        action: {
          label: i18n.t("game.openLog"),
          onClick: () => (window.location.hash = `#/instance/${encodeURIComponent(s.instanceId)}/log`),
        },
      });
    }
  },
  clearView: (id) => set((st) => ({ logs: { ...st.logs, [id]: [] } })),
}));

export const isRunning = (s?: GameStatus) => s?.state === "running";
