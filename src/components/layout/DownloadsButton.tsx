import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router";
import clsx from "clsx";
import { Download, TriangleAlert } from "lucide-react";
import { useJobs } from "@/stores/jobs";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { progressRatio } from "@/lib/format";

/** Кнопка в статус-баре: сводка задач, по клику — центр загрузок. */
export function DownloadsButton() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const jobs = Object.values(useJobs((s) => s.jobs));
  const running = jobs.filter((j) => j.state === "running");
  const queued = jobs.filter((j) => j.state === "queued").length;
  const failed = jobs.filter((j) => j.state === "failed").length;
  const overall =
    running.length === 0 ? undefined : running.reduce((s, j) => s + progressRatio(j.progress), 0) / running.length;

  return (
    <button
      type="button"
      onClick={() => navigate(pathname === "/downloads" ? "/" : "/downloads")}
      className={clsx(
        "ml-auto flex h-6 items-center gap-2 rounded px-2 hover:bg-surface-2",
        (running.length > 0 || pathname === "/downloads") && "text-text",
      )}
      title={t("downloads.title")}
    >
      <Download size={13} />
      {running.length + queued > 0 ? (
        <>
          {t("jobs.active", { count: running.length + queued })}
          {queued > 0 && <span className="text-muted">{t("downloads.queuedShort", { count: queued })}</span>}
          <ProgressBar value={overall} className="w-24" />
        </>
      ) : (
        t("jobs.title")
      )}
      {failed > 0 && (
        <span className="flex items-center gap-0.5 text-danger">
          <TriangleAlert size={12} />
          {failed}
        </span>
      )}
    </button>
  );
}
