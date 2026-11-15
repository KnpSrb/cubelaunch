import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { isTauri } from "@tauri-apps/api/core";
import clsx from "clsx";
import { ArrowUpCircle, FilePlus, FolderOpen, Globe, RefreshCw, Search, Trash2, TriangleAlert } from "lucide-react";
import { Button, Input, Switch } from "@/components/ui";
import { ConfirmDialog } from "@/components/ui/Dialog";
import { api } from "@/lib/ipc";
import { formatBytes } from "@/lib/format";
import { modrinthUrl, openExternal } from "@/lib/external";
import { LOADER_NAMES, type ContentFile, type ContentKind, type ContentUpdate, type Instance } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { ContentIcon, DropOverlay, useFileDrop, useFinishedJobs, useRunningJob } from "./shared";
import { ModrinthBrowser } from "./ModrinthBrowser";

const JOB_KINDS = ["installContent", "updateContent"];

function displayName(f: ContentFile, title?: string) {
  return f.meta?.name ?? title ?? f.fileName.replace(/\.disabled$/, "").replace(/\.(jar|zip|litemod)$/i, "");
}

export function ContentTab({ inst, kind }: { inst: Instance; kind: ContentKind }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const key = ["content", inst.id, kind];
  const files = useQuery({ queryKey: key, queryFn: () => api.listContent(inst.id, kind) });
  const modrinth = useQuery({
    queryKey: ["content-modrinth", inst.id, kind, files.data?.map((f) => f.sha1).join()],
    queryFn: () => api.contentModrinthInfo(inst.id, kind),
    enabled: !!files.data?.length,
    staleTime: Infinity,
    retry: false,
  });
  const [filter, setFilter] = useState("");
  const [browser, setBrowser] = useState(false);
  const [toDelete, setToDelete] = useState<ContentFile | null>(null);
  const [updates, setUpdates] = useState<ContentUpdate[] | null>(null);
  const [checking, setChecking] = useState(false);
  const busy = useRunningJob(inst.id, JOB_KINDS);
  const finished = useFinishedJobs(inst.id, JOB_KINDS);
  const noLoader = kind === "mods" && !inst.loader;

  // После установки/обновления из Modrinth перечитываем список.
  useEffect(() => {
    if (finished > 0) {
      void files.refetch();
      setUpdates(null);
    }
  }, [finished]);

  // Смена версии/загрузчика инстанса делает старый список обновлений неактуальным.
  useEffect(() => setUpdates(null), [inst.minecraft, inst.loader?.kind, inst.loader?.version, kind]);

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: key });
  };

  const addPaths = async (paths: string[]) => {
    try {
      const r = await api.addContentFiles(inst.id, kind, paths);
      if (r.added.length) notify.success(t("content.added", { count: r.added.length }));
      if (r.skipped.length) notify.error(t("content.skipped", { files: r.skipped.join(", ") }));
      refresh();
    } catch (e) {
      notify.error(e);
    }
  };
  const dragOver = useFileDrop(true, (paths) => void addPaths(paths));

  const pickFiles = async () => {
    const exts = kind === "mods" ? ["jar", "zip", "litemod"] : ["zip"];
    const sel = await openDialog({ multiple: true, filters: [{ name: t(`content.kinds.${kind}`), extensions: exts }] });
    if (sel) await addPaths(Array.isArray(sel) ? sel : [sel]);
  };

  const toggle = async (f: ContentFile, enabled: boolean) => {
    // Оптимистично: переключатель реагирует сразу.
    queryClient.setQueryData<ContentFile[]>(key, (old) =>
      old?.map((x) => (x.fileName === f.fileName ? { ...x, enabled } : x)),
    );
    try {
      await api.setContentEnabled(inst.id, kind, f.fileName, enabled);
    } catch (e) {
      notify.error(e);
    }
    refresh();
  };

  const remove = async (f: ContentFile) => {
    try {
      await api.deleteContent(inst.id, kind, [f.fileName]);
      notify.success(t("content.deleted", { name: displayName(f) }));
    } catch (e) {
      notify.error(e);
    }
    refresh();
  };

  const checkUpdates = async () => {
    setChecking(true);
    try {
      const u = await api.checkContentUpdates(inst.id, kind);
      setUpdates(u);
      if (!u.length) notify.success(t("content.upToDate"));
    } catch (e) {
      notify.error(e);
    } finally {
      setChecking(false);
    }
  };

  const applyUpdates = async (list: ContentUpdate[]) => {
    try {
      await api.updateContent(
        inst.id,
        kind,
        list.map((u) => ({ fileName: u.fileName, versionId: u.versionId })),
      );
    } catch (e) {
      notify.error(e);
    }
  };

  const updatesByFile = useMemo(() => new Map((updates ?? []).map((u) => [u.fileName, u])), [updates]);
  const info = modrinth.data ?? {};
  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return (files.data ?? []).filter((f) => {
      if (!q) return true;
      const hay = [f.fileName, f.meta?.name, f.meta?.id, info[f.fileName]?.title].filter(Boolean).join(" ").toLowerCase();
      return hay.includes(q);
    });
  }, [files.data, filter, info]);
  const disabledCount = files.data?.filter((f) => !f.enabled).length ?? 0;
  const installedProjects = useMemo(() => new Set(Object.values(info).map((i) => i.projectId)), [info]);

  return (
    <div className="relative mx-auto flex max-w-5xl flex-col gap-3 p-6">
      <DropOverlay show={dragOver} label={t("content.dropHere", { kind: t(`content.kinds.${kind}`) })} />

      {noLoader && (
        <div className="flex items-center gap-3 rounded-md border border-warning/40 bg-warning/10 p-3 text-sm">
          <TriangleAlert size={16} className="shrink-0 text-warning" />
          <span className="flex-1">{t("content.needLoader")}</span>
          <Button size="sm" onClick={() => navigate(`/instance/${encodeURIComponent(inst.id)}/version`, { replace: true })}>
            {t("content.chooseLoader")}
          </Button>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-48 flex-1">
          <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
          <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t("content.filter")} className="w-full pl-8" />
        </div>
        <Button variant="primary" icon={<Globe size={15} />} disabled={noLoader} onClick={() => setBrowser(true)}>
          {t("content.browse")}
        </Button>
        {isTauri() && (
          <Button icon={<FilePlus size={15} />} onClick={() => void pickFiles()}>
            {t("content.addFiles")}
          </Button>
        )}
        <Button
          icon={<ArrowUpCircle size={15} className={clsx(checking && "animate-pulse")} />}
          disabled={checking || busy || noLoader || !files.data?.length}
          onClick={() => void checkUpdates()}
        >
          {t("content.checkUpdates")}
        </Button>
        <Button
          aria-label={t("content.openFolder")}
          title={t("content.openFolder")}
          icon={<FolderOpen size={15} />}
          onClick={() => api.openContentFolder(inst.id, kind).catch(notify.error)}
        />
        <Button aria-label={t("common.refresh")} title={t("common.refresh")} icon={<RefreshCw size={15} />} onClick={refresh} />
      </div>

      {updates && updates.length > 0 && (
        <div className="flex items-center gap-3 rounded-md border border-accent/40 bg-accent/10 p-3 text-sm">
          <ArrowUpCircle size={16} className="shrink-0 text-accent" />
          <span className="flex-1">{t("content.updatesAvailable", { count: updates.length })}</span>
          <Button size="sm" variant="primary" disabled={busy} onClick={() => void applyUpdates(updates)}>
            {t("content.updateAll")}
          </Button>
        </div>
      )}

      <p className="text-xs text-muted">
        {t("content.count", { count: files.data?.length ?? 0 })}
        {disabledCount > 0 && <> · {t("content.disabledCount", { count: disabledCount })}</>}
        {busy && <> · {t("content.working")}</>}
      </p>

      <div className="overflow-hidden rounded-md border border-border">
        {files.isLoading && <p className="p-4 text-sm text-muted">{t("common.loading")}</p>}
        {files.isError && <p className="p-4 text-sm text-danger">{String((files.error as { message?: string })?.message ?? files.error)}</p>}
        {files.data && files.data.length === 0 && (
          <div className="flex flex-col items-center gap-2 p-8 text-center text-sm text-muted">
            <p>{t("content.empty", { kind: t(`content.kinds.${kind}`) })}</p>
            {isTauri() && <p className="text-xs">{t("content.emptyHint")}</p>}
          </div>
        )}
        {shown.map((f) => {
          const mr = info[f.fileName];
          const upd = updatesByFile.get(f.fileName);
          const version = f.meta?.version ?? mr?.versionNumber;
          return (
            <div
              key={f.fileName}
              className={clsx(
                "flex items-center gap-3 border-b border-border px-3 py-2 last:border-b-0",
                !f.enabled && "opacity-55",
              )}
            >
              <Switch
                checked={f.enabled}
                onCheckedChange={(v) => void toggle(f, v)}
                ariaLabel={t(f.enabled ? "content.disable" : "content.enable", { name: displayName(f, mr?.title) })}
                disabled={f.isDir}
              />
              <ContentIcon src={f.meta?.icon ?? mr?.iconUrl} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate text-sm font-medium">{displayName(f, mr?.title)}</span>
                  {version && <span className="truncate text-xs text-muted">{version}</span>}
                  {f.incompatible && (
                    <span
                      className="shrink-0 rounded bg-danger/15 px-1.5 py-0.5 text-[11px] text-danger"
                      title={t("content.incompatibleHint", {
                        loaders: (f.meta?.loaders ?? []).map((l) => LOADER_NAMES[l as keyof typeof LOADER_NAMES] ?? l).join(", "),
                      })}
                    >
                      {t("content.incompatible")}
                    </span>
                  )}
                  {upd && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void applyUpdates([upd])}
                      className="shrink-0 rounded bg-accent/15 px-1.5 py-0.5 text-[11px] text-accent hover:bg-accent/25"
                      title={t("content.updateOne")}
                    >
                      → {upd.versionNumber}
                    </button>
                  )}
                </div>
                <div className="flex items-center gap-2 text-xs text-muted">
                  <span className="truncate">{f.meta?.description ?? f.fileName}</span>
                </div>
              </div>
              {mr && (
                <button
                  type="button"
                  onClick={() => void openExternal(modrinthUrl(mr.projectType, mr.slug))}
                  className="shrink-0 rounded bg-surface-2 px-1.5 py-0.5 text-[11px] text-muted hover:text-text"
                  title={t("content.openOnModrinth")}
                >
                  Modrinth
                </button>
              )}
              {!f.isDir && <span className="w-16 shrink-0 text-right text-xs text-muted">{formatBytes(f.size)}</span>}
              <Button
                variant="ghost"
                size="sm"
                aria-label={t("content.delete", { name: displayName(f, mr?.title) })}
                title={t("common.delete")}
                icon={<Trash2 size={15} />}
                onClick={() => setToDelete(f)}
              />
            </div>
          );
        })}
      </div>

      <ConfirmDialog
        open={!!toDelete}
        title={t("content.deleteTitle")}
        body={toDelete ? t("content.deleteBody", { name: displayName(toDelete, info[toDelete.fileName]?.title) }) : ""}
        confirmLabel={t("common.delete")}
        danger
        onConfirm={() => {
          if (toDelete) void remove(toDelete);
          setToDelete(null);
        }}
        onClose={() => setToDelete(null)}
      />

      <ModrinthBrowser
        open={browser}
        onClose={() => setBrowser(false)}
        inst={inst}
        kind={kind}
        installed={installedProjects}
      />
    </div>
  );
}
