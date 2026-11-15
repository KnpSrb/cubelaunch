import { useState } from "react";
import { useNavigate, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import clsx from "clsx";
import { Archive, Copy, Download, FolderOpen, Hammer, PackageOpen, Play, Replace, ShieldCheck, Square } from "lucide-react";
import { ModpackSection } from "@/features/modpacks/ModpackSection";
import { ExportDialog } from "@/features/modpacks/ExportDialog";
import { CloneDialog, InstanceExportDialog } from "./TransferDialogs";
import { Button, Field, Section } from "@/components/ui";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { api } from "@/lib/ipc";
import { formatDate, formatDuration, progressRatio } from "@/lib/format";
import { useGames } from "@/stores/games";
import { jobForInstance, useJobs } from "@/stores/jobs";
import { isActive, loaderLabel } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { ChangeVersionDialog } from "./ChangeVersionDialog";
import { ContentTab } from "@/features/content/ContentTab";
import { WorldsTab } from "@/features/content/WorldsTab";
import { InstanceIcon } from "./InstanceIcon";
import { LogView } from "./LogView";
import { OverridesForm } from "./OverridesForm";
import { usePlay } from "./usePlay";

const TABS = ["version", "mods", "resourcepacks", "shaders", "worlds", "screenshots", "settings", "log"] as const;
type Tab = (typeof TABS)[number];

export function InstancePage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id = "", tab = "version" } = useParams();
  const current = (TABS as readonly string[]).includes(tab) ? (tab as Tab) : "version";
  const inst = useQuery({ queryKey: ["instance", id], queryFn: () => api.getInstance(id) });
  // Список инстансов обновляется событиями — берём свежую версию оттуда, если есть.
  const fromList = useQuery({ queryKey: ["instances"], queryFn: api.listInstances }).data?.find((i) => i.id === id);
  const data = fromList ?? inst.data;
  const status = useGames((s) => s.status[id]);
  const job = jobForInstance(useJobs((s) => s.jobs), id);
  const { play, stop } = usePlay();
  const [changing, setChanging] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [transfer, setTransfer] = useState<null | "clone" | "export">(null);

  if (inst.isError && !data) {
    return <p className="p-6 text-sm text-muted">{t("instancePage.notFound")}</p>;
  }
  if (!data) return null;

  const running = status?.state === "running";
  const busy = isActive(job) && !!job;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-4 border-b border-border bg-surface px-6 py-4">
        <InstanceIcon name={data.name} src={data.modpack?.iconUrl} size={48} />
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-lg font-semibold">{data.name}</h1>
          <p className="text-sm text-muted">
            {t("selection.version", { version: data.minecraft })}
            {data.loader && <> · {loaderLabel(data.minecraft, data.loader)}</>}
            {data.group && <> · {data.group}</>}
            {running && <span className="ml-2 text-accent">● {t("game.running")}</span>}
          </p>
          {busy && (
            <div className="mt-1 flex items-center gap-2">
              <ProgressBar value={job.progress.filesTotal > 0 ? progressRatio(job.progress) : undefined} className="w-48" />
              <span className="text-xs text-muted">{job.stage && t(`jobs.stage.${job.stage}`)}</span>
            </div>
          )}
        </div>
        <Button icon={<FolderOpen size={16} />} onClick={() => api.openInstanceFolder(data.id).catch(notify.error)}>
          {t("selection.openFolder")}
        </Button>
        {running ? (
          <Button variant="danger" size="lg" icon={<Square size={16} />} onClick={() => stop(data)}>
            {t("game.stop")}
          </Button>
        ) : (
          <Button variant="primary" size="lg" icon={<Play size={18} />} disabled={busy} onClick={() => play(data)}>
            {t("selection.play")}
          </Button>
        )}
      </div>

      <nav className="flex gap-1 border-b border-border bg-surface px-4" role="tablist">
        {TABS.map((tb) => (
          <button
            key={tb}
            type="button"
            role="tab"
            aria-selected={current === tb}
            onClick={() => navigate(`/instance/${encodeURIComponent(id)}/${tb}`, { replace: true })}
            className={clsx(
              "-mb-px border-b-2 px-3 py-2 text-sm transition-colors",
              current === tb ? "border-accent font-medium text-text" : "border-transparent text-muted hover:text-text",
            )}
          >
            {t(`instancePage.tabs.${tb}`)}
          </button>
        ))}
      </nav>

      <div className={clsx("min-h-0 flex-1", current === "log" ? "p-4" : "overflow-y-auto")}>
        {current === "log" && <LogView instanceId={id} fileName={`${data.id}-latest.log`} />}
        {current === "settings" && (
          <div className="mx-auto max-w-3xl p-6">
            <OverridesForm inst={data} />
          </div>
        )}
        {current === "version" && (
          <div className="mx-auto flex max-w-3xl flex-col gap-4 p-6">
            <Section title={t("instancePage.tabs.version")}>
              <Field label={t("instancePage.minecraft")}>
                <span className="pt-1.5 text-sm">{data.minecraft}</span>
              </Field>
              <Field label={t("newInstance.loader")}>
                <span className="pt-1.5 text-sm">
                  {data.loader ? loaderLabel(data.minecraft, data.loader) : <span className="text-muted">{t("loaders.none")}</span>}
                </span>
              </Field>
              <Field label={t("instancePage.created")}>
                <span className="pt-1.5 text-sm">{formatDate(data.createdAt)}</span>
              </Field>
              <Field label={t("instancePage.lastPlayed")}>
                <span className="pt-1.5 text-sm">{data.lastPlayed ? formatDate(data.lastPlayed) : t("instances.never")}</span>
              </Field>
              <Field label={t("instancePage.playtime")}>
                <span className="pt-1.5 text-sm">{formatDuration(data.playtimeSecs)}</span>
              </Field>
              <div className="flex gap-2 pt-3">
                <Button icon={<Replace size={15} />} disabled={busy || running} onClick={() => setChanging(true)}>
                  {t("changeVersion.open")}
                </Button>
                <Button
                  icon={data.installed ? <ShieldCheck size={15} /> : <Download size={15} />}
                  disabled={busy || running}
                  onClick={() => api.installInstance(data.id).catch(notify.error)}
                >
                  {data.installed ? t("selection.verify") : t("selection.install")}
                </Button>
              </div>
              <div className="flex flex-wrap gap-2 pt-2">
                <Button icon={<PackageOpen size={15} />} disabled={busy} onClick={() => setExporting(true)}>
                  {t("export.open")}
                </Button>
                <Button icon={<Archive size={15} />} disabled={busy} onClick={() => setTransfer("export")}>
                  {t("instanceExport.open")}
                </Button>
                <Button icon={<Copy size={15} />} onClick={() => setTransfer("clone")}>
                  {t("selection.copy")}
                </Button>
              </div>
            </Section>
            {data.modpack && <ModpackSection inst={data} disabled={busy || running} />}
            <ExportDialog inst={data} open={exporting} onClose={() => setExporting(false)} />
            <InstanceExportDialog inst={data} open={transfer === "export"} onClose={() => setTransfer(null)} />
            <CloneDialog
              inst={data}
              open={transfer === "clone"}
              onClose={() => setTransfer(null)}
              onCloned={(c) => navigate(`/instance/${encodeURIComponent(c.id)}/version`)}
            />
            <ChangeVersionDialog inst={data} open={changing} onClose={() => setChanging(false)} />
          </div>
        )}
        {current === "mods" && <ContentTab key="mods" inst={data} kind="mods" />}
        {current === "resourcepacks" && <ContentTab key="resourcepacks" inst={data} kind="resourcepacks" />}
        {current === "shaders" && <ContentTab key="shaderpacks" inst={data} kind="shaderpacks" />}
        {current === "worlds" && <WorldsTab inst={data} />}
        {current === "screenshots" && (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-muted">
            <Hammer size={32} />
            <p className="text-sm">{t("instancePage.tabSoon")}</p>
          </div>
        )}
      </div>
    </div>
  );
}
