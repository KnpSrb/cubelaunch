import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { File, Folder } from "lucide-react";
import { Dialog } from "@/components/ui/Dialog";
import { Button, Input } from "@/components/ui";
import { api, inTauri } from "@/lib/ipc";
import type { Instance } from "@/lib/types";
import { notify } from "@/stores/toasts";

/** Копия инстанса: имя, группа, копировать ли миры. */
export function CloneDialog({
  inst,
  open,
  onClose,
  onCloned,
}: {
  inst: Instance | null;
  open: boolean;
  onClose: () => void;
  onCloned?: (inst: Instance) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const groups = useQuery({ queryKey: ["groups"], queryFn: api.listGroups, enabled: open });
  const [name, setName] = useState("");
  const [group, setGroup] = useState("");
  const [saves, setSaves] = useState(true);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open || !inst) return;
    setName(t("clone.defaultName", { name: inst.name }).slice(0, 64));
    setGroup(inst.group ?? "");
    setSaves(true);
  }, [open, inst, t]);

  if (!inst) return null;
  const submit = async () => {
    setBusy(true);
    try {
      const created = await api.cloneInstance(inst.id, name.trim(), group.trim() || null, saves);
      void queryClient.invalidateQueries({ queryKey: ["instances"] });
      notify.info(t("clone.started", { name: created.name }));
      onCloned?.(created);
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
      title={t("clone.title", { name: inst.name })}
      width="sm"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={busy || !name.trim()} onClick={() => void submit()}>
            {t("clone.submit")}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) void submit();
        }}
      >
        <label className="flex flex-col gap-1.5 text-sm font-medium">
          {t("newInstance.name")}
          <Input value={name} maxLength={64} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.target.select()} />
        </label>
        <label className="flex flex-col gap-1.5 text-sm font-medium">
          {t("newInstance.group")}
          <Input value={group} maxLength={64} list="clone-groups" placeholder={t("newInstance.groupPlaceholder")} onChange={(e) => setGroup(e.target.value)} />
          <datalist id="clone-groups">
            {groups.data?.map((g) => <option key={g} value={g} />)}
          </datalist>
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={saves} onChange={(e) => setSaves(e.target.checked)} className="accent-[var(--accent)]" />
          {t("clone.copySaves")}
        </label>
        <p className="text-xs text-muted">{t("clone.hint")}</p>
      </form>
    </Dialog>
  );
}

/** Экспорт инстанса целиком в zip Cubelaunch. */
export function InstanceExportDialog({ inst, open, onClose }: { inst: Instance | null; open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const candidates = useQuery({
    queryKey: ["instance-export-candidates", inst?.id],
    queryFn: () => api.instanceExportCandidates(inst!.id),
    enabled: open && !!inst,
  });
  const [include, setInclude] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open && candidates.data) setInclude(new Set(candidates.data.filter((c) => c.default).map((c) => c.name)));
  }, [open, candidates.data]);

  if (!inst) return null;
  const toggle = (n: string) => {
    const next = new Set(include);
    if (next.has(n)) next.delete(n);
    else next.add(n);
    setInclude(next);
  };
  const submit = async () => {
    const fileName = `${inst.name.replace(/[\\/:*?"<>|]+/g, "_")}.zip`;
    const path = inTauri
      ? await saveDialog({ defaultPath: fileName, filters: [{ name: t("instanceExport.filter"), extensions: ["zip"] }] })
      : `C:/Users/me/Desktop/${fileName}`;
    if (!path) return;
    setBusy(true);
    try {
      await api.exportInstance(inst.id, path, [...include]);
      notify.info(t("export.started"));
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
      title={t("instanceExport.title", { name: inst.name })}
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={busy} onClick={() => void submit()}>
            {t("export.save")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <p className="text-sm text-muted">{t("instanceExport.hint")}</p>
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">{t("export.include")}</span>
          <div className="grid max-h-64 grid-cols-2 gap-x-4 gap-y-1 overflow-y-auto rounded-md border border-border p-2">
            {candidates.data?.map((c) => (
              <label key={c.name} className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={include.has(c.name)} onChange={() => toggle(c.name)} className="accent-[var(--accent)]" />
                {c.isDir ? <Folder size={14} className="text-muted" /> : <File size={14} className="text-muted" />}
                <span className="truncate">{c.name}</span>
              </label>
            ))}
            {candidates.data?.length === 0 && <p className="text-sm text-muted">{t("export.empty")}</p>}
          </div>
          <p className="text-xs text-muted">{t("instanceExport.includeHint")}</p>
        </div>
      </div>
    </Dialog>
  );
}
