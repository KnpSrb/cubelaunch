import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ExternalLink, FileUp, FolderSearch, RefreshCw } from "lucide-react";
import { Button, Field, Section, Select } from "@/components/ui";
import { api, inTauri } from "@/lib/ipc";
import { openExternal } from "@/lib/external";
import { toUserError } from "@/lib/errors";
import type { Instance } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { ContentIcon, useFinishedJobs } from "@/features/content/shared";
import { SOURCE_NAMES } from "@/features/content/ModrinthBrowser";
import { packVersionLabel } from "./ModpackBrowser";
import { pickPackFile } from "./ImportPack";

const PACK_JOBS = ["installModpack", "updateModpack"];

/** Блок «Модпак» на вкладке «Версия»: источник, обновление, файлы для ручной загрузки. */
export function ModpackSection({ inst, disabled }: { inst: Instance; disabled: boolean }) {
  const { t } = useTranslation();
  const link = inst.modpack!;
  const remote = link.source !== "file" && !!link.projectId;
  const finished = useFinishedJobs(inst.id, PACK_JOBS);
  const state = useQuery({ queryKey: ["modpack-state", inst.id, finished], queryFn: () => api.modpackState(inst.id) });
  const versions = useQuery({
    queryKey: ["modpack-versions", link.source, link.projectId],
    queryFn: () => api.modpackVersions(link.source, link.projectId!),
    enabled: remote,
    staleTime: 5 * 60_000,
  });
  const [target, setTarget] = useState("");
  useEffect(() => {
    if (versions.data?.length && !target) setTarget(versions.data[0].id);
  }, [versions.data, target]);

  const list = versions.data ?? [];
  const currentIdx = list.findIndex((v) => v.id === link.versionId);
  // Список новый → старый: всё, что выше текущей, — обновления.
  const newer = currentIdx > 0 ? list.slice(0, currentIdx) : [];
  const latestRelease = newer.find((v) => v.versionType === "release") ?? newer[0];

  const update = async (versionId: string | null, path: string | null) => {
    try {
      await api.updateModpack(inst.id, versionId, path);
      notify.info(t("modpacks.updateStarted", { name: inst.name }));
    } catch (e) {
      notify.error(e);
    }
  };

  const addMissing = async (pick: boolean) => {
    let paths: string[] = [];
    if (pick) {
      if (!inTauri) return;
      const p = await openDialog({ multiple: true, directory: false });
      if (!p) return;
      paths = Array.isArray(p) ? p : [p];
    }
    try {
      const left = await api.modpackAddMissing(inst.id, paths);
      void state.refetch();
      if (left === 0) notify.success(t("modpacks.missingDone"));
      else notify.info(t("modpacks.missingLeft", { count: left }));
    } catch (e) {
      notify.error(e);
    }
  };

  return (
    <Section title={t("modpacks.section")}>
      <div className="flex items-center gap-3 py-2">
        <ContentIcon src={link.iconUrl} size={40} />
        <div className="min-w-0 flex-1">
          <p className="truncate font-medium">{link.name}</p>
          <p className="text-sm text-muted">
            {link.version || "—"} · {link.source === "file" ? t("modpacks.fromFile") : SOURCE_NAMES[link.source]}
            {state.data && <> · {t("modpacks.managed", { count: state.data.files })}</>}
          </p>
        </div>
      </div>

      {remote && (
        <Field label={t("modpacks.version")} hint={t("modpacks.updateHint")}>
          {versions.isLoading && <span className="pt-1.5 text-sm text-muted">{t("common.loading")}</span>}
          {versions.isError && <span className="pt-1.5 text-sm text-danger">{toUserError(versions.error).title}</span>}
          {list.length > 0 && (
            <>
              {latestRelease ? (
                <p className="text-sm text-accent">{t("modpacks.updateAvailable", { version: latestRelease.versionNumber })}</p>
              ) : (
                currentIdx === 0 && <p className="text-sm text-muted">{t("modpacks.upToDate")}</p>
              )}
              <div className="flex gap-2">
                <Select
                  aria-label={t("modpacks.version")}
                  className="min-w-0 flex-1"
                  value={target}
                  onValueChange={setTarget}
                  options={list.map((v) => ({
                    value: v.id,
                    label: packVersionLabel(v) + (v.id === link.versionId ? ` (${t("modpacks.current")})` : ""),
                  }))}
                />
                <Button icon={<RefreshCw size={15} />} disabled={disabled || !target} onClick={() => void update(target, null)}>
                  {target === link.versionId ? t("modpacks.reinstall") : t("modpacks.switch")}
                </Button>
              </div>
            </>
          )}
        </Field>
      )}

      <div className="flex gap-2 pt-1">
        <Button
          icon={<FileUp size={15} />}
          disabled={disabled}
          onClick={async () => {
            const p = await pickPackFile();
            if (p) void update(null, p);
          }}
        >
          {t("modpacks.updateFromFile")}
        </Button>
      </div>

      {state.data && state.data.missing.length > 0 && (
        <div className="mt-4 flex flex-col gap-2 rounded-md border border-[var(--warning)] p-3">
          <p className="text-sm font-medium">{t("modpacks.missingTitle", { count: state.data.missing.length })}</p>
          <p className="text-xs text-muted">{t("modpacks.missingHint")}</p>
          <ul className="flex flex-col gap-1">
            {state.data.missing.map((m) => (
              <li key={m.path} className="flex items-center gap-2 text-sm">
                <span className="min-w-0 flex-1 truncate">
                  {m.name} <span className="text-muted">({m.fileName})</span>
                </span>
                <Button size="sm" variant="ghost" icon={<ExternalLink size={13} />} onClick={() => void openExternal(m.pageUrl)}>
                  {t("modpacks.openPage")}
                </Button>
              </li>
            ))}
          </ul>
          <div className="flex gap-2">
            <Button size="sm" icon={<FolderSearch size={14} />} onClick={() => void addMissing(false)}>
              {t("modpacks.checkDownloads")}
            </Button>
            <Button size="sm" onClick={() => void addMissing(true)}>
              {t("modpacks.pickFiles")}
            </Button>
          </div>
        </div>
      )}
    </Section>
  );
}
