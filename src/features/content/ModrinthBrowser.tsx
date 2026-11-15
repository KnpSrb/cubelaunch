import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import clsx from "clsx";
import { Check, ChevronDown, Download, ExternalLink, Search } from "lucide-react";
import { Dialog } from "@/components/ui/Dialog";
import { Button, Input, Segmented, Select } from "@/components/ui";
import { api } from "@/lib/ipc";
import { curseforgeUrl, modrinthUrl, openExternal } from "@/lib/external";
import { toUserError } from "@/lib/errors";
import { formatDate } from "@/lib/format";
import type { ContentKind, ContentSource, Instance, SearchHit, SearchSort } from "@/lib/types";
import { loaderLabel } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { ContentIcon } from "./shared";
import { useNavigate } from "react-router";

export const SOURCE_NAMES: Record<ContentSource, string> = { modrinth: "Modrinth", curseforge: "CurseForge" };

export const SORTS: SearchSort[] = ["relevance", "downloads", "follows", "newest", "updated"];

export function compact(n: number, lang: string) {
  return new Intl.NumberFormat(lang, { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

export function useDebounced<T>(value: T, ms: number) {
  const [v, setV] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setV(value), ms);
    return () => clearTimeout(id);
  }, [value, ms]);
  return v;
}

export function ModrinthBrowser({
  open,
  onClose,
  inst,
  kind,
  installed,
}: {
  open: boolean;
  onClose: () => void;
  inst: Instance;
  kind: ContentKind;
  installed: Set<string>;
}) {
  const { t, i18n } = useTranslation();
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SearchSort>("relevance");
  const [withDeps, setWithDeps] = useState(true);
  const [source, setSource] = useState<ContentSource>("modrinth");
  const q = useDebounced(query.trim(), 350);
  const navigate = useNavigate();
  const cf = useQuery({ queryKey: ["curseforge-status"], queryFn: api.curseforgeStatus, enabled: open });
  const cfMissing = source === "curseforge" && cf.data && !cf.data.configured;

  const search = useInfiniteQuery({
    queryKey: ["content-search", source, inst.id, inst.minecraft, inst.loader?.kind, kind, q, sort],
    queryFn: ({ pageParam }) => api.contentSearch(source, inst.id, kind, q, sort, pageParam),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.offset + last.hits.length < last.totalHits ? last.offset + last.hits.length : undefined),
    enabled: open && !cfMissing && (source === "modrinth" || !!cf.data),
    staleTime: 5 * 60_000,
  });
  const hits = search.data?.pages.flatMap((p) => p.hits) ?? [];
  const total = search.data?.pages[0]?.totalHits ?? 0;

  const target = [inst.minecraft, inst.loader && kind === "mods" ? loaderLabel(inst.minecraft, inst.loader) : null]
    .filter(Boolean)
    .join(" · ");

  return (
    <Dialog
      open={open}
      onClose={onClose}
      width="xl"
      title={t("modrinth.title", { source: SOURCE_NAMES[source], kind: t(`content.kinds.${kind}`), target })}
      footer={<Button onClick={onClose}>{t("common.close")}</Button>}
    >
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <Segmented
            value={source}
            onValueChange={setSource}
            options={(["modrinth", "curseforge"] as const).map((s) => ({ value: s, label: SOURCE_NAMES[s] }))}
          />
          <div className="relative min-w-60 flex-1">
            <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
            <Input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("modrinth.search", { source: SOURCE_NAMES[source] })}
              className="w-full pl-8"
            />
          </div>
          <Select
            aria-label={t("modrinth.sort")}
            value={sort}
            onValueChange={setSort}
            options={SORTS.map((s) => ({ value: s, label: t(`modrinth.sorts.${s}`) }))}
          />
          {kind === "mods" && (
            <label className="flex items-center gap-1.5 text-sm">
              <input
                type="checkbox"
                checked={withDeps}
                onChange={(e) => setWithDeps(e.target.checked)}
                className="accent-[var(--accent)]"
              />
              {t("modrinth.withDeps")}
            </label>
          )}
        </div>

        {cfMissing && (
          <div className="flex flex-col items-center gap-2 py-6 text-center text-sm text-muted">
            <p>{t("curseforge.notConfigured")}</p>
            <Button size="sm" onClick={() => navigate("/settings#curseforge")}>
              {t("curseforge.openSettings")}
            </Button>
          </div>
        )}
        {search.isLoading && <p className="py-6 text-center text-sm text-muted">{t("common.loading")}</p>}
        {search.isError && <p className="py-6 text-center text-sm text-danger">{toUserError(search.error).title}</p>}
        {search.data && hits.length === 0 && <p className="py-6 text-center text-sm text-muted">{t("modrinth.nothing")}</p>}
        {hits.length > 0 && <p className="text-xs text-muted">{t("modrinth.found", { count: total })}</p>}

        <div className="flex flex-col divide-y divide-border">
          {hits.map((h) => (
            <HitRow
              key={h.projectId}
              hit={h}
              source={source}
              inst={inst}
              kind={kind}
              installed={source === "modrinth" && installed.has(h.projectId)}
              withDeps={withDeps && kind === "mods"}
              lang={i18n.language}
            />
          ))}
        </div>

        {search.hasNextPage && (
          <Button className="self-center" disabled={search.isFetchingNextPage} onClick={() => void search.fetchNextPage()}>
            {search.isFetchingNextPage ? t("common.loading") : t("modrinth.more")}
          </Button>
        )}
        <p className="text-center text-[11px] text-muted">{t("modrinth.notice", { source: SOURCE_NAMES[source] })}</p>
      </div>
    </Dialog>
  );
}

