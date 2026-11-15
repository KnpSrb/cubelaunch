import { create } from "zustand";
import { api } from "@/lib/ipc";
import { isActive, type JobInfo } from "@/lib/types";
import i18n from "@/i18n";
import { notify } from "./toasts";

/** Последний замер для расчёта скорости: время и скачанные байты. */
const samples = new Map<number, { t: number; bytes: number; stage?: string }>();

interface JobsState {
  jobs: Record<number, JobInfo>;
  /** Скорость загрузки, байт/с (сглаженная). */
  speeds: Record<number, number>;
  init: () => Promise<void>;
  upsert: (job: JobInfo) => void;
  cancel: (id: number) => void;
  retry: (id: number) => void;
  dismiss: (id: number) => void;
  clearFinished: () => void;
}

const DONE_TOAST = ["downloadJava", "installContent", "updateContent", "backupWorld", "installModpack", "updateModpack", "exportModpack", "cloneInstance", "exportInstance", "importInstance"];

export const useJobs = create<JobsState>((set, get) => ({
  jobs: {},
  speeds: {},
  init: async () => {
    const list = await api.listJobs();
    set({ jobs: Object.fromEntries(list.map((j) => [j.id, j])) });
  },
  upsert: (job) => {
    const prev = get().jobs[job.id];
    const speeds = { ...get().speeds };
    if (job.state === "running" && job.progress.bytesTotal > 0) {
      const now = performance.now();
      const s = samples.get(job.id);
      if (s && s.stage === job.stage && now - s.t >= 400) {
        const inst = Math.max(0, ((job.progress.bytesDone - s.bytes) * 1000) / (now - s.t));
        speeds[job.id] = speeds[job.id] === undefined ? inst : speeds[job.id] * 0.7 + inst * 0.3;
        samples.set(job.id, { t: now, bytes: job.progress.bytesDone, stage: job.stage });
      } else if (!s || s.stage !== job.stage) {
        samples.set(job.id, { t: now, bytes: job.progress.bytesDone, stage: job.stage });
        delete speeds[job.id];
      }
    } else {
      samples.delete(job.id);
      delete speeds[job.id];
    }
    set({ jobs: { ...get().jobs, [job.id]: job }, speeds });
    // Уведомляем о завершении один раз — при переходе из активного состояния.
    if (isActive(prev) || (!prev && !isActive(job))) {
      // О запуске игры сообщает её собственный статус, отдельный тост не нужен.
      if (job.state === "done" && job.kind === "installVersion") notify.success(i18n.t("jobs.doneToast", { title: job.title }));
      if (job.state === "done" && DONE_TOAST.includes(job.kind)) notify.success(i18n.t("jobs.javaDoneToast", { title: job.title }));
      if (job.state === "failed" && job.error) {
        // Заголовок — суть ошибки, подробности — что именно не удалось.
        const details = [i18n.t("jobs.failedToast", { title: job.title }), job.error.details].filter(Boolean).join("\n");
        notify.error({ ...job.error, details });
      }
    }
  },
  cancel: (id) => void api.cancelJob(id).catch(notify.error),
  retry: (id) =>
    void api
      .retryJob(id)
      .then(() => {
        const { [id]: _old, ...rest } = get().jobs;
        set({ jobs: rest });
      })
      .catch(notify.error),
  dismiss: (id) => {
    const { [id]: _removed, ...rest } = get().jobs;
    set({ jobs: rest });
    void api.dismissJob(id).catch(notify.error);
  },
  clearFinished: () => {
    for (const j of Object.values(get().jobs)) if (!isActive(j)) get().dismiss(j.id);
  },
}));

/** Активная задача для инстанса (последняя по id). */
export function jobForInstance(jobs: Record<number, JobInfo>, instanceId: string): JobInfo | undefined {
  return Object.values(jobs)
    .filter((j) => j.instanceId === instanceId)
    .sort((a, b) => b.id - a.id)[0];
}
