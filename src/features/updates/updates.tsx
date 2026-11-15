import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { create } from "zustand";
import { ArrowUpCircle, ExternalLink, X } from "lucide-react";
import { Button } from "@/components/ui";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { api } from "@/lib/ipc";
import { openExternal } from "@/lib/external";
import { formatDate, progressRatio } from "@/lib/format";
import { isActive, type UpdateInfo } from "@/lib/types";
import { useGames } from "@/stores/games";
import { useJobs } from "@/stores/jobs";
import { useSettings } from "@/stores/settings";
import { notify } from "@/stores/toasts";

export const UPDATE_KEY = ["update"] as const;

/** Скрытое пользователем обновление (до перезапуска лаунчера). */
const useDismissed = create<{ version: string | null; dismiss: (v: string) => void }>((set) => ({
  version: null,
  dismiss: (version) => set({ version }),
}));

/** Результат последней проверки обновлений (сама проверка — по требованию). */
export function useUpdate() {
  return useQuery<UpdateInfo | null>({
    queryKey: UPDATE_KEY,
    queryFn: api.checkUpdate,
    enabled: false,
    retry: false,
    staleTime: Infinity,
  });
}

/** Проверка при запуске (если включена в настройках). Ошибки — только в лог. */
export function useStartupUpdateCheck() {
  const queryClient = useQueryClient();
  const enabled = useSettings((s) => s.settings?.updates.checkOnStartup);
  const [done, setDone] = useState(false);
  useEffect(() => {
    if (done || enabled === undefined) return;
    setDone(true);
    if (!enabled) return;
    queryClient
      .fetchQuery({ queryKey: UPDATE_KEY, queryFn: api.checkUpdate, retry: false })
      .catch((e) => console.warn("проверка обновлений не удалась", e));
  }, [enabled, done, queryClient]);
}

/** Что мешает поставить обновление прямо сейчас (null — ничего). */
export function useInstallBlocker(): string | null {
  const { t } = useTranslation();
  const games = Object.values(useGames((s) => s.status)).some((g) => g.state === "running");
  const jobs = Object.values(useJobs((s) => s.jobs)).some((j) => isActive(j) && j.kind !== "selfUpdate");
  if (games) return t("updates.blockedGame");
  if (jobs) return t("updates.blockedJobs");
  return null;
}

export async function installUpdate() {
  try {
    await api.installUpdate();
  } catch (e) {
    notify.error(e);
  }
}

/** Описание релиза: из markdown понимаем только заголовки и списки, остальное — как текст. */
function UpdateNotes({ notes }: { notes: string }) {
  const clean = (s: string) => s.replace(/\*\*(.+?)\*\*/g, "$1").replace(/`([^`]+)`/g, "$1").replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
  return (
    <div className="max-h-48 space-y-1 overflow-auto rounded-md bg-surface-2 p-3 text-xs text-text">
      {notes.split(/\r?\n/).map((line, i) => {
        const h = /^#{1,6}\s+(.*)$/.exec(line);
        if (h) return <p key={i} className="pt-1 font-semibold">{clean(h[1])}</p>;
        const li = /^\s*[-*+]\s+(.*)$/.exec(line);
        if (li) return <p key={i} className="pl-3 -indent-2">• {clean(li[1])}</p>;
        return line.trim() ? <p key={i}>{clean(line)}</p> : null;
      })}
    </div>
  );
}

/** Действия с найденным обновлением: установка или ссылка на ручную загрузку. */
export function UpdateActions({ info }: { info: UpdateInfo }) {
  const { t } = useTranslation();
  const blocker = useInstallBlocker();
  const job = Object.values(useJobs((s) => s.jobs))
    .filter((j) => j.kind === "selfUpdate")
    .sort((a, b) => b.id - a.id)[0];
  if (job && isActive(job)) {
    return (
      <div className="flex min-w-56 items-center gap-2 text-xs text-muted">
        {t(`jobs.stage.${job.stage ?? "download"}`)}
        <ProgressBar value={job.stage === "install" ? undefined : progressRatio(job.progress)} className="w-32" />
      </div>
    );
  }
  if (info.manual) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-xs text-muted">{t(`updates.manual.${info.manual}`)}</span>
        {info.releaseUrl && (
          <Button variant="secondary" icon={<ExternalLink size={14} />} onClick={() => void openExternal(info.releaseUrl!)}>
            {t("updates.openRelease")}
          </Button>
        )}
      </div>
    );
  }
  return (
    <Button variant="primary" icon={<ArrowUpCircle size={15} />} disabled={!!blocker} title={blocker ?? undefined} onClick={installUpdate}>
      {job?.state === "failed" ? t("updates.retry") : t("updates.install")}
    </Button>
  );
}

/** Полоса под шапкой: «Доступна версия X». */
export function UpdateBanner() {
  const { t } = useTranslation();
  const { data } = useUpdate();
  const dismissed = useDismissed();
  const [notesOpen, setNotesOpen] = useState(false);
  if (!data || dismissed.version === data.version) return null;
  return (
    <div className="shrink-0 border-b border-border bg-accent/10 px-3 py-2 text-sm">
      <div className="flex flex-wrap items-center gap-3">
        <ArrowUpCircle size={17} className="text-accent" />
        <span className="mr-auto">
          {t("updates.available", { version: data.version, current: data.current })}
          {data.date && <span className="ml-2 text-xs text-muted">{formatDate(data.date)}</span>}
        </span>
        {data.notes && (
          <Button variant="ghost" onClick={() => setNotesOpen((v) => !v)} aria-expanded={notesOpen}>
            {notesOpen ? t("updates.hideNotes") : t("updates.notes")}
          </Button>
        )}
        <UpdateActions info={data} />
        <Button variant="ghost" icon={<X size={15} />} onClick={() => dismissed.dismiss(data.version)} title={t("updates.later")}>
          {t("updates.later")}
        </Button>
      </div>
      {notesOpen && data.notes && (
        <div className="mt-2">
          <UpdateNotes notes={data.notes} />
        </div>
      )}
    </div>
  );
}

/** Блок «Обновления» на странице настроек. */
export function UpdateSettingsBlock() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const update = useUpdate();
  const [checked, setChecked] = useState(false);
  const check = async () => {
    try {
      await queryClient.fetchQuery({ queryKey: UPDATE_KEY, queryFn: api.checkUpdate, retry: false, staleTime: 0 });
      setChecked(true);
    } catch (e) {
      notify.error(e);
    }
  };
  const info = update.data;
  return (
    <div className="space-y-2 pt-1.5">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="secondary" onClick={check} disabled={update.isFetching}>
          {t("updates.checkNow")}
        </Button>
        {!update.isFetching && info === null && checked && <span className="text-sm text-muted">{t("updates.upToDate")}</span>}
        {info && (
          <>
            <span className="text-sm">{t("updates.availableShort", { version: info.version })}</span>
            <UpdateActions info={info} />
          </>
        )}
      </div>
      {info?.notes && <UpdateNotes notes={info.notes} />}
    </div>
  );
}
