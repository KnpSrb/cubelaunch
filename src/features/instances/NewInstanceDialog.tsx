import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import clsx from "clsx";
import { RefreshCw, Search } from "lucide-react";
import { Dialog } from "@/components/ui/Dialog";
import { Button, Input, Segmented } from "@/components/ui";
import { ModpackBrowser } from "@/features/modpacks/ModpackBrowser";
import { ImportPack } from "@/features/modpacks/ImportPack";
import { api } from "@/lib/ipc";
import { formatDate } from "@/lib/format";
import { LOADER_NAMES, type Instance, type LoaderSpec, type VersionType } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { LoaderPicker } from "./LoaderPicker";

const TYPES: VersionType[] = ["release", "snapshot", "old_beta", "old_alpha"];

export function NewInstanceDialog({
  open,
  onClose,
  groups,
  onCreated,
  importPath,
}: {
  open: boolean;
  onClose: () => void;
  groups: string[];
  onCreated: (inst: Instance) => void;
  /** Открыть сразу на импорте этого файла (перетаскивание модпака в окно). */
  importPath?: string | null;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const manifest = useQuery({
    queryKey: ["version-manifest"],
    queryFn: () => api.getVersionManifest(false),
    enabled: open,
    staleTime: 10 * 60_000,
  });

  const [name, setName] = useState("");
  const [nameTouched, setNameTouched] = useState(false);
  const [group, setGroup] = useState("");
  const [query, setQuery] = useState("");
  const [types, setTypes] = useState<Set<VersionType>>(new Set(["release"]));
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loader, setLoader] = useState<LoaderSpec | null>(null);
  const [mode, setMode] = useState<"custom" | "modpacks" | "import">("custom");

  // Сброс формы при каждом открытии.
  useEffect(() => {
    if (!open) return;
    setName("");
    setNameTouched(false);
    setGroup("");
    setQuery("");
    setTypes(new Set(["release"]));
    setSelected(null);
    setLoader(null);
    setMode(importPath ? "import" : "custom");
  }, [open, importPath]);

  // По умолчанию выбираем последний релиз.
  useEffect(() => {
    if (open && !selected && manifest.data) setSelected(manifest.data.latest.release);
  }, [open, selected, manifest.data]);

  // Имя подставляется из версии и загрузчика, пока пользователь не ввёл своё.
  const autoName = (version: string) => (loader ? `${version} ${LOADER_NAMES[loader.kind]}` : version);
  useEffect(() => {
    if (!nameTouched && selected) setName(autoName(selected));
  }, [selected, nameTouched, loader?.kind]);

  const versions = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (manifest.data?.versions ?? []).filter((v) => types.has(v.type) && (!q || v.id.toLowerCase().includes(q)));
  }, [manifest.data, types, query]);

  const toggleType = (ty: VersionType) => {
    const next = new Set(types);
    if (next.has(ty)) next.delete(ty);
    else next.add(ty);
    if (next.size > 0) setTypes(next);
  };

  const [refreshing, setRefreshing] = useState(false);
  const refresh = async () => {
    setRefreshing(true);
    try {
      queryClient.setQueryData(["version-manifest"], await api.getVersionManifest(true));
    } catch (e) {
      notify.error(e);
    } finally {
      setRefreshing(false);
    }
  };

  const create = async (version = selected) => {
    if (!version || busy || (loader && !loader.version)) return;
    // При двойном клике имя ещё может быть от предыдущей версии.
    const finalName = (nameTouched ? name : autoName(version)).trim();
    if (!finalName) return;
    setBusy(true);
    try {
      const inst = await api.createInstance({ name: finalName, group: group.trim() || null, minecraft: version, loader });
      notify.success(t("newInstance.created", { name: inst.name }));
      onCreated(inst);
      onClose();
    } catch (e) {
      notify.error(e);
    } finally {
      setBusy(false);
    }
  };

  const latest = manifest.data?.latest;
  // Загрузчик выбран, но версия ещё не подобрана (грузится или нет поддержки).
  const loaderPending = !!loader && !loader.version;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t("newInstance.title")}
      width={mode === "modpacks" ? "xl" : "lg"}
      footer={
        mode !== "custom" ? (
          <Button onClick={onClose}>{t("common.close")}</Button>
        ) : (
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={!selected || !name.trim() || busy || loaderPending} onClick={() => void create()}>
            {t("newInstance.create")}
          </Button>
        </>
        )
      }
    >
      <div className="flex flex-col gap-4">
      <Segmented
        value={mode}
        onValueChange={setMode}
        options={[
          { value: "custom", label: t("newInstance.modeCustom") },
          { value: "modpacks", label: t("newInstance.modeModpacks") },
          { value: "import", label: t("newInstance.modeImport") },
        ]}
      />
      {mode !== "custom" && (
        <label className="flex max-w-sm flex-col gap-1.5 text-sm font-medium">
          {t("newInstance.group")}
          <Input
            value={group}
            list="new-instance-groups"
            placeholder={t("newInstance.groupPlaceholder")}
            onChange={(e) => setGroup(e.target.value)}
          />
        </label>
      )}
      {mode === "modpacks" && (
        <ModpackBrowser
          group={group.trim() || null}
          onInstalled={(inst) => {
            onCreated(inst);
            onClose();
          }}
        />
      )}
      {mode === "import" && (
        <ImportPack
          initialPath={importPath ?? null}
          group={group.trim() || null}
          onImported={(inst) => {
            onCreated(inst);
            onClose();
          }}
        />
      )}
      {mode === "custom" && (
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          void create();
        }}
      >
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5 text-sm font-medium">
            {t("newInstance.name")}
            <Input
              value={name}
              maxLength={64}
              onChange={(e) => {
                setName(e.target.value);
                setNameTouched(true);
              }}
            />
          </label>
          <label className="flex flex-col gap-1.5 text-sm font-medium">
            {t("newInstance.group")}
            <Input
              value={group}
              list="new-instance-groups"
              placeholder={t("newInstance.groupPlaceholder")}
              onChange={(e) => setGroup(e.target.value)}
            />
          </label>
        </div>

        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <span className="mr-auto text-sm font-medium">{t("newInstance.version")}</span>
            {TYPES.map((ty) => (
              <label key={ty} className="flex items-center gap-1.5 text-sm">
                <input
                  type="checkbox"
                  checked={types.has(ty)}
                  onChange={() => toggleType(ty)}
                  className="accent-[var(--accent)]"
                />
                {t(`versionFilters.${ty}`)}
              </label>
            ))}
          </div>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("newInstance.search")}
                className="w-full pl-8"
              />
            </div>
            <Button
              aria-label={t("newInstance.refresh")}
              title={t("newInstance.refresh")}
              disabled={refreshing}
              icon={<RefreshCw size={14} className={clsx((refreshing || manifest.isFetching) && "animate-spin")} />}
              onClick={refresh}
            />
          </div>

          <div className="h-64 overflow-y-auto rounded-md border border-border" role="listbox" aria-label={t("newInstance.version")}>
            {manifest.isLoading && <p className="p-3 text-sm text-muted">{t("common.loading")}</p>}
            {manifest.isError && <p className="p-3 text-sm text-danger">{t("errors.network")}</p>}
            {manifest.data && versions.length === 0 && (
              <p className="p-3 text-sm text-muted">{t("newInstance.noVersions")}</p>
            )}
            {versions.length > 0 && (
              <table className="w-full text-sm">
                <thead className="sticky top-0 bg-surface-2 text-left text-xs text-muted">
                  <tr>
                    <th className="px-3 py-1.5 font-medium">{t("newInstance.colVersion")}</th>
                    <th className="px-3 py-1.5 font-medium">{t("newInstance.colType")}</th>
                    <th className="px-3 py-1.5 font-medium">{t("newInstance.colDate")}</th>
                  </tr>
                </thead>
                <tbody>
                  {versions.map((v) => (
                    <tr
                      key={v.id}
                      role="option"
                      aria-selected={selected === v.id}
                      onClick={() => setSelected(v.id)}
                      onDoubleClick={() => {
                        setSelected(v.id);
                        // С загрузчиком сначала нужно дождаться списка его версий.
                        if (!loader) void create(v.id);
                      }}
                      className={clsx(
                        "cursor-pointer",
                        selected === v.id ? "bg-accent text-accent-fg" : "hover:bg-surface-2",
                      )}
                    >
                      <td className="px-3 py-1.5 font-medium">
                        {v.id}
                        {(v.id === latest?.release || v.id === latest?.snapshot) && (
                          <span className="ml-2 text-xs opacity-70">({t("newInstance.latest")})</span>
                        )}
                      </td>
                      <td className="px-3 py-1.5">{t(`versionTypes.${v.type}`)}</td>
                      <td className="px-3 py-1.5">{formatDate(v.releaseTime)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>

        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">{t("newInstance.loader")}</span>
          <LoaderPicker minecraft={selected} value={loader} onChange={setLoader} />
        </div>
      </form>
      )}
      <datalist id="new-instance-groups">
        {groups.map((g) => (
          <option key={g} value={g} />
        ))}
      </datalist>
      </div>
    </Dialog>
  );
}
