import { useNavigate } from "react-router";
import { api } from "@/lib/ipc";
import { isBackendError } from "@/lib/errors";
import type { Instance } from "@/lib/types";
import { useSettings } from "@/stores/settings";
import { useUi } from "@/stores/ui";
import { notify } from "@/stores/toasts";
import i18n from "@/i18n";

/** Запуск и остановка игры с общей обработкой ошибок. */
export function usePlay() {
  const navigate = useNavigate();
  const setAccountsOpen = useUi((s) => s.setAccountsOpen);

  const play = async (inst: Instance) => {
    try {
      await api.launchInstance(inst.id);
      if (useSettings.getState().settings?.game.showLogOnLaunch) {
        navigate(`/instance/${encodeURIComponent(inst.id)}/log`);
      }
    } catch (e) {
      if (isBackendError(e) && e.kind === "no_account") {
        notify.error(e, { label: i18n.t("accounts.addOffline"), onClick: () => setAccountsOpen(true) });
        setAccountsOpen(true);
      } else if (isBackendError(e) && e.kind === "auth_relogin") {
        notify.error(e, { label: i18n.t("accounts.microsoft"), onClick: () => setAccountsOpen(true) });
      } else {
        notify.error(e);
      }
    }
  };

  const stop = (inst: Instance) => api.killGame(inst.id).catch(notify.error);

  return { play, stop };
}
