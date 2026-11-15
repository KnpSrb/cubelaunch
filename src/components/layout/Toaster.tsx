import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, Info, TriangleAlert, X } from "lucide-react";
import clsx from "clsx";
import { useToasts, type Toast } from "@/stores/toasts";

function ToastView({ toast }: { toast: Toast }) {
  const { t } = useTranslation();
  const dismiss = useToasts((s) => s.dismiss);
  const [showDetails, setShowDetails] = useState(false);
  const Icon = toast.kind === "error" ? TriangleAlert : toast.kind === "success" ? CheckCircle2 : Info;

  return (
    <div
      role={toast.kind === "error" ? "alert" : "status"}
      className="pointer-events-auto w-96 rounded-lg border border-border bg-surface p-3 shadow-panel"
    >
      <div className="flex items-start gap-2.5">
        <Icon
          size={18}
          className={clsx(
            "mt-0.5 shrink-0",
            toast.kind === "error" && "text-danger",
            toast.kind === "success" && "text-accent",
            toast.kind === "info" && "text-muted",
          )}
        />
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">{toast.title}</p>
          {toast.action && (
            <button
              type="button"
              className="mt-1 mr-3 text-xs font-medium text-accent hover:underline"
              onClick={() => {
                toast.action!.onClick();
                dismiss(toast.id);
              }}
            >
              {toast.action.label}
            </button>
          )}
          {toast.details && (
            <>
              <button
                type="button"
                className="mt-1 text-xs text-muted underline-offset-2 hover:underline"
                onClick={() => setShowDetails((v) => !v)}
              >
                {t("common.details")}
              </button>
              {showDetails && (
                <pre className="mt-1.5 max-h-40 overflow-auto rounded bg-surface-2 p-2 text-xs whitespace-pre-wrap select-text">
                  {toast.details}
                </pre>
              )}
            </>
          )}
        </div>
        <button
          type="button"
          aria-label={t("common.close")}
          className="text-muted hover:text-text"
          onClick={() => dismiss(toast.id)}
        >
          <X size={16} />
        </button>
      </div>
    </div>
  );
}

export function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  return (
    <div className="pointer-events-none fixed right-4 bottom-10 z-[100] flex flex-col gap-2">
      {toasts.map((t) => (
        <ToastView key={t.id} toast={t} />
      ))}
    </div>
  );
}
