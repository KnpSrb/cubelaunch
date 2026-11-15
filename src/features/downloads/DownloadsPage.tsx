import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import clsx from "clsx";
import { Clock, ExternalLink, ListX, RotateCcw, X } from "lucide-react";
import { Button, NumberInput } from "@/components/ui";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { useJobs } from "@/stores/jobs";
import { useSettings } from "@/stores/settings";
import { notify } from "@/stores/toasts";
import { formatBytes, formatDate, progressRatio } from "@/lib/format";
import { toUserError } from "@/lib/errors";
import { isActive, type JobInfo } from "@/lib/types";

function formatEta(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return "";
  if (secs < 60) return `${Math.ceil(secs)} с`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m} мин ${Math.round(secs % 60)} с`;
  return `${Math.floor(m / 60)} ч ${m % 60} мин`;
}

/** Центр загрузок: активные, ждущие в очереди и завершённые задачи. */
export function DownloadsPage() {
  const { t } = useTranslation();
  const { jobs, speeds, clearFinished } = useJobs();
  const settings = useSettings((s) => s.settings);
  const save = useSettings((s) => s.save);
  const list = Object.values(jobs);
  const running = list.filter((j) => j.state === "running").sort((a, b) => a.id - b.id);
  const queued = list.filter((j) => j.state === "queued").sort((a, b) => a.id - b.id);
  const finished = list.filter((j) => !isActive(j)).sort((a, b) => b.id - a.id);
  const totalSpeed = running.reduce((s, j) => s + (speeds[j.id] ?? 0), 0);

  const setMaxJobs = (maxJobs: number) => {
    if (!settings || maxJobs === settings.downloads.maxJobs || maxJobs < 1 || maxJobs > 8) return;
    save({ ...settings, downloads: { ...settings.downloads, maxJobs } }).catch(notify.error);
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-4xl flex-col gap-5 p-6">
        <div className="flex flex-wrap items-center gap-3">
          <h1 className="mr-auto text-lg font-semibold">{t("downloads.title")}</h1>
          {totalSpeed > 0 && <span className="text-sm text-muted">{t("downloads.speed", { speed: formatBytes(totalSpeed) })}</span>}
          <label className="flex items-center gap-2 text-sm text-muted" title={t("downloads.maxJobsHint")}>
            {t("downloads.maxJobs")}
            {settings && <NumberInput min={1} max={8} value={settings.downloads.maxJobs} onValueChange={setMaxJobs} className="w-20" />}
          </label>
          <Button icon={<ListX size={15} />} disabled={finished.length === 0} onClick={clearFinished}>
            {t("jobs.clearFinished")}
          </Button>
        </div>

        {list.length === 0 && <p className="py-16 text-center text-sm text-muted">{t("jobs.none")}</p>}

        {running.length > 0 && (
          <Group title={t("downloads.running", { count: running.length })}>
            {running.map((j) => (
              <JobRow key={j.id} job={j} speed={speeds[j.id]} />
            ))}
          </Group>
        )}
        {queued.length > 0 && (
          <Group title={t("downloads.queued", { count: queued.length })}>
            {queued.map((j, i) => (
              <JobRow key={j.id} job={j} position={i + 1} />
            ))}
          </Group>
        )}
        {finished.length > 0 && (
          <Group title={t("downloads.finished", { count: finished.length })}>
            {finished.map((j) => (
              <JobRow key={j.id} job={j} />
            ))}
          </Group>
        )}
      </div>
    </div>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="text-sm font-semibold text-muted">{title}</h2>
      {children}
    </section>
  );
}

function JobRow({ job: j, speed, position }: { job: JobInfo; speed?: number; position?: number }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { cancel, retry, dismiss } = useJobs();
  const p = j.progress;
  const ratio = progressRatio(p);
  const eta = speed && p.bytesTotal > 0 ? (p.bytesTotal - p.bytesDone) / speed : 0;
  const status =
    j.state === "queued"
      ? t("downloads.position", { n: position })
      : j.state === "running"
        ? j.stage
          ? t(`jobs.stage.${j.stage}`)
          : t("jobs.state.running")
        : j.state === "failed" && j.error
          ? toUserError(j.error).title
          : t(`jobs.state.${j.state}`);

  return (
    <div className="rounded-lg border border-border bg-surface p-3">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <p className="truncate font-medium" title={j.title}>
              {j.title}
            </p>
            <span className="shrink-0 text-xs text-muted">{t(`jobs.kind.${j.kind}`, { defaultValue: j.kind })}</span>
          </div>
          <p className={clsx("mt-0.5 flex items-center gap-1 text-xs", j.state === "failed" ? "text-danger" : "text-muted")}>
            {j.state === "queued" && <Clock size={12} />}
            {status}
            {j.state === "failed" && j.error?.details && <span className="truncate text-muted" title={j.error.details}> — {j.error.details}</span>}
            {!isActive(j) && j.finishedAt && <span className="ml-auto shrink-0 text-muted">{formatDate(j.finishedAt)} {new Date(j.finishedAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}</span>}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {j.instanceId && (
            <Button
              size="sm"
              variant="ghost"
              icon={<ExternalLink size={14} />}
              title={t("downloads.openInstance")}
              aria-label={t("downloads.openInstance")}
              onClick={() => navigate(`/instance/${encodeURIComponent(j.instanceId!)}/version`)}
            />
          )}
          {isActive(j) ? (
            <Button size="sm" variant="ghost" onClick={() => cancel(j.id)}>
              {t("jobs.cancel")}
            </Button>
          ) : (
            <>
              {j.retryable && (j.state === "failed" || j.state === "cancelled") && (
                <Button size="sm" variant="ghost" icon={<RotateCcw size={14} />} onClick={() => retry(j.id)}>
                  {t("jobs.retry")}
                </Button>
              )}
              <Button size="sm" variant="ghost" icon={<X size={14} />} aria-label={t("jobs.dismiss")} title={t("jobs.dismiss")} onClick={() => dismiss(j.id)} />
            </>
          )}
        </div>
      </div>
      {j.state === "running" && (p.filesTotal > 0 || p.bytesTotal > 0) && (
        <>
          <ProgressBar value={ratio} className="mt-2" />
          <p className="mt-1 flex gap-3 text-xs text-muted">
            {p.filesTotal > 0 && <span>{t("jobs.files", { done: p.filesDone, total: p.filesTotal })}</span>}
            {p.bytesTotal > 0 && (
              <span>
                {formatBytes(p.bytesDone)} / {formatBytes(p.bytesTotal)}
              </span>
            )}
            {speed !== undefined && speed > 0 && <span>{t("downloads.speed", { speed: formatBytes(speed) })}</span>}
            {eta > 0 && <span className="ml-auto">{t("downloads.eta", { time: formatEta(eta) })}</span>}
          </p>
        </>
      )}
      {j.state === "running" && p.filesTotal === 0 && p.bytesTotal === 0 && <ProgressBar className="mt-2" />}
    </div>
  );
}
