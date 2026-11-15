import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Copy, ExternalLink, Loader2, TriangleAlert } from "lucide-react";
import { Dialog } from "@/components/ui/Dialog";
import { Button } from "@/components/ui";
import { api, inTauri, listen } from "@/lib/ipc";
import { EVENTS } from "@/lib/events";
import { isBackendError, toUserError } from "@/lib/errors";
import type { DeviceCode, LoginStage } from "@/lib/types";
import { notify } from "@/stores/toasts";

type Phase =
  | { kind: "loading" }
  | { kind: "notConfigured" }
  | { kind: "code"; code: DeviceCode; stage: LoginStage }
  | { kind: "error"; title: string; details?: string };

export function MsaLoginDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  // Номер попытки: ответы от отменённых попыток игнорируются.
  const attempt = useRef(0);

  const start = useCallback(async () => {
    const my = ++attempt.current;
    setPhase({ kind: "loading" });
    try {
      const status = await api.msaStatus();
      if (!status.configured) return setPhase({ kind: "notConfigured" });
      const code = await api.msaBegin();
      if (my !== attempt.current) return;
      setPhase({ kind: "code", code, stage: "waiting" });
      const acc = await api.msaFinish();
      if (my !== attempt.current) return;
      notify.success(t("accounts.msaDone", { name: acc.username }));
      void qc.invalidateQueries({ queryKey: ["accounts"] });
      onClose();
    } catch (e) {
      if (my !== attempt.current || (isBackendError(e) && e.kind === "cancelled")) return;
      setPhase({ kind: "error", ...toUserError(e) });
    }
  }, [onClose, qc, t]);

  useEffect(() => {
    if (!open) return;
    void start();
    const un = listen<LoginStage>(EVENTS.loginStage, (stage) =>
      setPhase((p) => (p.kind === "code" ? { ...p, stage } : p)),
    );
    return () => {
      attempt.current++;
      void api.msaCancel().catch(() => {});
      void un.then((f) => f());
    };
  }, [open, start]);

  const copyAndOpen = async (code: DeviceCode) => {
    try {
      await navigator.clipboard.writeText(code.userCode);
      notify.info(t("accounts.codeCopied"));
    } catch {
      /* буфер обмена недоступен — код и так виден на экране */
    }
    try {
      if (inTauri) await openUrl(code.verificationUri);
      else window.open(code.verificationUri, "_blank", "noopener");
    } catch (e) {
      notify.error(e);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} title={t("accounts.msaTitle")} width="sm">
      {phase.kind === "loading" && (
        <div className="flex justify-center py-8">
          <Loader2 className="animate-spin text-muted" />
        </div>
      )}

      {phase.kind === "notConfigured" && (
        <div className="flex flex-col gap-3">
          <p className="flex gap-2 text-sm">
            <TriangleAlert size={18} className="shrink-0 text-warning" />
            {t("accounts.msaNotConfigured")}
          </p>
          <Button
            onClick={() => {
              onClose();
              navigate("/settings#general");
            }}
          >
            {t("toolbar.settings")}
          </Button>
        </div>
      )}

      {phase.kind === "code" && (
        <div className="flex flex-col items-center gap-4 text-center">
          <p className="text-sm">{t("accounts.msaStep1")}</p>
          <p className="text-xs text-muted">{phase.code.verificationUri}</p>
          <div
            className="rounded-lg border-2 border-dashed border-accent px-6 py-3 font-mono text-3xl font-bold tracking-widest select-text"
            aria-label="code"
          >
            {phase.code.userCode}
          </div>
          <Button variant="primary" icon={<ExternalLink size={15} />} onClick={() => copyAndOpen(phase.code)}>
            {t("accounts.copyAndOpen")}
          </Button>
          <button
            type="button"
            className="flex items-center gap-1 text-xs text-muted hover:text-text"
            onClick={() => navigator.clipboard.writeText(phase.code.userCode).then(() => notify.info(t("accounts.codeCopied")))}
          >
            <Copy size={12} /> {t("common.copy")}
          </button>
          <div className="flex items-center gap-2 text-sm text-muted" role="status">
            <Loader2 size={15} className="animate-spin" />
            {t(`accounts.stage.${phase.stage}`)}
          </div>
          <p className="text-xs text-muted">
            {t("accounts.codeExpires", { min: Math.round(phase.code.expiresIn / 60) })} {t("accounts.msaHint")}
          </p>
        </div>
      )}

      {phase.kind === "error" && (
        <div className="flex flex-col gap-3">
          <p className="flex gap-2 text-sm text-danger" role="alert">
            <TriangleAlert size={18} className="shrink-0" />
            {phase.title}
          </p>
          {phase.details && (
            <pre className="max-h-32 overflow-auto rounded bg-surface-2 p-2 text-xs whitespace-pre-wrap select-text">{phase.details}</pre>
          )}
          <Button variant="primary" onClick={() => void start()}>
            {t("accounts.retry")}
          </Button>
        </div>
      )}
    </Dialog>
  );
}
