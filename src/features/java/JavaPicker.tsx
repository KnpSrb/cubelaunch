import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, RefreshCw } from "lucide-react";
import clsx from "clsx";
import { Button, Input } from "@/components/ui";
import { api } from "@/lib/ipc";
import { notify } from "@/stores/toasts";

export function useJavaList() {
  return useQuery({ queryKey: ["java"], queryFn: () => api.detectJava(false), staleTime: Infinity });
}

/**
 * Выбор Java: «автоматически» (null), одна из найденных или свой путь с проверкой.
 */
export function JavaPicker({ value, onChange }: { value: string | null; onChange: (path: string | null) => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const list = useJavaList();
  const [custom, setCustom] = useState(value ?? "");
  const [testing, setTesting] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  const refresh = async () => {
    setRefreshing(true);
    try {
      qc.setQueryData(["java"], await api.detectJava(true));
    } catch (e) {
      notify.error(e);
    } finally {
      setRefreshing(false);
    }
  };

  const test = async () => {
    setTesting(true);
    try {
      const j = await api.testJava(custom);
      notify.success(t("javaSettings.testOk", { version: j.version, arch: j.arch }));
      onChange(j.path);
    } catch (e) {
      notify.error(e);
    } finally {
      setTesting(false);
    }
  };

  const rows = list.data ?? [];

  return (
    <div className="flex flex-col gap-2">
      <div className="overflow-hidden rounded-md border border-border" role="radiogroup">
        <label className={clsx("flex cursor-pointer items-center gap-2 px-3 py-2 text-sm", value === null && "bg-accent/10")}>
          <input type="radio" checked={value === null} onChange={() => onChange(null)} className="accent-[var(--accent)]" />
          {t("javaSettings.auto")}
        </label>
        {(list.isLoading || refreshing) && <p className="border-t border-border px-3 py-2 text-sm text-muted">{t("javaSettings.searching")}</p>}
        {list.isSuccess && rows.length === 0 && !refreshing && (
          <p className="border-t border-border px-3 py-2 text-sm text-muted">{t("javaSettings.noneFound")}</p>
        )}
        {rows.map((j) => (
          <label
            key={j.path}
            className={clsx(
              "flex cursor-pointer items-center gap-2 border-t border-border px-3 py-2 text-sm",
              value === j.path && "bg-accent/10",
            )}
          >
            <input type="radio" checked={value === j.path} onChange={() => onChange(j.path)} className="accent-[var(--accent)]" />
            <span className="w-14 font-semibold">Java {j.major}</span>
            <span className="w-28 text-muted">
              {j.version} · {j.arch}
            </span>
            <span className="min-w-0 flex-1 truncate text-xs text-muted" title={j.path}>
              {j.path}
            </span>
            {j.managed && <span className="text-xs text-accent">{t("javaSettings.managed")}</span>}
          </label>
        ))}
      </div>
      <div className="flex gap-2">
        <Input
          value={custom}
          placeholder={t("javaSettings.custom")}
          onChange={(e) => setCustom(e.target.value)}
          className="flex-1 font-mono text-xs"
        />
        <Button icon={<CheckCircle2 size={14} />} disabled={!custom.trim() || testing} onClick={test}>
          {t("javaSettings.test")}
        </Button>
        <Button
          variant="ghost"
          icon={<RefreshCw size={14} className={clsx(refreshing && "animate-spin")} />}
          disabled={refreshing}
          onClick={refresh}
        >
          {t("javaSettings.refresh")}
        </Button>
      </div>
      {value !== null && !rows.some((j) => j.path === value) && (
        <p className="text-xs text-muted">
          {t("javaSettings.custom")}: <code>{value}</code>
        </p>
      )}
    </div>
  );
}
