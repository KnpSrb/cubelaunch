import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import clsx from "clsx";
import { ChevronDown, Download, ExternalLink, Search } from "lucide-react";
import { Button, Input, Segmented, Select } from "@/components/ui";
import { api } from "@/lib/ipc";
import { curseforgeUrl, modrinthUrl, openExternal } from "@/lib/external";
import { toUserError } from "@/lib/errors";
import { formatDate } from "@/lib/format";
import { LOADER_KINDS, LOADER_NAMES, type ContentSource, type Instance, type LoaderKind, type ModrinthVersion, type SearchHit, type SearchSort } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { ContentIcon } from "@/features/content/shared";
import { SORTS, SOURCE_NAMES, compact, useDebounced } from "@/features/content/ModrinthBrowser";

/** Подпись версии модпака: номер · версии игры · загрузчики. */
export function packVersionLabel(v: ModrinthVersion) {
  const games = v.gameVersions.length > 2 ? `${v.gameVersions[0]}…` : v.gameVersions.join(", ");
  return [v.versionNumber, v.versionType !== "release" ? v.versionType : null, games, v.loaders.filter((l) => l !== "minecraft").join("/")]
    .filter(Boolean)
    .join(" · ");
}

/** Поиск модпаков на Modrinth/CurseForge и установка как нового инстанса. */
export function ModpackBrowser({ group, onInstalled }: { group: string | null; onInstalled: (inst: Instance) => void }) {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const [source, setSource] = useState<ContentSource>("modrinth");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SearchSort>("relevance");
  const [minecraft, setMinecraft] = useState("");
  const [loader, setLoader] = useState<LoaderKind | "any">("any");
  const q = useDebounced(query.trim(), 350);
  const mc = useDebounced(minecraft.trim(), 500);
  const cf = useQuery({ queryKey: ["curseforge-status"], queryFn: api.curseforgeStatus });
  const cfMissing = source === "curseforge" && cf.data && !cf.data.configured;

  const search = useInfiniteQuery({
    queryKey: ["modpack-search", source, q, sort, mc, loader],
    queryFn: ({ pageParam }) => api.modpackSearch(source, q, sort, pageParam, mc || null, loader === "any" ? null : loader),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.offset + last.hits.length < last.totalHits ? last.offset + last.hits.length : undefined),
    enabled: !cfMissing && (source === "modrinth" || !!cf.data),
    staleTime: 5 * 60_000,
  });
  const hits = search.data?.pages.flatMap((p) => p.hits) ?? [];

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented
          value={source}
          onValueChange={setSource}
          options={(["modrinth", "curseforge"] as const).map((s) => ({ value: s, label: SOURCE_NAMES[s] }))}
        />
        <div className="relative min-w-52 flex-1">
          <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("modpacks.search", { source: SOURCE_NAMES[source] })}
            className="w-full pl-8"
          />
        </div>
        <Input
          value={minecraft}
          onChange={(e) => setMinecraft(e.target.value)}
          placeholder={t("modpacks.anyVersion")}
          aria-label={t("instancePage.minecraft")}
          className="w-28"
        />
        <Select
          aria-label={t("newInstance.loader")}
          value={loader}
          onValueChange={setLoader}
          options={[{ value: "any" as const, label: t("modpacks.anyLoader") }, ...LOADER_KINDS.map((k) => ({ value: k, label: LOADER_NAMES[k] }))]}
        />
        <Select
          aria-label={t("modrinth.sort")}
          value={sort}
          onValueChange={setSort}
          options={SORTS.map((s) => ({ value: s, label: t(`modrinth.sorts.${s}`) }))}
        />
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
      {search.data && hits.length === 0 && <p className="py-6 text-center text-sm text-muted">{t("modpacks.nothing")}</p>}

      <div className="flex flex-col divide-y divide-border">
        {hits.map((h) => (
          <PackRow key={h.projectId} hit={h} source={source} group={group} lang={i18n.language} onInstalled={onInstalled} />
        ))}
      </div>
      {search.hasNextPage && (
        <Button className="self-center" disabled={search.isFetchingNextPage} onClick={() => void search.fetchNextPage()}>
          {search.isFetchingNextPage ? t("common.loading") : t("modrinth.more")}
        </Button>
      )}
      <p className="text-center text-[11px] text-muted">{t("modpacks.notice", { source: SOURCE_NAMES[source] })}</p>
    </div>
  );
}

function PackRow({
  hit,
  source,
  group,
  lang,
  onInstalled,
}: {
  hit: SearchHit;
  source: ContentSource;
  group: string | null;
  lang: string;
  onInstalled: (inst: Instance) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [versionId, setVersionId] = useState("");
  const [busy, setBusy] = useState(false);
  const versions = useQuery({
    queryKey: ["modpack-versions", source, hit.projectId],
    queryFn: () => api.modpackVersions(source, hit.projectId),
    enabled: expanded,
    staleTime: 5 * 60_000,
  });
  useEffect(() => {
    if (versions.data?.length && !versionId) setVersionId(versions.data.find((v) => v.versionType === "release")?.id ?? versions.data[0].id);
  }, [versions.data, versionId]);

  const install = async (id?: string) => {
    setBusy(true);
    try {
      let vid = id;
      if (!vid) {
        const list = versions.data ?? (await api.modpackVersions(source, hit.projectId));
        const best = list.find((v) => v.versionType === "release") ?? list[0];
        if (!best) {
          notify.error({ kind: "content", message: t("modpacks.noVersions"), details: null });
          return;
        }
        vid = best.id;
      }
      const inst = await api.installModpack({
        source,
        projectId: hit.projectId,
        versionId: vid,
        title: hit.title,
        name: null,
        group,
        iconUrl: hit.iconUrl ?? null,
      });
      notify.success(t("modpacks.installing", { name: inst.name }));
      onInstalled(inst);
    } catch (e) {
      notify.error(e);
    } finally {
      setBusy(false);
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
              onClick={() => void openExternal(source === "curseforge" ? curseforgeUrl("modpacks", hit.slug) : modrinthUrl("modpack", hit.slug))}
              title={t("modrinth.openOn", { source: SOURCE_NAMES[source] })}
            >
              {hit.title}
              <ExternalLink size={11} className="ml-1 inline opacity-60" />
            </button>
            {hit.author && <span className="truncate text-xs text-muted">{t("modrinth.by", { author: hit.author })}</span>}
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
          <Button size="sm" variant="primary" disabled={busy} onClick={() => void install()}>
            {t("modrinth.install")}
          </Button>
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
          {versions.isError && <span className="text-xs text-danger">{toUserError(versions.error).title}</span>}
          {versions.data && versions.data.length === 0 && <span className="text-xs text-muted">{t("modpacks.noVersions")}</span>}
          {versions.data && versions.data.length > 0 && (
            <>
              <Select
                aria-label={t("modrinth.versions")}
                className="min-w-80"
                value={versionId}
                onValueChange={setVersionId}
                options={versions.data.map((v) => ({ value: v.id, label: packVersionLabel(v) }))}
              />
              <Button size="sm" disabled={!versionId || busy} onClick={() => void install(versionId)}>
                {t("modrinth.installThis")}
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
