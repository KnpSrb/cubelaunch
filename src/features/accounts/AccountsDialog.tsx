import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import clsx from "clsx";
import { Check, Loader2, RefreshCw, Trash2, TriangleAlert } from "lucide-react";
import { Dialog } from "@/components/ui/Dialog";
import { Button, Input } from "@/components/ui";
import { api } from "@/lib/ipc";
import type { Account } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { MsaLoginDialog } from "./MsaLoginDialog";
import { SkinHead, SkinViewer3D, useSkin } from "./skin";

export function useAccounts() {
  return useQuery({ queryKey: ["accounts"], queryFn: api.listAccounts });
}

function AccountRow({
  a,
  isDefault,
  selected,
  onSelect,
  onChanged,
}: {
  a: Account;
  isDefault: boolean;
  selected: boolean;
  onSelect: () => void;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const skin = useSkin(a);
  const [refreshing, setRefreshing] = useState(false);

  const refresh = async () => {
    setRefreshing(true);
    try {
      const acc = await api.refreshAccount(a.id);
      notify.success(t("accounts.refreshed", { name: acc.username }));
      onChanged();
    } catch (e) {
      notify.error(e);
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div
      role="option"
      aria-selected={selected}
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => e.key === "Enter" && onSelect()}
      className={clsx(
        "flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2",
        selected ? "border-accent bg-accent/5" : "border-border hover:bg-surface-2",
      )}
    >
      <SkinHead skin={skin.data?.skin} size={24} />
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium">{a.username}</p>
        {a.kind === "offline" ? (
          <p className="text-xs text-warning">{t("accounts.offlineBadge")}</p>
        ) : (
          <p className="text-xs text-muted">{t("accounts.microsoftBadge")}</p>
        )}
      </div>
      {isDefault ? (
        <span className="flex items-center gap-1 text-xs text-accent">
          <Check size={14} />
          {t("accounts.default")}
        </span>
      ) : (
        <Button
          size="sm"
          onClick={(e) => {
            e.stopPropagation();
            api.setDefaultAccount(a.id).then(onChanged).catch(notify.error);
          }}
        >
          {t("accounts.makeDefault")}
        </Button>
      )}
      {a.kind === "microsoft" && (
        <button
          type="button"
          aria-label={t("accounts.refresh")}
          title={t("accounts.refresh")}
          disabled={refreshing}
          className="text-muted hover:text-text"
          onClick={(e) => {
            e.stopPropagation();
            void refresh();
          }}
        >
          <RefreshCw size={15} className={clsx(refreshing && "animate-spin")} />
        </button>
      )}
      <button
        type="button"
        aria-label={t("accounts.remove")}
        title={t("accounts.remove")}
        className="text-muted hover:text-danger"
        onClick={(e) => {
          e.stopPropagation();
          api.removeAccount(a.id).then(onChanged).catch(notify.error);
        }}
      >
        <Trash2 size={15} />
      </button>
    </div>
  );
}

function SkinPanel({ account }: { account?: Account }) {
  const { t } = useTranslation();
  const skin = useSkin(account);
  return (
    <div className="flex w-60 shrink-0 flex-col items-center justify-center gap-2 rounded-md border border-border p-3">
      <p className="self-start text-sm font-medium">{t("accounts.skin")}</p>
      {!account?.skin ? (
        <p className="py-10 text-center text-xs text-muted">{t("accounts.noSkin")}</p>
      ) : skin.isLoading ? (
        <p className="flex items-center gap-2 py-10 text-xs text-muted">
          <Loader2 size={14} className="animate-spin" /> {t("accounts.skinLoading")}
        </p>
      ) : skin.data ? (
        <SkinViewer3D data={skin.data} width={200} height={260} />
      ) : (
        <p className="py-10 text-center text-xs text-danger">{t("errors.network")}</p>
      )}
    </div>
  );
}

export function AccountsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const accounts = useAccounts();
  const [name, setName] = useState("");
  const [msaOpen, setMsaOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const valid = /^[A-Za-z0-9_]{3,16}$/.test(name.trim());

  const list = accounts.data?.accounts ?? [];
  const defaultId = accounts.data?.defaultId;
  const selected = list.find((a) => a.id === selectedId) ?? list.find((a) => a.id === defaultId);
  useEffect(() => {
    if (open) setSelectedId(null);
  }, [open]);

  const refresh = () => qc.invalidateQueries({ queryKey: ["accounts"] });
  const add = async () => {
    if (!valid) return;
    try {
      const acc = await api.addOfflineAccount(name.trim());
      notify.success(t("accounts.added", { name: acc.username }));
      setName("");
      void refresh();
    } catch (e) {
      notify.error(e);
    }
  };

  return (
    <>
      <Dialog open={open && !msaOpen} onClose={onClose} title={t("accounts.title")} width="lg">
        <div className="flex gap-4">
          <div className="flex min-w-0 flex-1 flex-col gap-4">
            <div className="flex flex-col gap-1" role="listbox" aria-label={t("accounts.title")}>
              {list.length === 0 && <p className="text-sm text-muted">{t("accounts.empty")}</p>}
              {list.map((a) => (
                <AccountRow
                  key={a.id}
                  a={a}
                  isDefault={a.id === defaultId}
                  selected={a.id === selected?.id}
                  onSelect={() => setSelectedId(a.id)}
                  onChanged={() => void refresh()}
                />
              ))}
            </div>

            <Button variant="primary" onClick={() => setMsaOpen(true)}>
              {t("accounts.microsoft")}
            </Button>

            <form
              className="flex flex-col gap-2 rounded-md border border-border p-3"
              onSubmit={(e) => {
                e.preventDefault();
                void add();
              }}
            >
              <p className="text-sm font-medium">{t("accounts.addOffline")}</p>
              <div className="flex gap-2">
                <Input
                  value={name}
                  maxLength={16}
                  placeholder={t("accounts.nickname")}
                  onChange={(e) => setName(e.target.value)}
                  className="flex-1"
                  aria-describedby="nick-hint"
                />
                <Button type="submit" disabled={!valid}>
                  {t("accounts.add")}
                </Button>
              </div>
              <p id="nick-hint" className="text-xs text-muted">
                {t("accounts.nicknameHint")}
              </p>
              <p className="flex gap-1.5 text-xs text-warning">
                <TriangleAlert size={14} className="shrink-0" />
                {t("accounts.offlineWarning")}
              </p>
            </form>
          </div>
          <SkinPanel account={selected} />
        </div>
      </Dialog>
      <MsaLoginDialog open={open && msaOpen} onClose={() => setMsaOpen(false)} />
    </>
  );
}
