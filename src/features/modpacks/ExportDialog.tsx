import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { File, Folder } from "lucide-react";
import { Dialog } from "@/components/ui/Dialog";
import { Button, Input, Segmented } from "@/components/ui";
import { api, inTauri } from "@/lib/ipc";
import type { Instance, PackFormat } from "@/lib/types";
import { notify } from "@/stores/toasts";

/** Экспорт инстанса в .mrpack или zip CurseForge. */
export function ExportDialog({ inst, open, onClose }: { inst: Instance; open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<PackFormat>("mrpack");
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [summary, setSummary] = useState("");
  const [include, setInclude] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const candidates = useQuery({ queryKey: ["export-candidates", inst.id], queryFn: () => api.exportCandidates(inst.id), enabled: open });
  const cf = useQuery({ queryKey: ["curseforge-status"], queryFn: api.curseforgeStatus, enabled: open });

  useEffect(() => {
    if (!open) return;
    setName(inst.modpack?.name ?? inst.name);
    setVersion(inst.modpack?.version || "1.0.0");
    setSummary("");
    setFormat("mrpack");
  }, [open, inst]);
  useEffect(() => {
    if (candidates.data) setInclude(new Set(candidates.data.filter((c) => c.default).map((c) => c.name)));
  }, [candidates.data]);

  const toggle = (n: string) => {
    const next = new Set(include);
    if (next.has(n)) next.delete(n);
    else next.add(n);
    setInclude(next);
  };

  const submit = async () => {
    const ext = format === "mrpack" ? "mrpack" : "zip";
    const fileName = `${name.trim()} ${version.trim()}`.trim().replace(/[\\/:*?"<>|]+/g, "_") + `.${ext}`;
    const path = inTauri
      ? await saveDialog({ defaultPath: fileName, filters: [{ name: format === "mrpack" ? "Modrinth (.mrpack)" : "CurseForge (.zip)", extensions: [ext] }] })
      : `C:/Users/me/Desktop/${fileName}`;
    if (!path) return;
    setBusy(true);
    try {
      await api.exportModpack(inst.id, {
        format,
        path,
        name: name.trim(),
        version: version.trim(),
        summary: summary.trim() || null,
        include: [...include],
      });
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
      title={t("export.title", { name: inst.name })}
      width="lg"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={busy || !name.trim() || include.size === 0} onClick={() => void submit()}>
            {t("export.save")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Segmented
          value={format}
          onValueChange={setFormat}
          options={[
            { value: "mrpack", label: "Modrinth (.mrpack)" },
            { value: "curseforge", label: "CurseForge (.zip)" },
          ]}
        />
        <p className="text-xs text-muted">
          {format === "mrpack" ? t("export.mrpackHint") : cf.data?.configured ? t("export.cfHint") : t("export.cfNoKey")}
        </p>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5 text-sm font-medium">
            {t("export.name")}
            <Input value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="flex flex-col gap-1.5 text-sm font-medium">
            {t("export.version")}
            <Input value={version} maxLength={32} onChange={(e) => setVersion(e.target.value)} />
          </label>
        </div>
        {format === "mrpack" && (
          <label className="flex flex-col gap-1.5 text-sm font-medium">
            {t("export.summary")}
            <Input value={summary} maxLength={200} onChange={(e) => setSummary(e.target.value)} />
          </label>
        )}
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">{t("export.include")}</span>
          <div className="grid max-h-56 grid-cols-2 gap-x-4 gap-y-1 overflow-y-auto rounded-md border border-border p-2">
            {candidates.data?.map((c) => (
              <label key={c.name} className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={include.has(c.name)} onChange={() => toggle(c.name)} className="accent-[var(--accent)]" />
                {c.isDir ? <Folder size={14} className="text-muted" /> : <File size={14} className="text-muted" />}
                <span className="truncate">{c.name}</span>
              </label>
            ))}
            {candidates.data?.length === 0 && <p className="text-sm text-muted">{t("export.empty")}</p>}
          </div>
          <p className="text-xs text-muted">{t("export.includeHint")}</p>
        </div>
      </div>
    </Dialog>
  );
}