function HitRow({
  hit,
  source,
  inst,
  kind,
  installed,
  withDeps,
  lang,
}: {
  hit: SearchHit;
  source: ContentSource;
  inst: Instance;
  kind: ContentKind;
  installed: boolean;
  withDeps: boolean;
  lang: string;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [versionId, setVersionId] = useState("");
  const [started, setStarted] = useState(false);
  const versions = useQuery({
    queryKey: ["content-versions", source, inst.id, inst.minecraft, inst.loader?.kind, kind, hit.projectId],
    queryFn: () => api.contentVersions(source, inst.id, kind, hit.projectId),
    enabled: expanded,
    staleTime: 5 * 60_000,
  });
  useEffect(() => {
    if (versions.data?.length && !versionId) setVersionId(versions.data.find((v) => v.versionType === "release")?.id ?? versions.data[0].id);
  }, [versions.data, versionId]);
  // После установки (проект появился среди установленных) снимаем отметку «ставится».
  useEffect(() => {
    if (installed) setStarted(false);
  }, [installed]);

  const install = async (id?: string) => {
    try {
      let vid = id;
      if (!vid) {
        const list = versions.data ?? (await api.contentVersions(source, inst.id, kind, hit.projectId));
        const best = list.find((v) => v.versionType === "release") ?? list[0];
        if (!best) {
          notify.error({ kind: "content", message: t("modrinth.noCompatible"), details: null });
          return;
        }
        vid = best.id;
      }
      if (source === "curseforge") await api.installCurseforge(inst.id, kind, hit.projectId, vid, hit.title, withDeps);
      else await api.installModrinth(inst.id, kind, vid, hit.title, withDeps);
      setStarted(true);
      notify.success(t("modrinth.installing", { title: hit.title }));
    } catch (e) {
      notify.error(e);
    }
  };

  return (
    <div className="flex flex-col gap-2 py-3">
      <div className="flex items-start gap-3">
        <ContentIcon src={hit.iconUrl} size={48} />
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <button
              type="button"
              className="truncate text-sm font-semibold hover:underline"
              onClick={() => void openExternal(source === "curseforge" ? curseforgeUrl(kind, hit.slug) : modrinthUrl(hit.projectType, hit.slug))}
              title={t("modrinth.openOn", { source: SOURCE_NAMES[source] })}
            >
              {hit.title}
              <ExternalLink size={11} className="ml-1 inline opacity-60" />
            </button>
            <span className="truncate text-xs text-muted">{t("modrinth.by", { author: hit.author })}</span>
          </div>
          <p className="line-clamp-2 text-xs text-muted">{hit.description}</p>
          <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
            <span>
              <Download size={11} className="mr-0.5 inline" />
              {compact(hit.downloads, lang)}
            </span>
            {hit.dateModified && <span>{t("modrinth.updated", { date: formatDate(hit.dateModified) })}</span>}
            {hit.categories.slice(0, 4).map((c) => (
              <span key={c} className="rounded bg-surface-2 px-1.5 py-0.5">
                {c}
              </span>
            ))}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {installed ? (
            <span className="flex items-center gap-1 px-2 text-xs text-accent">
              <Check size={14} />
              {t("modrinth.installed")}
            </span>
          ) : (
            <Button size="sm" variant="primary" disabled={started} onClick={() => void install()}>
              {started ? t("modrinth.started") : t("modrinth.install")}
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            aria-label={t("modrinth.versions")}
            title={t("modrinth.versions")}
            aria-expanded={expanded}
            icon={<ChevronDown size={15} className={clsx("transition-transform", expanded && "rotate-180")} />}
            onClick={() => setExpanded((x) => !x)}
          />
        </div>
      </div>
      {expanded && (
        <div className="ml-15 flex flex-wrap items-center gap-2">
          {versions.isLoading && <span className="text-xs text-muted">{t("common.loading")}</span>}
          {versions.data && versions.data.length === 0 && <span className="text-xs text-muted">{t("modrinth.noCompatible")}</span>}
          {versions.data && versions.data.length > 0 && (
            <>
              <Select
                aria-label={t("modrinth.versions")}
                className="min-w-72"
                value={versionId}
                onValueChange={setVersionId}
                options={versions.data.map((v) => ({
                  value: v.id,
                  label: [v.versionNumber, v.versionType !== "release" ? v.versionType : null, v.loaders.join("/")]
                    .filter(Boolean)
                    .join(" · "),
                }))}
              />
              <Button size="sm" disabled={!versionId} onClick={() => void install(versionId)}>
                {installed ? t("modrinth.installThisReplace") : t("modrinth.installThis")}
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
