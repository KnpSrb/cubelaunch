import { Outlet, useLocation, useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  ArrowLeft,
  Box,
  Check,
  CircleHelp,
  FolderOpen,
  Plus,
  Settings as SettingsIcon,
  UserRound,
} from "lucide-react";
import { Button } from "@/components/ui";
import { Menu } from "@/components/ui/Menu";
import { Toaster } from "./Toaster";
import { api } from "@/lib/ipc";
import type { LauncherFolder } from "@/lib/types";
import { notify } from "@/stores/toasts";
import { useHotkeys } from "@/app/hotkeys";
import { useUi } from "@/stores/ui";
import { NewInstanceDialog } from "@/features/instances/NewInstanceDialog";
import { DownloadsButton } from "./DownloadsButton";
import { UpdateBanner, useStartupUpdateCheck } from "@/features/updates/updates";
import { AccountsDialog, useAccounts } from "@/features/accounts/AccountsDialog";
import { SkinHead, useSkin } from "@/features/accounts/skin";

export function AppShell() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const onHome = pathname === "/";
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const instances = useQuery({ queryKey: ["instances"], queryFn: api.listInstances });
  const { newInstanceOpen, importPath, openNewInstance, closeNewInstance, selectInstance, accountsOpen, setAccountsOpen } = useUi();
  const accounts = useAccounts();
  const accountList = accounts.data?.accounts ?? [];
  const current = accountList.find((a) => a.id === accounts.data?.defaultId);
  const currentSkin = useSkin(current);
  const groups = [...new Set((instances.data ?? []).map((i) => i.group).filter((g): g is string => !!g))].sort();

  const newInstance = () => {
    if (!onHome) navigate("/");
    openNewInstance();
  };
  useHotkeys({
    "mod+n": newInstance,
    "mod+,": () => navigate("/settings"),
    escape: () => !onHome && navigate("/"),
  });

  useStartupUpdateCheck();

  const openFolder = (f: LauncherFolder) => api.openLauncherFolder(f).catch(notify.error);
  const folders: LauncherFolder[] = ["data", "instances", "logs", "java"];

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-12 shrink-0 items-center gap-1.5 border-b border-border bg-surface px-3">
        {onHome ? (
          <div className="mr-3 flex items-center gap-2 font-semibold">
            <Box size={20} className="text-accent" />
            {t("app.name")}
          </div>
        ) : (
          <Button variant="ghost" icon={<ArrowLeft size={16} />} onClick={() => navigate("/")}>
            {t("toolbar.back")}
          </Button>
        )}

        <Button variant="primary" icon={<Plus size={16} />} onClick={newInstance} title="Ctrl+N">
          {t("toolbar.addInstance")}
        </Button>
        <Menu
          trigger={({ toggle }) => (
            <Button variant="ghost" icon={<FolderOpen size={16} />} onClick={toggle}>
              {t("toolbar.folders")}
            </Button>
          )}
          items={folders.map((f) => ({ label: t(`folders.${f}`), onSelect: () => openFolder(f) }))}
        />
        <Button
          variant="ghost"
          icon={<SettingsIcon size={16} />}
          onClick={() => navigate("/settings")}
          title="Ctrl+,"
        >
          {t("toolbar.settings")}
        </Button>
        <Button
          variant="ghost"
          icon={<CircleHelp size={16} />}
          onClick={() => navigate("/settings#about")}
        >
          {t("toolbar.help")}
        </Button>

        <div className="ml-auto">
          <Menu
            align="right"
            trigger={({ toggle }) => (
              <Button variant="secondary" icon={<SkinHead skin={currentSkin.data?.skin} size={18} />} onClick={toggle}>
                {current ? current.username : t("account.none")}
                {current?.kind === "offline" && (
                  <span className="rounded bg-surface-3 px-1.5 text-[11px] text-warning">{t("accounts.offline")}</span>
                )}
              </Button>
            )}
            items={[
              ...accountList.map((a) => ({
                label: a.username + (a.kind === "offline" ? ` (${t("accounts.offline")})` : ""),
                icon: a.id === current?.id ? <Check size={15} /> : <span className="inline-block w-[15px]" />,
                onSelect: () => api.setDefaultAccount(a.id).catch(notify.error),
              })),
              ...(accountList.length ? ["separator" as const] : []),
              { label: t("account.manage"), icon: <UserRound size={15} />, onSelect: () => setAccountsOpen(true) },
            ]}
          />
        </div>
      </header>

      <UpdateBanner />

      <main className="min-h-0 flex-1">
        <Outlet />
      </main>

      <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-surface px-3 text-xs text-muted">
        <span>
          {t("app.name")} {info.data?.version}
        </span>
        <span className="truncate" title={info.data?.dataDir}>
          {info.data?.dataDir}
        </span>
        <DownloadsButton />
      </footer>

      <NewInstanceDialog
        open={newInstanceOpen}
        onClose={closeNewInstance}
        importPath={importPath}
        groups={groups}
        onCreated={(inst) => {
          void instances.refetch();
          selectInstance(inst.id);
        }}
      />
      <AccountsDialog open={accountsOpen} onClose={() => setAccountsOpen(false)} />
      <Toaster />
    </div>
  );
}
