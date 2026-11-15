import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { Dialog } from "@/components/ui/Dialog";
import { Button, Select } from "@/components/ui";
import { api } from "@/lib/ipc";
import type { Instance, LoaderSpec } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { LoaderPicker } from "./LoaderPicker";

/** Смена версии Minecraft и/или загрузчика у существующего инстанса. */
export function ChangeVersionDialog({ inst, open, onClose }: { inst: Instance; open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const manifest = useQuery({
    queryKey: ["version-manifest"],
    queryFn: () => api.getVersionManifest(false),
    enabled: open,
    staleTime: 10 * 60_000,
  });
  const [minecraft, setMinecraft] = useState(inst.minecraft);
  const [loader, setLoader] = useState<LoaderSpec | null>(inst.loader ?? null);
  const [snapshots, setSnapshots] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setMinecraft(inst.minecraft);
    setLoader(inst.loader ?? null);
    setSnapshots(false);
  }, [open, inst.minecraft, inst.loader]);

  const options = useMemo(() => {
    const list = (manifest.data?.versions ?? [])
      .filter((v) => v.type === "release" || (snapshots && v.type === "snapshot") || v.id === inst.minecraft)
      .map((v) => ({ value: v.id, label: v.type === "release" ? v.id : `${v.id} (${t(`versionTypes.${v.type}`)})` }));
    if (!list.some((o) => o.value === minecraft)) list.unshift({ value: minecraft, label: minecraft });
    return list;
  }, [manifest.data, snapshots, inst.minecraft, minecraft, t]);

  const unchanged =
    minecraft === inst.minecraft &&
    (loader?.kind ?? null) === (inst.loader?.kind ?? null) &&
    (loader?.version ?? null) === (inst.loader?.version ?? null);
  const downgrade = minecraft !== inst.minecraft;

  const apply = async () => {
    if (busy || unchanged || (loader && !loader.version)) return;
    setBusy(true);
    try {
      await api.setInstanceVersion(inst.id, minecraft, loader);
      notify.success(t("changeVersion.started"));
      onClose();
    } catch (e) {
      notify.error(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t("changeVersion.title", { name: inst.name })}
      width="lg"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={busy || unchanged || (!!loader && !loader.version)} onClick={() => void apply()}>
            {t("changeVersion.apply")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">{t("instancePage.minecraft")}</span>
          <div className="flex items-center gap-3">
            <Select
              aria-label={t("instancePage.minecraft")}
              className="min-w-56"
              value={minecraft}
              onValueChange={setMinecraft}
              options={options}
            />
            <label className="flex items-center gap-1.5 text-sm">
              <input
                type="checkbox"
                checked={snapshots}
                onChange={(e) => setSnapshots(e.target.checked)}
                className="accent-[var(--accent)]"
              />
              {t("versionFilters.snapshot")}
            </label>
          </div>
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">{t("newInstance.loader")}</span>
          <LoaderPicker minecraft={minecraft} value={loader} onChange={setLoader} />
        </div>
        <p className="flex items-start gap-2 rounded-md bg-surface-2 p-3 text-xs text-muted">
          <TriangleAlert size={14} className="mt-0.5 shrink-0 text-warning" />
          <span>{downgrade ? t("changeVersion.warnVersion") : t("changeVersion.warnLoader")}</span>
        </p>
      </div>
    </Dialog>
  );
}
