import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Segmented, Select } from "@/components/ui";
import { api } from "@/lib/ipc";
import { toUserError } from "@/lib/errors";
import { LOADER_KINDS, LOADER_NAMES, type LoaderKind, type LoaderSpec } from "@/lib/types";

type KindOrNone = LoaderKind | "none";

/** Список версий загрузчика для версии Minecraft (кэшируется на время сессии). */
export function useLoaderVersions(kind: LoaderKind | null, minecraft: string | null) {
  return useQuery({
    queryKey: ["loader-versions", kind, minecraft],
    queryFn: () => api.loaderVersions(kind!, minecraft!),
    enabled: !!kind && !!minecraft,
    staleTime: 10 * 60_000,
  });
}

/**
 * Выбор загрузчика модов и его версии. `value === null` — без загрузчика.
 * Пока список версий грузится или пуст, наружу отдаётся `{ kind, version: "" }`,
 * чтобы форма могла заблокировать кнопку создания.
 */
export function LoaderPicker({
  minecraft,
  value,
  onChange,
}: {
  minecraft: string | null;
  value: LoaderSpec | null;
  onChange: (v: LoaderSpec | null) => void;
}) {
  const { t } = useTranslation();
  const kind = value?.kind ?? null;
  const [showUnstable, setShowUnstable] = useState(false);
  const versions = useLoaderVersions(kind, minecraft);

  const list = useMemo(() => {
    const all = versions.data ?? [];
    const filtered = showUnstable ? all : all.filter((v) => v.stable || v.version === value?.version);
    // Если стабильных нет совсем — показываем всё, иначе список пуст без причины.
    return filtered.length > 0 ? filtered : all;
  }, [versions.data, showUnstable, value?.version]);

  // Сменилась версия Minecraft — прежняя версия загрузчика к ней не относится,
  // сбрасываем её до прихода нового списка (иначе можно успеть создать инстанс со старой).
  const prevMinecraft = useRef(minecraft);
  useEffect(() => {
    if (prevMinecraft.current === minecraft) return;
    prevMinecraft.current = minecraft;
    if (kind && value?.version) onChange({ kind, version: "" });
  }, [minecraft]);

  // Подбираем версию по умолчанию: рекомендуемую, иначе самую новую из показанных.
  useEffect(() => {
    if (!kind || !versions.data) return;
    if (value?.version && versions.data.some((v) => v.version === value.version)) return;
    const pick = versions.data.find((v) => v.recommended) ?? list[0];
    onChange({ kind, version: pick?.version ?? "" });
  }, [kind, versions.data]);

  const setKind = (k: KindOrNone) => onChange(k === "none" ? null : { kind: k, version: "" });

  return (
    <div className="flex flex-col gap-2">
      <Segmented<KindOrNone>
        value={kind ?? "none"}
        onValueChange={setKind}
        options={[
          { value: "none", label: t("loaders.none") },
          ...LOADER_KINDS.map((k) => ({ value: k, label: LOADER_NAMES[k] })),
        ]}
      />
      {kind && (
        <div className="flex flex-wrap items-center gap-3">
          {versions.isLoading && <span className="text-sm text-muted">{t("loaders.loading")}</span>}
          {versions.isError && (
            <span className="text-sm text-danger">{toUserError(versions.error).title}</span>
          )}
          {versions.data && versions.data.length === 0 && (
            <span className="text-sm text-muted">
              {t("loaders.unsupported", { loader: LOADER_NAMES[kind], version: minecraft })}
            </span>
          )}
          {versions.data && versions.data.length > 0 && (
            <>
              <Select
                aria-label={t("loaders.version")}
                className="min-w-56"
                value={value?.version ?? ""}
                onValueChange={(v) => onChange({ kind, version: v })}
                options={list.map((v) => ({
                  value: v.version,
                  label: [
                    v.label,
                    v.recommended ? t("loaders.recommended") : null,
                    !v.stable ? t("loaders.unstable") : null,
                  ]
                    .filter(Boolean)
                    .join(" · "),
                }))}
              />
              {versions.data.some((v) => !v.stable) && versions.data.some((v) => v.stable) && (
                <label className="flex items-center gap-1.5 text-sm">
                  <input
                    type="checkbox"
                    checked={showUnstable}
                    onChange={(e) => setShowUnstable(e.target.checked)}
                    className="accent-[var(--accent)]"
                  />
                  {t("loaders.showUnstable")}
                </label>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
