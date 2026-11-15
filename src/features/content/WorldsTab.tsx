import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Archive, FolderOpen, Globe2, RefreshCw, Skull, Trash2 } from "lucide-react";
import { Button } from "@/components/ui";
import { ConfirmDialog } from "@/components/ui/Dialog";
import { api } from "@/lib/ipc";
import { formatBytes, formatDate } from "@/lib/format";
import type { Instance, World } from "@/lib/types";
import { useGames } from "@/stores/games";
import { notify } from "@/stores/toasts";
import { useFinishedJobs, useRunningJob } from "./shared";

export function WorldsTab({ inst }: { inst: Instance }) {
  const { t } = useTranslation();
  const data = useQuery({ queryKey: ["worlds", inst.id], queryFn: () => api.listWorlds(inst.id) });
  const [toDelete, setToDelete] = useState<World | null>(null);
  const running = useGames((s) => s.status[inst.id]?.state === "running");
  const backingUp = useRunningJob(inst.id, ["backupWorld"]);
  const finished = useFinishedJobs(inst.id, ["backupWorld"]);

  useEffect(() => {
    if (finished > 0) void data.refetch();
  }, [finished]);
  // После выхода из игры список миров мог измениться.
  useEffect(() => {
    if (!running) void data.refetch();
  }, [running]);

  const backup = (w: World) =>
    api
      .backupWorld(inst.id, w.folder)
      .then(() => notify.success(t("worlds.backupStarted", { name: w.name })))
      .catch(notify.error);

  const remove = async (w: World) => {
    try {
      await api.deleteWorld(inst.id, w.folder);
      notify.success(t("worlds.deleted", { name: w.name }));
    } catch (e) {
      notify.error(e);
    }
    void data.refetch();
  };

  const worlds = data.data?.worlds ?? [];
  const backups = data.data?.backups ?? [];

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-3 p-6">
      <div className="flex flex-wrap items-center gap-2">
        <p className="flex-1 text-sm text-muted">{t("worlds.count", { count: worlds.length })}</p>
        <Button icon={<FolderOpen size={15} />} onClick={() => api.openContentFolder(inst.id, "saves").catch(notify.error)}>
          {t("worlds.openSaves")}
        </Button>
        <Button icon={<Archive size={15} />} onClick={() => api.openContentFolder(inst.id, "backups").catch(notify.error)}>
          {t("worlds.openBackups", { count: backups.length })}
        </Button>
        <Button aria-label={t("common.refresh")} title={t("common.refresh")} icon={<RefreshCw size={15} />} onClick={() => void data.refetch()} />
      </div>

      {running && <p className="rounded-md bg-surface-2 p-3 text-xs text-muted">{t("worlds.runningHint")}</p>}

      <div className="overflow-hidden rounded-md border border-border">
        {data.isLoading && <p className="p-4 text-sm text-muted">{t("common.loading")}</p>}
        {data.data && worlds.length === 0 && <p className="p-8 text-center text-sm text-muted">{t("worlds.empty")}</p>}
        {worlds.map((w) => (
          <div key={w.folder} className="flex items-center gap-3 border-b border-border px-3 py-2.5 last:border-b-0">
            {w.icon ? (
              <img src={w.icon} alt="" className="h-12 w-12 shrink-0 rounded-md [image-rendering:pixelated]" />
            ) : (
              <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-md bg-surface-2 text-muted">
                <Globe2 size={22} />
              </div>
            )}
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate text-sm font-medium">{w.name}</span>
                {w.hardcore && (
                  <span className="flex shrink-0 items-center gap-1 rounded bg-danger/15 px-1.5 py-0.5 text-[11px] text-danger">
                    <Skull size={11} />
                    {t("worlds.hardcore")}
                  </span>
                )}
              </div>
              <p className="truncate text-xs text-muted">
                {[
                  w.gameMode && t(`worlds.modes.${w.gameMode}`),
                  w.version,
                  w.lastPlayed && t("worlds.lastPlayed", { date: formatDate(w.lastPlayed) }),
                  formatBytes(w.size),
                  w.folder !== w.name ? w.folder : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </p>
            </div>
            <Button size="sm" icon={<Archive size={14} />} disabled={backingUp} onClick={() => void backup(w)}>
              {t("worlds.backup")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              aria-label={t("worlds.openFolder")}
              title={t("worlds.openFolder")}
              icon={<FolderOpen size={15} />}
              onClick={() => api.openContentFolder(inst.id, `saves/${w.folder}`).catch(notify.error)}
            />
            <Button
              size="sm"
              variant="ghost"
              aria-label={t("worlds.delete", { name: w.name })}
              title={t("common.delete")}
              disabled={running}
              icon={<Trash2 size={15} />}
              onClick={() => setToDelete(w)}
            />
          </div>
        ))}
      </div>

      {backups.length > 0 && (
        <p className="text-xs text-muted">
          {t("worlds.lastBackup", { name: backups[0].fileName, date: backups[0].created ? formatDate(backups[0].created) : "" })}
        </p>
      )}

      <ConfirmDialog
        open={!!toDelete}
        title={t("worlds.deleteTitle")}
        body={toDelete ? t("worlds.deleteBody", { name: toDelete.name }) : ""}
        confirmLabel={t("common.delete")}
        danger
        onConfirm={() => {
          if (toDelete) void remove(toDelete);
          setToDelete(null);
        }}
        onClose={() => setToDelete(null)}
      />
    </div>
  );
}
