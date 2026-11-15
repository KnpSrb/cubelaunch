import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileArchive, FolderOpen } from "lucide-react";
import { Button, Input } from "@/components/ui";
import { api, inTauri } from "@/lib/ipc";
import { LOADER_NAMES, type Instance, type PackPreview } from "@/lib/types";
import { notify } from "@/stores/toasts";

export const PACK_FILTERS = [{ name: "Modpack / instance (.mrpack, .zip)", extensions: ["mrpack", "zip"] }];

const FORMAT_NAMES = {
  mrpack: "Modrinth (.mrpack)",
  curseforge: "CurseForge (zip)",
  cubelaunch: "Cubelaunch (zip)",
  prism: "Prism Launcher / MultiMC (zip)",
} as const;

/** Выбор файла модпака в системном диалоге. */
export async function pickPackFile(): Promise<string | null> {
  if (!inTauri) return "C:/Users/me/Downloads/Example Pack 1.2.mrpack";
  const p = await openDialog({ multiple: false, directory: false, filters: PACK_FILTERS });
  return typeof p === "string" ? p : null;
}

/** Импорт .mrpack / zip CurseForge / zip инстанса Cubelaunch или Prism как нового инстанса. */
export function ImportPack({
  initialPath,
  group,
  onImported,
}: {
  initialPath: string | null;
  group: string | null;
  onImported: (inst: Instance) => void;
}) {
  const { t } = useTranslation();
  const [path, setPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<PackPreview | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    async (p: string) => {
      setPath(p);
      setPreview(null);
      setBusy(true);
      try {
        const pv = await api.inspectModpack(p);
        setPreview(pv);
        setName(pv.name);
      } catch (e) {
        notify.error(e);
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (initialPath) void load(initialPath);
  }, [initialPath, load]);

  const choose = async () => {
    const p = await pickPackFile();
    if (p) void load(p);
  };

  const isInstance = preview?.format === "cubelaunch" || preview?.format === "prism";
  const submit = async () => {
    if (!path || !preview) return;
    setBusy(true);
    try {
      const inst = await api.importModpack(path, name.trim() || null, group);
      notify.success(t(isInstance ? "instanceImport.started" : "modpacks.installing", { name: inst.name }));
      onImported(inst);
    } catch (e) {
      notify.error(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <Button icon={<FolderOpen size={15} />} onClick={() => void choose()} disabled={busy}>
          {t("modpacks.chooseFile")}
        </Button>
        <span className="min-w-0 flex-1 truncate text-sm text-muted" title={path ?? undefined}>
          {path ?? t("modpacks.importHint")}
        </span>
      </div>
      {busy && !preview && <p className="text-sm text-muted">{t("common.loading")}</p>}
      {preview && (
        <div className="flex flex-col gap-3 rounded-md border border-border p-4">
          <div className="flex items-start gap-3">
            <FileArchive size={36} className="shrink-0 text-accent" />
            <div className="min-w-0 flex-1">
              <p className="font-semibold">
                {preview.name} {preview.version && <span className="font-normal text-muted">{preview.version}</span>}
              </p>
              <p className="text-sm text-muted">
                {FORMAT_NAMES[preview.format]} · Minecraft {preview.minecraft}
                {preview.loader && ` · ${LOADER_NAMES[preview.loader]} ${preview.loaderVersion ?? ""}`}
              </p>
              <p className="text-sm text-muted">
                {isInstance
                  ? t("instanceImport.contents", { count: preview.overrides }) + (preview.hasSaves ? ` · ${t("instanceImport.withSaves")}` : "")
                  : t("modpacks.contents", { files: preview.files, overrides: preview.overrides })}
              </p>
              {preview.summary && <p className="mt-1 text-sm">{preview.summary}</p>}
            </div>
          </div>
          {preview.format === "curseforge" && preview.files > 0 && <p className="text-xs text-muted">{t("modpacks.cfNeedsKey")}</p>}
          {isInstance && <p className="text-xs text-muted">{t("instanceImport.hint")}</p>}
          {preview.warnings.length > 0 && (
            <p className="text-xs text-warning">{t("instanceImport.warnings", { list: preview.warnings.join(", ") })}</p>
          )}
          <label className="flex max-w-md flex-col gap-1.5 text-sm font-medium">
            {t("newInstance.name")}
            <Input value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
          </label>
          <Button variant="primary" className="self-start" disabled={busy || !name.trim()} onClick={() => void submit()}>
            {t("modpacks.import")}
          </Button>
        </div>
      )}
    </div>
  );
}
