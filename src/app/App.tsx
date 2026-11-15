import { useEffect } from "react";
import { HashRouter, Route, Routes } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AppShell } from "@/components/layout/AppShell";
import { InstancesPage } from "@/features/instances/InstancesPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { DownloadsPage } from "@/features/downloads/DownloadsPage";
import { useSettings } from "@/stores/settings";
import { useJobs } from "@/stores/jobs";
import { notify } from "@/stores/toasts";
import { listen } from "@/lib/ipc";
import { EVENTS } from "@/lib/events";
import type { GameStatus, Instance, JobInfo, LogLine, Settings } from "@/lib/types";
import { useGames } from "@/stores/games";
import { InstancePage } from "@/features/instances/InstancePage";
import { applyAppearance, watchSystemTheme } from "./theme";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 60_000, retry: 1, refetchOnWindowFocus: false },
    mutations: { onError: (e) => notify.error(e) },
  },
});

function useBootstrap() {
  const { settings, load, receive } = useSettings();

  useEffect(() => {
    load().catch(notify.error);
  }, [load]);

  // Перерисовка при смене системной темы (для режима «Как в системе»).
  useEffect(
    () =>
      watchSystemTheme(() => {
        const s = useSettings.getState().settings;
        if (s) applyAppearance(s.appearance);
      }),
    [],
  );

  // События бэкенда: настройки, задачи, список инстансов.
  useEffect(() => {
    void useJobs.getState().init().catch(notify.error);
    void useGames.getState().init().catch(notify.error);
    const instanceName = (id: string) =>
      queryClient.getQueryData<Instance[]>(["instances"])?.find((i) => i.id === id)?.name;
    const subs = [
      listen<Settings>(EVENTS.settingsChanged, receive),
      listen<JobInfo>(EVENTS.jobUpdate, (j) => {
        useJobs.getState().upsert(j);
        // Скачанная Java сразу появляется в списках.
        if (j.state === "done" && (j.kind === "downloadJava" || j.kind === "launch"))
          void queryClient.invalidateQueries({ queryKey: ["java"] });
      }),
      listen(EVENTS.instancesChanged, () => {
        void queryClient.invalidateQueries({ queryKey: ["instances"] });
        void queryClient.invalidateQueries({ queryKey: ["instance"] });
        void queryClient.invalidateQueries({ queryKey: ["groups"] });
      }),
      listen(EVENTS.accountsChanged, () => void queryClient.invalidateQueries({ queryKey: ["accounts"] })),
      listen<{ instanceId: string; lines: LogLine[] }>(EVENTS.gameLog, (b) => useGames.getState().onLog(b.instanceId, b.lines)),
      listen<GameStatus>(EVENTS.gameState, (s) => useGames.getState().onState(s, instanceName(s.instanceId))),
    ];
    return () => subs.forEach((s) => void s.then((f) => f()));
  }, [receive]);

  // Необработанные ошибки — в тот же единый обработчик.
  useEffect(() => {
    const onRejection = (e: PromiseRejectionEvent) => notify.error(e.reason);
    window.addEventListener("unhandledrejection", onRejection);
    return () => window.removeEventListener("unhandledrejection", onRejection);
  }, []);

  return settings !== null;
}

export function App() {
  const ready = useBootstrap();
  if (!ready) return null;
  return (
    <QueryClientProvider client={queryClient}>
      <HashRouter>
        <Routes>
          <Route element={<AppShell />}>
            <Route index element={<InstancesPage />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="downloads" element={<DownloadsPage />} />
            <Route path="instance/:id/:tab?" element={<InstancePage />} />
          </Route>
        </Routes>
      </HashRouter>
    </QueryClientProvider>
  );
}
